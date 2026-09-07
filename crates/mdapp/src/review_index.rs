//! Pure workspace-wide aggregation of persisted review records.
//!
//! Filesystem enumeration lives in `store.rs`. This module accepts parsed
//! reviews and a workspace allowlist, so membership and incremental update
//! behavior remain testable without AppKit or a real Application Support tree.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use crate::review::{Comment, DocumentIdentity, Review, Status};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedReview {
    pub review_path: PathBuf,
    pub document_path: PathBuf,
    pub relative_path: PathBuf,
    pub schema_version: u32,
    pub comments: Vec<Comment>,
    pub damaged: bool,
}

#[derive(Debug, Clone)]
pub struct ReviewIndex {
    root: PathBuf,
    documents_by_relative: BTreeMap<PathBuf, PathBuf>,
    relative_by_document: HashMap<PathBuf, PathBuf>,
    reviews: BTreeMap<PathBuf, IndexedReview>,
}

impl ReviewIndex {
    pub fn new(root: PathBuf, documents: impl IntoIterator<Item = (PathBuf, PathBuf)>) -> Self {
        let documents_by_relative: BTreeMap<_, _> = documents.into_iter().collect();
        let relative_by_document = documents_by_relative
            .iter()
            .map(|(relative, absolute)| (absolute.clone(), relative.clone()))
            .collect();
        Self {
            root,
            documents_by_relative,
            relative_by_document,
            reviews: BTreeMap::new(),
        }
    }

    #[allow(dead_code)]
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn update(&mut self, review_path: PathBuf, review: Review) -> bool {
        let previous = self.reviews.remove(&review_path);
        let next = self.resolve(&review_path, review);
        if let Some(next) = next.clone() {
            self.reviews.insert(review_path, next);
        }
        previous != next
    }

    pub fn remove(&mut self, review_path: &Path) -> bool {
        self.reviews.remove(review_path).is_some()
    }

    pub fn entries(&self) -> impl Iterator<Item = &IndexedReview> {
        self.reviews.values()
    }

    pub fn contains_comment(&self, document_path: &Path, id: &str) -> bool {
        self.reviews.values().any(|review| {
            review.document_path == document_path
                && review.comments.iter().any(|comment| comment.id == id)
        })
    }

    #[allow(dead_code)]
    pub fn comment_count(&self, status: Option<Status>) -> usize {
        self.reviews
            .values()
            .flat_map(|review| &review.comments)
            .filter(|comment| status.map_or(true, |status| comment.status == status))
            .count()
    }

    fn resolve(&self, review_path: &Path, review: Review) -> Option<IndexedReview> {
        let (document_path, relative_path) = match review.document.as_ref()? {
            DocumentIdentity::WorkspaceRelative(relative) => {
                let relative = Path::new(relative);
                let document = self.documents_by_relative.get(relative)?;
                (document.clone(), relative.to_path_buf())
            }
            DocumentIdentity::LegacyAbsolute(absolute) => {
                let document = PathBuf::from(absolute);
                let relative = self.relative_by_document.get(&document)?;
                (document, relative.clone())
            }
            DocumentIdentity::Standalone(_) => return None,
        };
        Some(IndexedReview {
            review_path: review_path.to_path_buf(),
            document_path,
            relative_path,
            schema_version: review.schema_version,
            comments: review.comments,
            damaged: !review.damage.is_empty(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::{parse_review, Comment, DocumentIdentity, Review};

    fn index() -> ReviewIndex {
        ReviewIndex::new(
            PathBuf::from("/workspace"),
            [
                (PathBuf::from("a.md"), PathBuf::from("/workspace/a.md")),
                (
                    PathBuf::from("guide/b.md"),
                    PathBuf::from("/workspace/guide/b.md"),
                ),
            ],
        )
    }

    fn review(identity: DocumentIdentity, status: Status) -> Review {
        Review {
            schema_version: 2,
            document: Some(identity),
            comments: vec![Comment::new("1", 1, 0, "quote", "").with_status(status)],
            damage: Vec::new(),
        }
    }

    #[test]
    fn indexes_only_allowlisted_workspace_documents() {
        let mut index = index();
        assert!(index.update(
            PathBuf::from("one.md"),
            review(
                DocumentIdentity::WorkspaceRelative("guide/b.md".into()),
                Status::Open
            ),
        ));
        assert!(!index.update(
            PathBuf::from("outside.md"),
            review(
                DocumentIdentity::WorkspaceRelative("../outside.md".into()),
                Status::Open
            ),
        ));
        assert!(!index.update(
            PathBuf::from("excluded.md"),
            review(
                DocumentIdentity::WorkspaceRelative("hidden.md".into()),
                Status::Open
            ),
        ));
        assert_eq!(index.entries().count(), 1);
        assert_eq!(
            index.entries().next().unwrap().document_path,
            PathBuf::from("/workspace/guide/b.md")
        );
    }

    #[test]
    fn legacy_absolute_identity_must_match_the_workspace_allowlist() {
        let mut index = index();
        index.update(
            PathBuf::from("legacy.md"),
            review(
                DocumentIdentity::LegacyAbsolute("/workspace/a.md".into()),
                Status::Open,
            ),
        );
        index.update(
            PathBuf::from("other.md"),
            review(
                DocumentIdentity::LegacyAbsolute("/other/a.md".into()),
                Status::Open,
            ),
        );
        assert_eq!(index.entries().count(), 1);
    }

    #[test]
    fn an_incremental_update_replaces_only_its_own_review() {
        let mut index = index();
        let path = PathBuf::from("one.md");
        index.update(
            path.clone(),
            review(
                DocumentIdentity::WorkspaceRelative("a.md".into()),
                Status::Open,
            ),
        );
        index.update(
            PathBuf::from("two.md"),
            review(
                DocumentIdentity::WorkspaceRelative("guide/b.md".into()),
                Status::Stale,
            ),
        );
        assert!(index.update(
            path,
            review(
                DocumentIdentity::WorkspaceRelative("a.md".into()),
                Status::Resolved,
            ),
        ));
        assert_eq!(index.comment_count(Some(Status::Open)), 0);
        assert_eq!(index.comment_count(Some(Status::Resolved)), 1);
        assert_eq!(index.comment_count(Some(Status::Stale)), 1);
    }

    #[test]
    fn damaged_reviews_keep_the_records_that_were_read() {
        let mut review = parse_review(concat!(
            "~~~~ mdview-review 2 workspace-relative\na.md\n~~~~\n",
            "~~~~ mdview-quote 1 1 0 open\nvisible\n~~~~\n",
            "~~~~ mdview-future\nunknown\n~~~~\n",
        ));
        assert!(!review.damage.is_empty());
        let mut index = index();
        index.update(PathBuf::from("damaged.md"), review.clone());
        let entry = index.entries().next().unwrap();
        assert!(entry.damaged);
        assert_eq!(entry.comments, std::mem::take(&mut review.comments));
    }

    #[test]
    fn deleting_a_review_removes_only_that_entry() {
        let mut index = index();
        let path = PathBuf::from("one.md");
        index.update(
            path.clone(),
            review(
                DocumentIdentity::WorkspaceRelative("a.md".into()),
                Status::Open,
            ),
        );
        assert!(index.remove(&path));
        assert!(!index.remove(&path));
        assert_eq!(index.entries().count(), 0);
    }
}
