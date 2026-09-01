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

use crate::review::{parse_review, serialize_review, Comment, Review};

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

/// This document's review, empty when there is no file. A file that cannot be
/// read is indistinguishable from one that does not exist yet, on purpose: the
/// caller has nothing useful to do about either. A file that reads but does not
/// wholly parse is a different matter, and comes back in `Review::damage`.
pub fn load(canonical_doc: &str) -> Review {
    let Some(path) = review_path(canonical_doc) else {
        return Review::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => parse_review(&text),
        Err(_) => Review::default(),
    }
}

/// Write the review, replacing it atomically.
///
/// The temp file is created in the same directory so the rename stays on one
/// volume, and the rename is what makes a half-written file unobservable —
/// this file is shared with Claude, which may be reading it at any moment.
pub fn save(canonical_doc: &str, headings: &[String], comments: &[Comment]) -> std::io::Result<()> {
    let Some(path) = review_path(canonical_doc) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no home directory",
        ));
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serialize_review(canonical_doc, headings, comments);
    let temp = path.with_extension("md.tmp");
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, &path)
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

    /// The invariant the whole storage decision rests on: MDView writes only
    /// where MDView owns. A review beside the document would fail on a
    /// read-only volume and would show up in `git status` for every document
    /// anyone commented on.
    #[test]
    fn the_review_lives_under_application_support_not_beside_the_document() {
        let path = review_path("/Users/someone/project/notes.md").expect("HOME is set in tests");
        let text = path.to_string_lossy();
        assert!(text.contains("Library/Application Support/MDView/reviews"));
        assert!(!text.contains("/project/"), "must not be beside the document");
        assert!(text.ends_with(&crate::state::review_file_name("/Users/someone/project/notes.md")));
    }

    /// The watch has to be startable before anyone has commented, which is
    /// the state every document is in the first time it is opened.
    #[test]
    fn a_review_can_be_watched_before_it_has_ever_been_written() {
        let path = review_watch_path("/Users/someone/never/commented.md").expect("HOME is set");
        assert!(!path.exists(), "watching must not create the file itself");
        assert!(path.parent().expect("has a parent").is_dir(), "the directory must exist");
        assert_eq!(path, review_path("/Users/someone/never/commented.md").expect("HOME is set"));
    }

    #[test]
    fn a_document_with_no_review_yet_loads_as_no_comments() {
        let review = load("/nonexistent/never/opened.md");
        assert!(review.comments.is_empty());
        // And a file that is simply not there is not damage: refusing to write
        // here would make the first comment on any document impossible.
        assert!(review.damage.is_empty());
    }
}
