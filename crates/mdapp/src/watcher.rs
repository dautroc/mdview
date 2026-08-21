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
        let target = path.to_path_buf();

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
