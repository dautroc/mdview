use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

pub const QUIET_PERIOD: Duration = Duration::from_millis(100);

/// Collapses a burst of filesystem events into one reload.
///
/// Deliberately takes `now` as a parameter rather than calling
/// `Instant::now()` internally, so the timing behavior is testable without
/// sleeping.
pub struct Debouncer {
    quiet: Duration,
    pending_since: Option<Instant>,
}

impl Debouncer {
    pub fn new(quiet: Duration) -> Self {
        Self {
            quiet,
            pending_since: None,
        }
    }

    /// Note that a filesystem event arrived, restarting the quiet period.
    pub fn record(&mut self, now: Instant) {
        self.pending_since = Some(now);
    }

    /// True exactly once per burst, once the quiet period has elapsed.
    pub fn take_if_ready(&mut self, now: Instant) -> bool {
        match self.pending_since {
            Some(since) if now.duration_since(since) >= self.quiet => {
                self.pending_since = None;
                true
            }
            _ => false,
        }
    }
}

/// The absolute path `notify` will report for `path`.
///
/// `notify` reports absolute, symlink-resolved paths. A path given on argv
/// (`mdview notes.md`) is relative, and a review file is watched from the
/// moment its document opens -- which is usually before anyone has commented,
/// so there is no file there to resolve. Canonicalizing the *directory* and
/// rejoining the name covers the second case: falling back to the raw path
/// would compare a `/Users/...` path against the `/System/Volumes/...` one in
/// the event, match nothing, and leave the watch silently dead.
pub fn watch_target(path: &Path) -> PathBuf {
    if let Ok(resolved) = std::fs::canonicalize(path) {
        return resolved;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => match std::fs::canonicalize(parent) {
            Ok(directory) => directory.join(name),
            Err(_) => path.to_path_buf(),
        },
        _ => path.to_path_buf(),
    }
}

/// Watches one file for changes, surviving atomic saves.
pub struct FileWatcher {
    /// Dropping this stops the watch, so it must be held even though the field
    /// is never read.
    _watcher: RecommendedWatcher,
    events: Receiver<PathBuf>,
    debouncer: Debouncer,
}

impl FileWatcher {
    pub fn start(path: &Path) -> Result<Self, notify::Error> {
        // Watch the *directory*: editors save atomically by renaming a temp
        // file over the original, which invalidates a watch on the file itself.
        let directory = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        // Resolved so the comparison in the event handler below actually
        // matches; otherwise live reload is silently dead with no error and no
        // banner, since the watcher itself started fine.
        let target = watch_target(path);

        let (sender, events) = channel();
        let mut watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
            let Ok(event) = result else { return };
            // A directory watch is chatty; keep only our file.
            if event.paths.iter().any(|p| p == &target) {
                let _ = sender.send(target.clone());
            }
        })?;
        watcher.watch(&directory, RecursiveMode::NonRecursive)?;

        Ok(Self {
            _watcher: watcher,
            events,
            debouncer: Debouncer::new(QUIET_PERIOD),
        })
    }

    /// Drain pending events. Returns true when a reload is due. Called from a
    /// main-thread timer, so no AppKit object is ever touched off-thread.
    pub fn poll(&mut self, now: Instant) -> bool {
        loop {
            match self.events.try_recv() {
                Ok(_) => self.debouncer.record(now),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        self.debouncer.take_if_ready(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    const QUIET: Duration = Duration::from_millis(100);

    #[test]
    fn nothing_is_ready_without_an_event() {
        let mut d = Debouncer::new(QUIET);
        assert!(!d.take_if_ready(Instant::now()));
    }

    #[test]
    fn an_event_is_not_ready_until_the_quiet_period_elapses() {
        let start = Instant::now();
        let mut d = Debouncer::new(QUIET);
        d.record(start);
        assert!(!d.take_if_ready(start + Duration::from_millis(50)));
        assert!(d.take_if_ready(start + Duration::from_millis(150)));
    }

    #[test]
    fn a_burst_of_events_fires_once() {
        let start = Instant::now();
        let mut d = Debouncer::new(QUIET);
        for offset in [0, 10, 20, 30] {
            d.record(start + Duration::from_millis(offset));
        }
        assert!(!d.take_if_ready(start + Duration::from_millis(100)));
        assert!(d.take_if_ready(start + Duration::from_millis(200)));
        // And having fired, it stays quiet until something new happens.
        assert!(!d.take_if_ready(start + Duration::from_millis(400)));
    }

    /// A review file is watched from before it is first written, and the
    /// event will name a resolved path: without resolving the directory the
    /// comparison never matches and the watch is dead with nothing to show
    /// for it. The temp directory is itself under a symlink on macOS, so this
    /// is the real case and not a contrived one.
    #[test]
    fn a_file_that_does_not_exist_yet_resolves_through_its_symlinked_parent() {
        let base = std::env::temp_dir().join(format!("mdview-watch-{}", std::process::id()));
        let real = base.join("real");
        let link = base.join("link");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&real).expect("temp directory");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
        let resolved = std::fs::canonicalize(&real).expect("canonical temp directory");

        assert_eq!(
            watch_target(&link.join("not-written-yet.md")),
            resolved.join("not-written-yet.md"),
        );
        // And a file that does exist is unaffected by the new branch.
        std::fs::write(real.join("here.md"), "x").expect("write");
        assert_eq!(watch_target(&link.join("here.md")), resolved.join("here.md"));

        let _ = std::fs::remove_dir_all(&base);
    }

    /// The end-to-end version of the test above, and the only thing that
    /// proves `watch_target` agrees with what `notify` actually reports: a
    /// mismatch starts the watcher successfully, reports no error, and then
    /// never fires. This touches a real filesystem and a real clock, so it
    /// polls to a deadline rather than sleeping a fixed amount and hoping.
    #[test]
    fn a_review_written_for_the_first_time_wakes_its_watcher() {
        let base = std::env::temp_dir().join(format!("mdview-fire-{}", std::process::id()));
        let real = base.join("reviews");
        let link = base.join("by-link");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&real).expect("temp directory");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");

        // Watched through the symlink and before it exists: exactly how a
        // review is watched, from the moment its document opens.
        let review = link.join("deadbeef.md");
        let mut watcher = FileWatcher::start(&review).expect("watcher starts");

        // Written the way `store::save` writes: temp file, then rename over.
        let temp = real.join("deadbeef.md.tmp");
        std::fs::write(&temp, "# Review\n").expect("write");
        std::fs::rename(&temp, real.join("deadbeef.md")).expect("rename");

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut fired = false;
        while Instant::now() < deadline {
            if watcher.poll(Instant::now()) {
                fired = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = std::fs::remove_dir_all(&base);
        assert!(fired, "the watch never fired: the target path does not match the event");
    }

    #[test]
    fn a_new_event_after_firing_arms_it_again() {
        let start = Instant::now();
        let mut d = Debouncer::new(QUIET);
        d.record(start);
        assert!(d.take_if_ready(start + Duration::from_millis(200)));
        d.record(start + Duration::from_millis(300));
        assert!(d.take_if_ready(start + Duration::from_millis(500)));
    }
}
