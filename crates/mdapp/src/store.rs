//! Thin filesystem shim over the review store. Deliberately logic-free, the
//! way `defaults.rs` is: the grammar lives in `review.rs` and the file name in
//! `state.rs`, both of which are unit-tested without touching a disk.
//!
//! Reviews live under Application Support rather than beside the document.
//! MDView is a viewer you point at anything — a file in `/Applications`, on a
//! mounted DMG, on a read-only share — and a sibling write fails in all three.
//! It would also drop an untracked file into `git status` for every document
//! anyone comments on, which is noise this app's own diff view would surface.
//! The rule stays statable: MDView writes only where MDView owns.

use std::path::{Path, PathBuf};

use crate::review::{parse_review, serialize_review_with_root, Comment, Review, CURRENT_VERSION};

/// The most comments one document can carry, the way `FIND_MATCH_LIMIT` caps
/// find. Re-anchoring walks the document once per comment on every render.
pub const COMMENT_LIMIT: usize = 200;

/// `~/Library/Application Support/MDView/reviews`, created on demand.
pub fn reviews_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/MDView/reviews"))
}

/// Where this document's review lives. `None` only when there is no `HOME`.
pub fn review_path(canonical_doc: &str) -> Option<PathBuf> {
    Some(reviews_dir()?.join(crate::state::review_file_name(canonical_doc)))
}

/// The review path, with its directory created so a watch can be started on
/// it before it exists.
///
/// A document's watch can lean on the document's own directory being there.
/// This directory is MDView's, and `save` does not make it until the first
/// comment is written -- so without this, watching a document nobody has
/// commented on yet would fail, and the watch would never be retried.
pub fn review_watch_path(canonical_doc: &str) -> Option<PathBuf> {
    let path = review_path(canonical_doc)?;
    std::fs::create_dir_all(path.parent()?).ok()?;
    Some(path)
}

pub fn review_watch_directory() -> Option<PathBuf> {
    let directory = reviews_dir()?;
    std::fs::create_dir_all(&directory).ok()?;
    Some(directory)
}

/// This document's review, empty when there is no file. A file that cannot be
/// read is indistinguishable from one that does not exist yet, on purpose: the
/// caller has nothing useful to do about either. A file that reads but does not
/// wholly parse is a different matter, and comes back in `Review::damage`.
pub fn load(canonical_doc: &str) -> Review {
    let Some(path) = review_path(canonical_doc) else {
        return Review::default();
    };
    load_path(&path).unwrap_or_default()
}

pub fn load_path(path: &Path) -> std::io::Result<Review> {
    std::fs::read_to_string(path).map(|text| parse_review(&text))
}

/// Enumerate persisted reviews once when a workspace becomes active. Results
/// are sorted so index construction and later portable exports are deterministic.
pub fn is_review_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    name.len() == 19
        && name.ends_with(".md")
        && name[..16].bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn enumerate() -> std::io::Result<Vec<(PathBuf, Review)>> {
    let Some(directory) = reviews_dir() else {
        return Ok(Vec::new());
    };
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_review_file(path))
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths
        .into_iter()
        .filter_map(|path| load_path(&path).ok().map(|review| (path, review)))
        .collect())
}

/// Write the review, replacing it atomically.
///
/// The temp file is created in the same directory so the rename stays on one
/// volume, and the rename is what makes a half-written file unobservable —
/// this file is shared with Claude, which may be reading it at any moment.
pub fn save(
    canonical_doc: &str,
    workspace_root: Option<&Path>,
    headings: &[String],
    comments: &[Comment],
) -> std::io::Result<()> {
    let Some(path) = review_path(canonical_doc) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no home directory",
        ));
    };
    save_at(
        &path,
        canonical_doc,
        workspace_root,
        headings,
        comments,
        None,
    )
}

pub fn save_if_unchanged(
    canonical_doc: &str,
    workspace_root: Option<&Path>,
    headings: &[String],
    comments: &[Comment],
    expected: &Review,
) -> std::io::Result<()> {
    let Some(path) = review_path(canonical_doc) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no home directory",
        ));
    };
    save_at(
        &path,
        canonical_doc,
        workspace_root,
        headings,
        comments,
        Some(expected),
    )
}

fn save_at(
    path: &Path,
    canonical_doc: &str,
    workspace_root: Option<&Path>,
    headings: &[String],
    comments: &[Comment],
    expected: Option<&Review>,
) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let (current, exists) = match std::fs::read_to_string(path) {
        Ok(existing) => (parse_review(&existing), true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Review::default(), false),
        Err(error) => return Err(error),
    };
    if !current.damage.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "the existing review contains unreadable records",
        ));
    }
    if expected.is_some_and(|expected| expected != &current) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            "the review changed while it was being merged",
        ));
    }
    if exists && current.schema_version < CURRENT_VERSION {
        std::fs::copy(path, path.with_extension("md.bak"))?;
    }
    let text = serialize_review_with_root(canonical_doc, workspace_root, headings, comments);
    write_atomic(path, text.as_bytes())
}

pub fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("export");
    for _ in 0..100 {
        let suffix = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(
            ".{name}.mdview-{}-{suffix}.tmp",
            std::process::id()
        ));
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        if let Err(error) = file
            .write_all(contents)
            .and_then(|_| file.sync_all())
            .and_then(|_| std::fs::rename(&temp, path))
        {
            let _ = std::fs::remove_file(&temp);
            return Err(error);
        }
        return Ok(());
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate a temporary export file",
    ))
}

/// Keep the previous contents as `.bak` before a destructive write. `x` has no
/// undo, so this is the one recovery there is.
pub fn backup(canonical_doc: &str) {
    let Some(path) = review_path(canonical_doc) else {
        return;
    };
    if path.exists() {
        let _ = std::fs::copy(&path, path.with_extension("md.bak"));
    }
}

/// The document's heading text, for the decorative labels in the review file.
pub fn headings_of(doc: &Path) -> Vec<String> {
    match std::fs::read_to_string(doc) {
        Ok(text) => mdcore::headings(&text),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temporary_review(name: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        std::env::temp_dir().join(format!(
            "mdview-review-test-{}-{}-{name}.md",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    /// The invariant the whole storage decision rests on: MDView writes only
    /// where MDView owns. A review beside the document would fail on a
    /// read-only volume and would show up in `git status` for every document
    /// anyone commented on.
    #[test]
    fn the_review_lives_under_application_support_not_beside_the_document() {
        let path = review_path("/Users/someone/project/notes.md").expect("HOME is set in tests");
        let text = path.to_string_lossy();
        assert!(text.contains("Library/Application Support/MDView/reviews"));
        assert!(
            !text.contains("/project/"),
            "must not be beside the document"
        );
        assert!(text.ends_with(&crate::state::review_file_name(
            "/Users/someone/project/notes.md"
        )));
    }

    /// The watch has to be startable before anyone has commented, which is
    /// the state every document is in the first time it is opened.
    #[test]
    fn a_review_can_be_watched_before_it_has_ever_been_written() {
        let path = review_watch_path("/Users/someone/never/commented.md").expect("HOME is set");
        assert!(!path.exists(), "watching must not create the file itself");
        assert!(
            path.parent().expect("has a parent").is_dir(),
            "the directory must exist"
        );
        assert_eq!(
            path,
            review_path("/Users/someone/never/commented.md").expect("HOME is set")
        );
    }

    #[test]
    fn a_document_with_no_review_yet_loads_as_no_comments() {
        let review = load("/nonexistent/never/opened.md");
        assert!(review.comments.is_empty());
        // And a file that is simply not there is not damage: refusing to write
        // here would make the first comment on any document impossible.
        assert!(review.damage.is_empty());
    }

    #[test]
    fn the_first_v1_write_creates_a_backup_and_migrates_to_v2() {
        let path = temporary_review("migration");
        let legacy = include_str!("../tests/fixtures/reviews/v1.md");
        std::fs::write(&path, legacy).unwrap();
        let comments = parse_review(legacy).comments;
        save_at(
            &path,
            "/Users/example/project/docs/plan.md",
            Some(Path::new("/Users/example/project")),
            &[],
            &comments,
            None,
        )
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(path.with_extension("md.bak")).unwrap(),
            legacy
        );
        let migrated = parse_review(&std::fs::read_to_string(&path).unwrap());
        assert_eq!(migrated.schema_version, CURRENT_VERSION);
        assert_eq!(migrated.comments, comments);
        let _ = std::fs::remove_file(path.with_extension("md.bak"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn damaged_or_non_utf8_reviews_are_never_rewritten() {
        let comment = Comment::new("1", 1, 0, "replacement", "");
        for (name, original) in [
            ("damaged", b"~~~~ mdview-future\nunknown\n~~~~\n".as_slice()),
            ("non-utf8", &[0xff, 0xfe][..]),
        ] {
            let path = temporary_review(name);
            std::fs::write(&path, original).unwrap();
            assert!(save_at(&path, "/tmp/plan.md", None, &[], &[comment.clone()], None,).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), original);
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn only_canonical_review_names_enter_the_workspace_index() {
        assert!(is_review_file(Path::new("0123456789abcdef.md")));
        for ignored in [
            "0123456789abcdef.md.bak",
            "0123456789abcdef.md.tmp",
            ".summary.mdview-1-2.tmp",
            "review.md",
        ] {
            assert!(!is_review_file(Path::new(ignored)), "accepted {ignored}");
        }
    }

    #[test]
    fn atomic_exports_never_overwrite_a_predictable_sibling_temp_file() {
        let path = temporary_review("atomic-output");
        let sibling = path.with_extension("tmp");
        std::fs::write(&sibling, "keep me").unwrap();
        write_atomic(&path, b"export").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"export");
        assert_eq!(std::fs::read_to_string(&sibling).unwrap(), "keep me");
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(sibling);
    }

    #[test]
    fn compare_before_write_preserves_a_clean_concurrent_edit() {
        let path = temporary_review("concurrent");
        let original = serialize_review_with_root(
            "/tmp/plan.md",
            None,
            &[],
            &[Comment::new("1", 1, 0, "original", "")],
        );
        std::fs::write(&path, &original).unwrap();
        let expected = parse_review(&original);
        let changed = serialize_review_with_root(
            "/tmp/plan.md",
            None,
            &[],
            &[Comment::new("1", 1, 0, "changed elsewhere", "")],
        );
        std::fs::write(&path, &changed).unwrap();
        let result = save_at(
            &path,
            "/tmp/plan.md",
            None,
            &[],
            &[Comment::new("2", 1, 0, "incoming", "")],
            Some(&expected),
        );
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), changed);
        let _ = std::fs::remove_file(path);
    }
}
