//! Deterministic, path-safe review session export and import logic.

use std::collections::BTreeMap;
use std::fmt;

use crate::review::{
    fence_for, parse_review, serialize_portable_review, Comment, DocumentIdentity, CURRENT_VERSION,
};

pub const SESSION_VERSION: u32 = 1;
pub const MAX_SESSION_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_SESSION_DOCUMENTS: usize = 10_000;
pub const MAX_COMMENT_FIELD_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDocument {
    pub relative_path: String,
    pub fingerprint: String,
    pub comments: Vec<Comment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReviewSession {
    pub documents: Vec<SessionDocument>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionError(pub String);

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::error::Error for SessionError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeResult {
    pub comments: Vec<Comment>,
    pub imported: usize,
    pub unchanged: usize,
    pub conflicts: Vec<String>,
}

impl ReviewSession {
    pub fn new(documents: impl IntoIterator<Item = SessionDocument>) -> Result<Self, SessionError> {
        let mut by_path = BTreeMap::new();
        for document in documents {
            validate_document(&document)?;
            if by_path
                .insert(document.relative_path.clone(), document)
                .is_some()
            {
                return Err(SessionError(
                    "the session contains a duplicate document path".into(),
                ));
            }
        }
        Ok(Self {
            documents: by_path.into_values().collect(),
        })
    }

    pub fn serialize(&self) -> String {
        let mut output = format!(
            "# MDView Review Session\n\n~~~~ mdview-session {SESSION_VERSION}\nPortable workspace review session\n~~~~\n"
        );
        for document in &self.documents {
            let review =
                serialize_portable_review(&document.relative_path, &[], &document.comments)
                    .expect("ReviewSession validates every relative path");
            let fence = fence_for(&review);
            output.push_str(&format!(
                "\n## {}\n\n{fence} mdview-session-document {}\n{}\n{fence}\n",
                document.relative_path, document.fingerprint, review
            ));
        }
        output
    }

    pub fn parse(text: &str) -> Result<Self, SessionError> {
        if text.len() as u64 > MAX_SESSION_BYTES {
            return Err(SessionError(
                "the review session exceeds the 64 MiB limit".into(),
            ));
        }
        let lines: Vec<&str> = text.lines().collect();
        let mut index = 0;
        let mut header = false;
        let mut documents = Vec::new();
        while index < lines.len() {
            let Some(open) = opener(lines[index]) else {
                index += 1;
                continue;
            };
            if !open.kind.starts_with("mdview-session") {
                index += 1;
                continue;
            }
            let at = index + 1;
            index += 1;
            let mut payload = Vec::new();
            while index < lines.len() && !closes(lines[index], open.width) {
                payload.push(lines[index]);
                index += 1;
            }
            if index >= lines.len() {
                return Err(SessionError(format!(
                    "unterminated session record at line {at}"
                )));
            }
            index += 1;
            let body = payload.join("\n");
            match open.kind.as_str() {
                "mdview-session" => {
                    let [version] = open.info.as_slice() else {
                        return Err(SessionError(format!("invalid session header at line {at}")));
                    };
                    let version = version.parse::<u32>().map_err(|_| {
                        SessionError(format!("invalid session version at line {at}"))
                    })?;
                    if version != SESSION_VERSION {
                        return Err(SessionError(format!(
                            "unsupported review session version {version}"
                        )));
                    }
                    if header {
                        return Err(SessionError("the session has more than one header".into()));
                    }
                    header = true;
                }
                "mdview-session-document" => {
                    let [fingerprint] = open.info.as_slice() else {
                        return Err(SessionError(format!(
                            "invalid session document at line {at}"
                        )));
                    };
                    let review = parse_review(&body);
                    if review.schema_version != CURRENT_VERSION || !review.damage.is_empty() {
                        return Err(SessionError(format!(
                            "unreadable embedded review at line {at}"
                        )));
                    }
                    let Some(DocumentIdentity::WorkspaceRelative(relative_path)) = review.document
                    else {
                        return Err(SessionError(format!(
                            "unsafe document identity at line {at}"
                        )));
                    };
                    documents.push(SessionDocument {
                        relative_path,
                        fingerprint: fingerprint.clone(),
                        comments: review.comments,
                    });
                    if documents.len() > MAX_SESSION_DOCUMENTS {
                        return Err(SessionError(
                            "the review session contains more than 10,000 documents".into(),
                        ));
                    }
                }
                _ => return Err(SessionError(format!("unknown session record at line {at}"))),
            }
        }
        if !header {
            return Err(SessionError("the review session header is missing".into()));
        }
        Self::new(documents)
    }
}

pub fn fingerprint(content: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in content {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

pub fn inbox_prompt(
    index: &crate::review_index::ReviewIndex,
    filter: &str,
) -> Result<String, SessionError> {
    if !matches_filter_name(filter) {
        return Err(SessionError("unknown Review Inbox filter".into()));
    }
    let mut files = index
        .entries()
        .filter_map(|review| {
            let ids = review
                .comments
                .iter()
                .filter(|comment| status_matches(comment.status, filter))
                .map(|comment| comment.id.as_str())
                .collect::<Vec<_>>();
            (!ids.is_empty()).then(|| {
                format!(
                    "{} (document {}, comment IDs {})",
                    review.review_path.display(),
                    review.document_path.display(),
                    ids.join(", ")
                )
            })
        })
        .collect::<Vec<_>>();
    files.sort();
    if files.is_empty() {
        return Err(SessionError(format!(
            "there are no {filter} review comments"
        )));
    }
    Ok(format!(
        "Please address the {filter} review comments in these files: {}. Each selected comment is a `mdview-quote` fenced block whose opening line ends with its status. After addressing a comment in its named document, change only that final status token to `resolved`; keep both its quote and optional `mdview-note` blocks so MDView retains review history, and leave unlisted comment IDs unchanged.",
        files.join("; ")
    ))
}

pub fn review_summary(index: &crate::review_index::ReviewIndex) -> Result<String, SessionError> {
    let mut entries = index
        .entries()
        .filter(|review| !review.comments.is_empty())
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    if entries.is_empty() {
        return Err(SessionError(
            "there are no workspace reviews to summarize".into(),
        ));
    }
    if entries
        .iter()
        .flat_map(|review| &review.comments)
        .any(|comment| comment.status != crate::review::Status::Resolved)
    {
        return Err(SessionError(
            "resolve every workspace review comment before exporting a summary".into(),
        ));
    }
    if let Some(review) = entries.iter().find(|review| review.damaged) {
        return Err(SessionError(format!(
            "repair the review for {} before exporting a summary",
            review.relative_path.display()
        )));
    }
    let document_count = entries.len();
    let comment_count: usize = entries.iter().map(|review| review.comments.len()).sum();
    let mut output = format!(
        "# MDView Review Summary\n\n- Documents reviewed: {document_count}\n- Comments resolved: {comment_count}\n"
    );
    for review in entries {
        output.push_str(&format!("\n## {}\n", review.relative_path.display()));
        for comment in &review.comments {
            output.push_str(&format!("\n### Comment {} — resolved\n", comment.id));
            let quote_fence = fence_for(&comment.quote);
            output.push_str(&format!(
                "\n{quote_fence} quote\n{}\n{quote_fence}\n",
                comment.quote
            ));
            if !comment.note.is_empty() {
                let note_fence = fence_for(&comment.note);
                output.push_str(&format!(
                    "\n{note_fence} note\n{}\n{note_fence}\n",
                    comment.note
                ));
            }
        }
    }
    Ok(output)
}

fn matches_filter_name(filter: &str) -> bool {
    matches!(filter, "unresolved" | "open" | "resolved" | "stale" | "all")
}

fn status_matches(status: crate::review::Status, filter: &str) -> bool {
    match filter {
        "unresolved" => status != crate::review::Status::Resolved,
        "open" => status == crate::review::Status::Open,
        "resolved" => status == crate::review::Status::Resolved,
        "stale" => status == crate::review::Status::Stale,
        "all" => true,
        _ => false,
    }
}

pub fn merge_comments(local: &[Comment], incoming: &[Comment]) -> MergeResult {
    let mut comments = local.to_vec();
    let mut imported = 0;
    let mut unchanged = 0;
    let mut conflicts = Vec::new();
    for comment in incoming {
        match comments.iter().find(|local| local.id == comment.id) {
            Some(local) if local == comment => unchanged += 1,
            Some(_) => conflicts.push(comment.id.clone()),
            None => {
                comments.push(comment.clone());
                imported += 1;
            }
        }
    }
    MergeResult {
        comments,
        imported,
        unchanged,
        conflicts,
    }
}

fn validate_document(document: &SessionDocument) -> Result<(), SessionError> {
    if serialize_portable_review(&document.relative_path, &[], &[]).is_none() {
        return Err(SessionError(format!(
            "unsafe document path: {}",
            document.relative_path
        )));
    }
    if document.fingerprint.len() != 16
        || !document
            .fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(SessionError(format!(
            "invalid content fingerprint for {}",
            document.relative_path
        )));
    }
    if document.comments.len() > crate::store::COMMENT_LIMIT {
        return Err(SessionError(format!(
            "{} contains more than {} comments",
            document.relative_path,
            crate::store::COMMENT_LIMIT
        )));
    }
    let mut ids = BTreeMap::new();
    for comment in &document.comments {
        if comment.quote.len() > MAX_COMMENT_FIELD_BYTES
            || comment.note.len() > MAX_COMMENT_FIELD_BYTES
        {
            return Err(SessionError(format!(
                "comment {} in {} exceeds the 1 MiB field limit",
                comment.id, document.relative_path
            )));
        }
        if ids.insert(&comment.id, ()).is_some() {
            return Err(SessionError(format!(
                "duplicate comment id {} in {}",
                comment.id, document.relative_path
            )));
        }
    }
    Ok(())
}

struct Opener {
    kind: String,
    info: Vec<String>,
    width: usize,
}

fn opener(line: &str) -> Option<Opener> {
    let trimmed = line.trim_start();
    let width = trimmed
        .chars()
        .take_while(|character| *character == '~')
        .count();
    if width < 3 {
        return None;
    }
    let mut info = trimmed[width..].split_whitespace().map(str::to_string);
    Some(Opener {
        kind: info.next()?,
        info: info.collect(),
        width,
    })
}

fn closes(line: &str, width: usize) -> bool {
    let trimmed = line.trim();
    trimmed.len() >= width && trimmed.chars().all(|character| character == '~')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::Status;

    fn document(path: &str, content: &str, comments: Vec<Comment>) -> SessionDocument {
        SessionDocument {
            relative_path: path.into(),
            fingerprint: fingerprint(content.as_bytes()),
            comments,
        }
    }

    #[test]
    fn export_is_deterministic_sorted_and_contains_no_absolute_paths() {
        let comments = vec![Comment::new(
            "1",
            2,
            0,
            "~~~~ mdview-session-document not really",
            "## awkward\n---",
        )
        .with_status(Status::Stale)];
        let session = ReviewSession::new([
            document("z.md", "z", comments.clone()),
            document("docs/a.md", "a", comments),
        ])
        .unwrap();
        let first = session.serialize();
        let second = session.serialize();
        assert_eq!(first, second);
        assert!(first.find("docs/a.md").unwrap() < first.find("z.md").unwrap());
        assert!(!first.contains("/Users/"));
        assert_eq!(ReviewSession::parse(&first).unwrap(), session);
    }

    #[test]
    fn import_rejects_traversal_absolute_paths_and_unsupported_versions() {
        let valid = ReviewSession::new([document("docs/a.md", "a", Vec::new())])
            .unwrap()
            .serialize();
        for invalid in [
            valid.replace("docs/a.md", "../a.md"),
            valid.replace("docs/a.md", "/tmp/a.md"),
            valid.replace("mdview-session 1", "mdview-session 99"),
            valid.replace("mdview-session-document", "mdview-session-future"),
        ] {
            assert!(
                ReviewSession::parse(&invalid).is_err(),
                "accepted {invalid}"
            );
        }
    }

    #[test]
    fn merge_adds_new_ids_keeps_identical_ids_and_reports_conflicts() {
        let existing = Comment::new("1", 1, 0, "same", "same");
        let local = vec![existing.clone(), Comment::new("2", 1, 0, "local", "newer")];
        let incoming = vec![
            existing,
            Comment::new("2", 1, 0, "remote", "older"),
            Comment::new("3", 1, 0, "new", "imported"),
        ];
        let merged = merge_comments(&local, &incoming);
        assert_eq!(merged.imported, 1);
        assert_eq!(merged.unchanged, 1);
        assert_eq!(merged.conflicts, vec!["2"]);
        assert_eq!(merged.comments[1], local[1]);
        assert_eq!(merged.comments.last().unwrap().id, "3");
    }

    fn review_index_with(statuses: &[Status]) -> crate::review_index::ReviewIndex {
        let mut index = crate::review_index::ReviewIndex::new(
            "/workspace".into(),
            [("docs/a.md".into(), "/workspace/docs/a.md".into())],
        );
        index.update(
            "/reviews/a.md".into(),
            crate::review::Review {
                schema_version: CURRENT_VERSION,
                document: Some(DocumentIdentity::WorkspaceRelative("docs/a.md".into())),
                comments: statuses
                    .iter()
                    .enumerate()
                    .map(|(at, status)| {
                        Comment::new(&(at + 1).to_string(), 1, 0, "quote", "note")
                            .with_status(*status)
                    })
                    .collect(),
                damage: Vec::new(),
            },
        );
        index
    }

    #[test]
    fn inbox_prompt_uses_only_the_filter_and_names_each_review_file() {
        let index = review_index_with(&[Status::Open, Status::Resolved, Status::Stale]);
        let prompt = inbox_prompt(&index, "unresolved").unwrap();
        assert!(prompt.contains("/reviews/a.md"));
        assert!(prompt.contains("/workspace/docs/a.md"));
        assert!(prompt.contains("comment IDs 1, 3"));
        assert!(!prompt.contains("IDs 1, 2"));
        assert!(!prompt.contains('\n'));
        assert!(prompt.contains("`resolved`"));
        assert!(inbox_prompt(&index, "unknown").is_err());
    }

    #[test]
    fn summary_requires_every_comment_resolved_and_leaks_no_absolute_paths() {
        assert!(review_summary(&review_index_with(&[Status::Open])).is_err());
        let summary = review_summary(&review_index_with(&[Status::Resolved])).unwrap();
        assert!(summary.contains("## docs/a.md"));
        assert!(summary.contains("Comments resolved: 1"));
        assert!(!summary.contains("/workspace"));
        assert!(!summary.contains("/reviews"));
    }

    #[test]
    fn import_limits_comment_counts_and_payload_sizes() {
        let too_many = (0..=crate::store::COMMENT_LIMIT)
            .map(|id| Comment::new(&id.to_string(), 1, 0, "q", ""))
            .collect();
        assert!(ReviewSession::new([document("a.md", "a", too_many)]).is_err());
        let oversized = "x".repeat(MAX_COMMENT_FIELD_BYTES + 1);
        assert!(ReviewSession::new([document(
            "a.md",
            "a",
            vec![Comment::new("1", 1, 0, &oversized, "")],
        )])
        .is_err());
    }

    #[test]
    fn duplicate_documents_and_comment_ids_are_rejected() {
        assert!(ReviewSession::new([
            document("a.md", "a", Vec::new()),
            document("a.md", "a", Vec::new()),
        ])
        .is_err());
        assert!(ReviewSession::new([document(
            "a.md",
            "a",
            vec![
                Comment::new("1", 1, 0, "a", ""),
                Comment::new("1", 2, 0, "b", ""),
            ],
        )])
        .is_err());
    }
}
