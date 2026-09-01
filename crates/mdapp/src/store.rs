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

use crate::review::{parse_review, serialize_review, Comment};

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

/// This document's comments, or an empty list. A file that cannot be read is
/// indistinguishable from one that does not exist yet, on purpose: the caller
/// has nothing useful to do about either.
pub fn load(canonical_doc: &str) -> Vec<Comment> {
    let Some(path) = review_path(canonical_doc) else {
        return Vec::new();
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => parse_review(&text),
        Err(_) => Vec::new(),
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
