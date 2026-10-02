//! Receiving a file from a `croc` sender, by running the croc CLI.
//!
//! MDView does not bundle croc and does not speak its protocol. The sender is
//! already running croc on the other machine, so the receiver is that same
//! program with the code it printed: `CROC_SECRET=<code> croc --yes
//! --overwrite --out DIR` and the file lands in MDView's own inbox.
//!
//! Everything here is a plain function over paths and argv, so the parts worth
//! getting right -- where croc lives, what a failure means, which of the
//! received files to open -- are unit-tested without a croc on the machine.
//! The code is a user's input from the prompt. Current croc refuses to read it
//! from the command line -- the process list would show it -- so it travels in
//! the child's environment and never in argv or a shell string.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How long one receive may take before the child is killed. A code that is
/// valid but whose sender has walked away would otherwise wait forever, and
/// MDView would sit behind a "Receiving…" banner that never resolves.
pub const RECEIVE_TIMEOUT: Duration = Duration::from_secs(300);

/// How often the child is checked while it runs.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// A croc's stderr can carry progress and advice; only the tail of it is worth
/// a banner, and only so much of that.
const DETAIL_LIMIT: usize = 200;

/// What to tell someone whose Mac has no croc. It is the whole sentence, with
/// the way to fix it, because that is the only thing this reader can do next.
pub const INSTALL_HINT: &str = "croc is not installed. Install it with `brew install croc`, then try again.";

/// Everything that can go wrong between the code and an open document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrocError {
    /// No croc binary on this machine.
    NotInstalled,
    /// There is no `HOME`, so there is nowhere to put the file.
    NoHome,
    /// croc ran and failed. Carries the tail of its own message.
    Failed(String),
    /// croc ran but produced nothing to open.
    NoFile,
    /// croc could not be started, or the inbox could not be made.
    Io(String),
    /// croc was still running when the timeout expired.
    TimedOut,
}

/// `croc` in one of the places a GUI app can actually see, or on `PATH`.
///
/// A macOS app launched from Finder or the Dock inherits a minimal `PATH`
/// (`/usr/bin:/bin:/usr/sbin:/sbin`), so a bare `croc` would be invisible even
/// though the same command works in the reader's shell. The Homebrew prefixes
/// are therefore probed by name first, and `PATH` is consulted as well for a
/// binary somewhere else.
pub fn find_croc() -> Option<PathBuf> {
    for dir in ["/opt/homebrew/bin", "/usr/local/bin"] {
        let candidate = Path::new(dir).join("croc");
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    find_in_paths(std::env::split_paths(&std::env::var_os("PATH")?))
}

/// The croc in the first directory that has one. Split out so the search is
/// testable against a directory this test made rather than the machine's.
fn find_in_paths(dirs: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    dirs.into_iter().map(|dir| dir.join("croc")).find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// The argv of one receive, without the binary. The code is deliberately NOT
/// here: croc refuses to read a receive code from the command line, because the
/// process list would show it. It goes in the environment instead -- see
/// `run_receive`, and the `CROC_SECRET` name croc documents.
pub fn receive_args(out_dir: &Path) -> Vec<String> {
    vec![
        "--yes".to_string(),
        "--overwrite".to_string(),
        "--out".to_string(),
        out_dir.to_string_lossy().into_owned(),
    ]
}

/// Receive one file into `inbox_root` and return what arrived.
///
/// The receive goes in a directory of its own so two files with the same name
/// from two sends cannot collide, and so the document that opens has a stable
/// home -- live reload and reopen-after-relaunch both need a path that outlives
/// the moment. `NSTemporaryDirectory` is not that.
pub fn run_receive(binary: &Path, code: &str, inbox_root: &Path) -> Result<PathBuf, CrocError> {
    let out_dir = unique_receive_dir(inbox_root);
    std::fs::create_dir_all(&out_dir).map_err(|error| CrocError::Io(error.to_string()))?;

    let mut child = Command::new(binary)
        // The code is passed in croc's own environment variable, which is how
        // a receive is told its code without putting it in the process list.
        .env("CROC_SECRET", code)
        .args(receive_args(&out_dir))
        // Nothing reads croc's stdout: it is progress bars and the code's own
        // echo, which the reader already has. stderr is kept because it is
        // where a refusal ("could not connect", "wrong code") is written, and
        // that sentence is what the banner shows.
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => CrocError::NotInstalled,
            _ => CrocError::Io(error.to_string()),
        })?;

    // Drained on its own thread: a transfer that runs long enough can fill the
    // pipe, and a child blocked writing to a full pipe looks exactly like a
    // child that has hung.
    let mut stderr = child.stderr.take().expect("stderr was piped");
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(error) => return Err(CrocError::Io(error.to_string())),
        }
        if started.elapsed() >= RECEIVE_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(POLL_INTERVAL);
    };

    let stderr_text = reader.join().unwrap_or_default();
    let Some(status) = status else {
        return Err(CrocError::TimedOut);
    };
    if !status.success() {
        return Err(CrocError::Failed(failure_detail(&stderr_text)));
    }
    locate_received(&out_dir).ok_or(CrocError::NoFile)
}

/// `<inbox>/<seconds>-<pid>`, and a suffix if that already exists -- two
/// receives inside the same process and second still get their own directory.
fn unique_receive_dir(inbox_root: &Path) -> PathBuf {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_secs()).unwrap_or(0);
    let base = format!("{seconds}-{}", std::process::id());
    let mut candidate = inbox_root.join(&base);
    let mut suffix = 1;
    while candidate.exists() {
        candidate = inbox_root.join(format!("{base}-{suffix}"));
        suffix += 1;
    }
    candidate
}

/// The last thing croc said, which is the specific complaint and not the
/// progress before it. croc also appends an update notice after a failure, and
/// that would otherwise be the last line and the whole message.
fn failure_detail(stderr: &str) -> String {
    let line = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !is_update_notice(line))
        .next_back()
        .unwrap_or("");
    truncate(line, DETAIL_LIMIT)
}

/// croc's "A newer croc version is available." / "Run: croc update" footer,
/// which it prints even when the command itself failed.
fn is_update_notice(line: &str) -> bool {
    line.contains("newer croc version") || (line.starts_with("Run:") && line.contains("croc"))
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out: String = text.chars().take(limit.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// The received file to open. A send of one file leaves one file; a send of a
/// folder leaves several, and a Markdown one is what this app can show, so it
/// wins. Sorted, so the choice does not depend on the filesystem's order.
fn locate_received(dir: &Path) -> Option<PathBuf> {
    let mut files = Vec::new();
    collect_files(dir, &mut files);
    files.sort();
    files.iter().find(|path| is_markdown(path)).cloned().or_else(|| files.into_iter().next())
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut paths: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect_files(&path, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

fn is_markdown(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return false;
    };
    matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown" | "mdown" | "mkd")
}

/// The banner sentence for a failure. One place, so the in-app and the command
/// line spellings cannot drift apart.
pub fn message_for(error: &CrocError) -> String {
    match error {
        CrocError::NotInstalled => INSTALL_HINT.to_string(),
        CrocError::NoHome => "MDView has nowhere to put the file: there is no home directory.".to_string(),
        CrocError::NoFile => "croc finished, but no file arrived. Try the code again.".to_string(),
        CrocError::TimedOut => {
            "croc stopped answering. The code may have expired, or the sender stopped waiting."
                .to_string()
        }
        CrocError::Failed(detail) if detail.is_empty() => "croc could not receive the file.".to_string(),
        CrocError::Failed(detail) => format!("croc could not receive the file: {detail}"),
        CrocError::Io(detail) => format!("MDView could not run croc: {detail}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch(name: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let dir = std::env::temp_dir().join(format!(
            "mdview-croc-test-{}-{}-{name}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn executable(path: &Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, body).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// The code is kept out of argv entirely: current croc refuses to read a
    /// receive code from the command line, and the process list would show it
    /// if it did. The inbox is the only variable.
    #[test]
    fn the_receive_arguments_name_the_inbox_and_carry_no_code() {
        let args = receive_args(Path::new("/tmp/inbox"));
        assert_eq!(args, vec!["--yes", "--overwrite", "--out", "/tmp/inbox"]);
        // Whatever a reader pasted at the prompt, it is not here to be read
        // off `ps`.
        for arg in &args {
            assert!(!arg.contains("secret"), "a code reached argv: {arg}");
        }
    }

    #[test]
    fn a_croc_is_found_by_name_in_the_first_directory_that_has_an_executable_one() {
        let dir = scratch("paths");
        let empty = dir.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let good = dir.join("bin");
        std::fs::create_dir_all(&good).unwrap();
        executable(&good.join("croc"), "#!/bin/sh\nexit 0\n");

        assert_eq!(find_in_paths(vec![empty.clone(), good.clone()]), Some(good.join("croc")));
        assert_eq!(find_in_paths(vec![empty.clone()]), None);
        // A file that is there but not executable is not a croc to run.
        let plain = dir.join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::write(plain.join("croc"), "").unwrap();
        assert_eq!(find_in_paths(vec![plain]), None);
    }

    /// The whole receive, against a fake croc that writes the file croc would
    /// have written. What is guarded is the join: the directory made, the code
    /// that arrived through the environment, and the file found in it.
    #[test]
    fn a_finished_receive_returns_the_file_that_arrived() {
        let dir = scratch("receive");
        let binary = dir.join("croc");
        executable(
            &binary,
            "#!/bin/sh\n\
             out=.\n\
             while [ $# -gt 0 ]; do\n\
               if [ \"$1\" = \"--out\" ]; then out=\"$2\"; shift 2; else shift; fi\n\
             done\n\
             if [ \"$CROC_SECRET\" != \"alpha-bravo\" ]; then\n\
               echo 'the code did not arrive in the environment' >&2\n\
               exit 1\n\
             fi\n\
             printf '# received\\n' > \"$out/PLAN.md\"\n",
        );
        let inbox = dir.join("inbox");
        std::fs::create_dir_all(&inbox).unwrap();

        let received = run_receive(&binary, "alpha-bravo", &inbox).expect("the fake croc succeeds");
        assert_eq!(received.file_name().unwrap(), "PLAN.md");
        assert!(received.starts_with(&inbox));
        assert_eq!(std::fs::read_to_string(&received).unwrap(), "# received\n");
        assert!(received.parent().unwrap() != inbox, "each receive gets its own directory");
    }

    /// croc's own complaint is more useful than anything MDView could invent,
    /// so the last thing it said is what the banner carries -- and the update
    /// notice it appends after every failure is not part of the complaint.
    #[test]
    fn a_failed_receive_reports_what_croc_said_and_not_the_update_notice() {
        let dir = scratch("failed");
        let binary = dir.join("croc");
        executable(
            &binary,
            "#!/bin/sh\n\
             echo 'connecting…' >&2\n\
             echo 'could not connect to relay' >&2\n\
             echo 'A newer croc version is available: v9.9.9 (current: v9.9.8).' >&2\n\
             echo 'Run: croc update' >&2\n\
             exit 1\n",
        );
        let inbox = dir.join("inbox");
        std::fs::create_dir_all(&inbox).unwrap();

        let error = run_receive(&binary, "wrong-code", &inbox).unwrap_err();
        assert_eq!(error, CrocError::Failed("could not connect to relay".to_string()));
        assert!(message_for(&error).contains("could not connect to relay"));
        assert!(!message_for(&error).contains("croc update"));
    }

    #[test]
    fn the_update_notice_is_recognised_so_it_never_becomes_the_message() {
        assert!(is_update_notice("A newer croc version is available: v9 (current: v8)."));
        assert!(is_update_notice("Run: croc update"));
        assert!(!is_update_notice("could not connect to relay"));
        assert!(!is_update_notice("Enter receive code:"));
    }

    #[test]
    fn a_missing_croc_is_named_as_missing() {
        let dir = scratch("missing");
        let error = run_receive(&dir.join("no-such-croc"), "code", &dir.join("inbox")).unwrap_err();
        assert_eq!(error, CrocError::NotInstalled);
        assert!(message_for(&error).contains("brew install croc"));
    }

    /// A send of a folder arrives as several files. The one MDView can render
    /// is the one to open, and the choice does not depend on directory order.
    #[test]
    fn the_markdown_file_wins_when_a_send_brings_several() {
        let dir = scratch("locate");
        std::fs::write(dir.join("notes.txt"), "text").unwrap();
        std::fs::write(dir.join("zzz.md"), "# doc").unwrap();
        let nested = dir.join("docs");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("PLAN.md"), "# plan").unwrap();

        let found = locate_received(&dir).expect("a file arrived");
        assert_eq!(found.file_name().unwrap(), "PLAN.md");
        assert_eq!(found.parent().unwrap(), nested);

        // With nothing Markdown, something is still better than nothing.
        std::fs::remove_file(dir.join("zzz.md")).unwrap();
        std::fs::remove_file(nested.join("PLAN.md")).unwrap();
        assert_eq!(locate_received(&dir).unwrap(), dir.join("notes.txt"), "a non-Markdown file still opens");
        assert_eq!(locate_received(&dir.join("empty")), None);
    }

    #[test]
    fn two_receives_in_one_second_do_not_share_a_directory() {
        let root = scratch("unique");
        let first = unique_receive_dir(&root);
        std::fs::create_dir_all(&first).unwrap();
        let second = unique_receive_dir(&root);
        assert_ne!(first, second);
        assert!(second.starts_with(&root));
    }

    #[test]
    fn a_long_complaint_is_cut_to_something_a_banner_can_hold() {
        let long = "x".repeat(500);
        let cut = truncate(&long, 20);
        assert_eq!(cut.chars().count(), 20);
        assert!(cut.ends_with('…'));
        assert_eq!(truncate("short", 20), "short");
    }
}