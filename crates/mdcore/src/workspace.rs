//! Pure project-workspace discovery and search. This module performs no AppKit
//! work and keeps every filesystem and query decision directly testable.

use std::collections::BTreeMap;
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, Read};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use crate::render::markdown_options;

pub const MARKDOWN_EXTENSIONS: [&str; 3] = ["md", "markdown", "mdown"];
pub const DEFAULT_MAX_FILES: usize = 10_000;
pub const DEFAULT_MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
pub const DEFAULT_MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
pub const DEFAULT_MAX_RESULTS: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceLimits {
    pub max_files: usize,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
    pub max_results: usize,
}

impl Default for WorkspaceLimits {
    fn default() -> Self {
        Self {
            max_files: DEFAULT_MAX_FILES,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            max_total_bytes: DEFAULT_MAX_TOTAL_BYTES,
            max_results: DEFAULT_MAX_RESULTS,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("cannot open workspace {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("workspace root is not a directory: {0}")]
    NotDirectory(PathBuf),
    #[error("document is outside workspace {root}: {path}")]
    OutsideRoot { root: PathBuf, path: PathBuf },
    #[error("cannot read workspace document {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRoot {
    path: PathBuf,
}

impl WorkspaceRoot {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
        let requested = path.as_ref();
        let path = fs::canonicalize(requested).map_err(|source| WorkspaceError::Open {
            path: requested.to_path_buf(),
            source,
        })?;
        if !path.is_dir() {
            return Err(WorkspaceError::NotDirectory(path));
        }
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn contains(&self, path: impl AsRef<Path>) -> bool {
        fs::canonicalize(path)
            .ok()
            .is_some_and(|candidate| candidate.strip_prefix(&self.path).is_ok())
    }

    fn relative_path(&self, path: &Path) -> Result<PathBuf, WorkspaceError> {
        path.strip_prefix(&self.path)
            .map(Path::to_path_buf)
            .map_err(|_| WorkspaceError::OutsideRoot {
                root: self.path.clone(),
                path: path.to_path_buf(),
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFile {
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub size: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiscoverySummary {
    pub skipped_hidden_or_generated: usize,
    pub skipped_symlinks: usize,
    pub skipped_oversized: usize,
    pub skipped_unreadable: usize,
    pub truncated_by_file_limit: bool,
    pub truncated_by_byte_limit: bool,
}

impl DiscoverySummary {
    pub fn is_partial(self) -> bool {
        self.skipped_oversized > 0
            || self.skipped_unreadable > 0
            || self.truncated_by_file_limit
            || self.truncated_by_byte_limit
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSnapshot {
    pub root: WorkspaceRoot,
    pub files: Vec<WorkspaceFile>,
    pub generation: u64,
    pub indexed_bytes: u64,
    pub summary: DiscoverySummary,
}

impl WorkspaceSnapshot {
    pub fn discover(
        path: impl AsRef<Path>,
        generation: u64,
        limits: WorkspaceLimits,
    ) -> Result<Self, WorkspaceError> {
        let root = WorkspaceRoot::open(path)?;
        let mut files = Vec::new();
        let mut indexed_bytes = 0;
        let mut summary = DiscoverySummary::default();
        discover_directory(
            &root,
            root.path(),
            limits,
            &mut files,
            &mut indexed_bytes,
            &mut summary,
        );
        files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        Ok(Self {
            root,
            files,
            generation,
            indexed_bytes,
            summary,
        })
    }
}

fn discover_directory(
    root: &WorkspaceRoot,
    directory: &Path,
    limits: WorkspaceLimits,
    files: &mut Vec<WorkspaceFile>,
    indexed_bytes: &mut u64,
    summary: &mut DiscoverySummary,
) {
    if summary.truncated_by_file_limit || summary.truncated_by_byte_limit {
        return;
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(_) => {
            summary.skipped_unreadable += 1;
            return;
        }
    };
    let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => {
                summary.skipped_unreadable += 1;
                continue;
            }
        };
        if file_type.is_symlink() {
            summary.skipped_symlinks += 1;
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            if excluded_directory(&entry.file_name()) {
                summary.skipped_hidden_or_generated += 1;
            } else {
                discover_directory(root, &path, limits, files, indexed_bytes, summary);
            }
            continue;
        }
        if !file_type.is_file() || !is_markdown_path(&path) {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => {
                summary.skipped_unreadable += 1;
                continue;
            }
        };
        let size = metadata.len();
        if size > limits.max_file_bytes {
            summary.skipped_oversized += 1;
            continue;
        }
        if files.len() >= limits.max_files {
            summary.truncated_by_file_limit = true;
            return;
        }
        if indexed_bytes.saturating_add(size) > limits.max_total_bytes {
            summary.truncated_by_byte_limit = true;
            return;
        }
        let canonical = match fs::canonicalize(&path) {
            Ok(path) => path,
            Err(_) => {
                summary.skipped_unreadable += 1;
                continue;
            }
        };
        let relative_path = match root.relative_path(&canonical) {
            Ok(path) if canonical.to_str().is_some() && path.to_str().is_some() => path,
            Ok(_) => {
                summary.skipped_unreadable += 1;
                continue;
            }
            Err(_) => {
                summary.skipped_symlinks += 1;
                continue;
            }
        };
        *indexed_bytes += size;
        files.push(WorkspaceFile {
            path: canonical,
            relative_path,
            size,
        });
    }
}

fn excluded_directory(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return true;
    };
    if name.starts_with('.') {
        return true;
    }
    matches!(
        name.to_ascii_lowercase().as_str(),
        "build" | "dist" | "node_modules" | "target" | "vendor"
    )
}

fn excluded_relative_path(path: &Path) -> bool {
    path.parent().is_some_and(|parent| {
        parent
            .components()
            .any(|component| excluded_directory(component.as_os_str()))
    })
}

pub fn is_markdown_path(path: impl AsRef<Path>) -> bool {
    path.as_ref()
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            MARKDOWN_EXTENSIONS
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery(String);

impl SearchQuery {
    pub fn new(query: impl Into<String>) -> Option<Self> {
        let query = query.into();
        (!query.trim().is_empty()).then_some(Self(query))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub heading: Option<String>,
    pub snippet: String,
    pub match_range: std::ops::Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexedDocument {
    file: WorkspaceFile,
    source: String,
    folded: String,
    folded_to_source: Vec<usize>,
    headings: Vec<HeadingPosition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HeadingPosition {
    offset: usize,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceIndex {
    root: WorkspaceRoot,
    entries: BTreeMap<PathBuf, IndexedDocument>,
    generation: u64,
    max_files: usize,
    max_file_bytes: u64,
    max_total_bytes: u64,
    max_results: usize,
}

impl WorkspaceIndex {
    pub fn from_snapshot(
        snapshot: &WorkspaceSnapshot,
        limits: WorkspaceLimits,
    ) -> Result<Self, WorkspaceError> {
        let mut index = Self {
            root: snapshot.root.clone(),
            entries: BTreeMap::new(),
            generation: snapshot.generation,
            max_files: limits.max_files,
            max_file_bytes: limits.max_file_bytes,
            max_total_bytes: limits.max_total_bytes,
            max_results: limits.max_results,
        };
        for file in &snapshot.files {
            if index.entries.len() == index.max_files
                || index.total_bytes() >= index.max_total_bytes
            {
                break;
            }
            match index.insert_file(file.clone()) {
                Ok(()) => {
                    if index.total_bytes() > index.max_total_bytes {
                        index.entries.remove(&file.relative_path);
                        break;
                    }
                }
                // A file may disappear between discovery and indexing. The
                // watcher/reconciliation pass will converge the snapshot; one
                // transient file must not make search unavailable for all others.
                Err(WorkspaceError::Read { .. } | WorkspaceError::OutsideRoot { .. }) => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(index)
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn root(&self) -> &WorkspaceRoot {
        &self.root
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn search(&self, query: &SearchQuery) -> Vec<SearchHit> {
        let folded_query = fold(query.as_str()).0;
        let mut hits = Vec::new();
        for document in self.entries.values() {
            let Some(start) = document.folded.find(&folded_query) else {
                continue;
            };
            let end = start + folded_query.len();
            let source_start = document.folded_to_source[start];
            let source_end = document.folded_to_source[end];
            let match_range = source_start..source_end;
            hits.push(SearchHit {
                path: document.file.path.clone(),
                relative_path: document.file.relative_path.clone(),
                heading: nearest_heading(&document.headings, source_start),
                snippet: snippet(&document.source, match_range.clone()),
                match_range,
            });
            if hits.len() == self.max_results {
                break;
            }
        }
        hits
    }

    pub fn upsert(&mut self, path: impl AsRef<Path>) -> Result<(), WorkspaceError> {
        let requested = path.as_ref();
        let path = fs::canonicalize(requested).map_err(|source| WorkspaceError::Read {
            path: requested.to_path_buf(),
            source,
        })?;
        let relative_path = self.root.relative_path(&path)?;
        if !is_markdown_path(&path) || excluded_relative_path(&relative_path) {
            self.entries.remove(&relative_path);
            return Ok(());
        }
        let size = fs::metadata(&path)
            .map_err(|source| WorkspaceError::Read {
                path: path.clone(),
                source,
            })?
            .len();
        if size > self.max_file_bytes {
            self.entries.remove(&relative_path);
            return Ok(());
        }
        let key = relative_path.clone();
        self.insert_file(WorkspaceFile {
            path,
            relative_path,
            size,
        })?;
        if self.entries.len() > self.max_files || self.total_bytes() > self.max_total_bytes {
            self.entries.remove(&key);
        }
        Ok(())
    }

    pub fn remove(&mut self, path: impl AsRef<Path>) -> Result<bool, WorkspaceError> {
        let path = path.as_ref();
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.path().join(path)
        };
        let relative = absolute
            .strip_prefix(self.root.path())
            .map(Path::to_path_buf)
            .map_err(|_| WorkspaceError::OutsideRoot {
                root: self.root.path().to_path_buf(),
                path: absolute,
            })?;
        Ok(self.entries.remove(&relative).is_some())
    }

    pub fn files(&self) -> impl Iterator<Item = &WorkspaceFile> {
        self.entries.values().map(|entry| &entry.file)
    }

    /// Source text already admitted by the workspace safety limits. Consumers
    /// such as the link graph reuse this instead of performing a second,
    /// potentially unbounded filesystem read.
    pub fn documents(&self) -> impl Iterator<Item = (&WorkspaceFile, &str)> {
        self.entries
            .values()
            .map(|entry| (&entry.file, entry.source.as_str()))
    }

    fn total_bytes(&self) -> u64 {
        self.entries.values().map(|entry| entry.file.size).sum()
    }

    fn insert_file(&mut self, mut file: WorkspaceFile) -> Result<(), WorkspaceError> {
        let current = fs::canonicalize(&file.path).map_err(|source| WorkspaceError::Read {
            path: file.path.clone(),
            source,
        })?;
        if current != file.path
            || fs::symlink_metadata(&file.path)
                .map(|metadata| metadata.file_type().is_symlink())
                .unwrap_or(true)
        {
            return Err(WorkspaceError::OutsideRoot {
                root: self.root.path().to_path_buf(),
                path: current,
            });
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        options.custom_flags(libc::O_NOFOLLOW);
        let opened = options
            .open(&file.path)
            .map_err(|source| WorkspaceError::Read {
            path: file.path.clone(),
            source,
        })?;
        let mut bytes = Vec::new();
        opened
            .take(self.max_file_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|source| WorkspaceError::Read {
                path: file.path.clone(),
                source,
            })?;
        if bytes.len() as u64 > self.max_file_bytes {
            self.entries.remove(&file.relative_path);
            return Ok(());
        }
        file.size = bytes.len() as u64;
        let source = String::from_utf8_lossy(&bytes).into_owned();
        let (folded, folded_to_source) = fold(&source);
        let headings = heading_positions(&source);
        self.entries.insert(
            file.relative_path.clone(),
            IndexedDocument {
                file,
                source,
                folded,
                folded_to_source,
                headings,
            },
        );
        Ok(())
    }
}

fn fold(source: &str) -> (String, Vec<usize>) {
    let mut folded = String::new();
    let mut offsets = vec![0];
    for (source_start, character) in source.char_indices() {
        let source_end = source_start + character.len_utf8();
        let lower = character.to_lowercase().collect::<String>();
        let folded_start = folded.len();
        folded.push_str(&lower);
        offsets.resize(folded.len() + 1, source_start);
        offsets[folded_start] = source_start;
        offsets[folded.len()] = source_end;
    }
    if source.is_empty() {
        offsets[0] = 0;
    }
    (folded, offsets)
}

fn heading_positions(source: &str) -> Vec<HeadingPosition> {
    let mut headings = Vec::new();
    let mut active: Option<HeadingPosition> = None;
    for (event, range) in Parser::new_ext(source, markdown_options()).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { .. }) => {
                active = Some(HeadingPosition {
                    offset: range.start,
                    text: String::new(),
                });
            }
            Event::Text(text) | Event::Code(text) if active.is_some() => {
                if let Some(heading) = active.as_mut() {
                    heading.text.push_str(&text);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(heading) = active.take() {
                    headings.push(heading);
                }
            }
            _ => {}
        }
    }
    headings
}

fn nearest_heading(headings: &[HeadingPosition], offset: usize) -> Option<String> {
    headings
        .iter()
        .take_while(|heading| heading.offset <= offset)
        .last()
        .map(|heading| heading.text.clone())
        .filter(|heading| !heading.is_empty())
}

fn snippet(source: &str, range: std::ops::Range<usize>) -> String {
    const CONTEXT_CHARS: usize = 60;
    let before = source[..range.start]
        .char_indices()
        .rev()
        .nth(CONTEXT_CHARS)
        .map_or(0, |(offset, _)| offset);
    let after = source[range.end..]
        .char_indices()
        .nth(CONTEXT_CHARS)
        .map_or(source.len(), |(offset, _)| range.end + offset);
    let prefix = (before > 0).then_some("…").unwrap_or("");
    let suffix = (after < source.len()).then_some("…").unwrap_or("");
    let body = source[before..after]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    format!("{prefix}{body}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

    struct TempWorkspace(PathBuf);

    impl TempWorkspace {
        fn new() -> Self {
            let suffix = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("mdview-workspace-{}-{suffix}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn write(&self, relative: &str, source: impl AsRef<[u8]>) -> PathBuf {
            let path = self.0.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&path, source).unwrap();
            path
        }
    }

    impl Drop for TempWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn discovery_is_sorted_and_accepts_all_markdown_extensions() {
        let root = TempWorkspace::new();
        root.write("z.markdown", "z");
        root.write("guide/B.MDOWN", "b");
        root.write("guide/a.md", "a");
        root.write("guide/no.txt", "no");

        let snapshot = WorkspaceSnapshot::discover(&root.0, 7, WorkspaceLimits::default()).unwrap();
        let paths = snapshot
            .files
            .iter()
            .map(|file| file.relative_path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(paths, ["guide/B.MDOWN", "guide/a.md", "z.markdown"]);
        assert_eq!(snapshot.generation, 7);
    }

    #[test]
    fn discovery_skips_hidden_generated_and_symlinked_directories() {
        let root = TempWorkspace::new();
        root.write("kept.md", "yes");
        root.write(".git/hidden.md", "no");
        root.write("target/generated.md", "no");
        root.write("node_modules/pkg/readme.md", "no");
        let outside = TempWorkspace::new();
        outside.write("escape.md", "no");
        std::os::unix::fs::symlink(&outside.0, root.0.join("linked")).unwrap();

        let snapshot = WorkspaceSnapshot::discover(&root.0, 1, WorkspaceLimits::default()).unwrap();

        assert_eq!(snapshot.files.len(), 1);
        assert_eq!(snapshot.files[0].relative_path, Path::new("kept.md"));
        assert_eq!(snapshot.summary.skipped_hidden_or_generated, 3);
        assert_eq!(snapshot.summary.skipped_symlinks, 1);
    }

    #[test]
    fn discovery_reports_size_and_count_caps() {
        let root = TempWorkspace::new();
        root.write("a.md", "12345");
        root.write("b.md", "12345");
        root.write("huge.md", "1234567890");
        let limits = WorkspaceLimits {
            max_files: 1,
            max_file_bytes: 5,
            max_total_bytes: 100,
            max_results: 10,
        };

        let snapshot = WorkspaceSnapshot::discover(&root.0, 1, limits).unwrap();

        assert_eq!(snapshot.files.len(), 1);
        assert!(snapshot.summary.truncated_by_file_limit);
        assert!(snapshot.summary.is_partial());
    }

    #[test]
    fn byte_cap_is_reported_without_reading_unbounded_content() {
        let root = TempWorkspace::new();
        root.write("a.md", "12345");
        root.write("b.md", "12345");
        let limits = WorkspaceLimits {
            max_total_bytes: 7,
            ..WorkspaceLimits::default()
        };

        let snapshot = WorkspaceSnapshot::discover(&root.0, 1, limits).unwrap();

        assert_eq!(snapshot.indexed_bytes, 5);
        assert!(snapshot.summary.truncated_by_byte_limit);
    }

    #[test]
    fn search_is_case_insensitive_unicode_safe_and_reports_heading_context() {
        let root = TempWorkspace::new();
        root.write(
            "guide.md",
            "# Intro\n\nNothing here.\n\n## Café notes\n\nA λambda RETRY strategy works.\n",
        );
        let snapshot = WorkspaceSnapshot::discover(&root.0, 3, WorkspaceLimits::default()).unwrap();
        let index = WorkspaceIndex::from_snapshot(&snapshot, WorkspaceLimits::default()).unwrap();

        let hits = index.search(&SearchQuery::new("retry").unwrap());

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].relative_path, Path::new("guide.md"));
        assert_eq!(hits[0].heading.as_deref(), Some("Café notes"));
        assert!(hits[0].snippet.contains("λambda RETRY strategy"));
        assert_eq!(
            &fs::read_to_string(&hits[0].path).unwrap()[hits[0].match_range.clone()],
            "RETRY"
        );
    }

    #[test]
    fn search_result_order_and_cap_are_stable() {
        let root = TempWorkspace::new();
        root.write("z.md", "needle");
        root.write("a.md", "needle");
        let limits = WorkspaceLimits {
            max_results: 1,
            ..WorkspaceLimits::default()
        };
        let snapshot = WorkspaceSnapshot::discover(&root.0, 1, limits).unwrap();
        let index = WorkspaceIndex::from_snapshot(&snapshot, limits).unwrap();

        let hits = index.search(&SearchQuery::new("NEEDLE").unwrap());

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].relative_path, Path::new("a.md"));
    }

    #[test]
    fn incremental_add_change_and_delete_matches_a_full_rebuild() {
        let root = TempWorkspace::new();
        let first = root.write("first.md", "old word");
        let snapshot = WorkspaceSnapshot::discover(&root.0, 1, WorkspaceLimits::default()).unwrap();
        let mut incremental =
            WorkspaceIndex::from_snapshot(&snapshot, WorkspaceLimits::default()).unwrap();

        fs::write(&first, "new word").unwrap();
        incremental.upsert(&first).unwrap();
        let second = root.write("second.md", "new word too");
        incremental.upsert(&second).unwrap();
        fs::remove_file(&first).unwrap();
        incremental.remove("first.md").unwrap();

        let final_snapshot =
            WorkspaceSnapshot::discover(&root.0, 1, WorkspaceLimits::default()).unwrap();
        let rebuilt =
            WorkspaceIndex::from_snapshot(&final_snapshot, WorkspaceLimits::default()).unwrap();

        assert_eq!(incremental, rebuilt);
        assert_eq!(
            incremental
                .search(&SearchQuery::new("new word").unwrap())
                .len(),
            1
        );
    }

    #[test]
    fn representative_workspace_stays_bounded() {
        let root = TempWorkspace::new();
        for index in 0..500 {
            root.write(
                &format!("section-{}/note-{index}.md", index % 20),
                format!("# Note {index}\n\nshared workspace phrase {index}\n"),
            );
        }
        let limits = WorkspaceLimits {
            max_results: 25,
            ..WorkspaceLimits::default()
        };

        let snapshot = WorkspaceSnapshot::discover(&root.0, 9, limits).unwrap();
        let index = WorkspaceIndex::from_snapshot(&snapshot, limits).unwrap();
        let hits = index.search(&SearchQuery::new("shared workspace phrase").unwrap());

        assert_eq!(snapshot.files.len(), 500);
        assert_eq!(index.len(), 500);
        assert_eq!(hits.len(), 25);
        assert!(snapshot.indexed_bytes < limits.max_total_bytes);
    }

    #[test]
    fn actual_reads_cannot_exceed_the_aggregate_cap_after_discovery() {
        let root = TempWorkspace::new();
        let path = root.write("note.md", "1");
        let snapshot = WorkspaceSnapshot::discover(&root.0, 1, WorkspaceLimits::default()).unwrap();
        fs::write(path, "1234567890").unwrap();
        let limits = WorkspaceLimits {
            max_file_bytes: 20,
            max_total_bytes: 5,
            ..WorkspaceLimits::default()
        };

        let index = WorkspaceIndex::from_snapshot(&snapshot, limits).unwrap();

        assert!(index.is_empty());
    }

    #[test]
    fn replacement_symlinks_are_rejected_before_indexing() {
        let root = TempWorkspace::new();
        let path = root.write("note.md", "inside");
        let snapshot = WorkspaceSnapshot::discover(&root.0, 1, WorkspaceLimits::default()).unwrap();
        let outside = TempWorkspace::new();
        let outside_path = outside.write("outside.md", "outside");
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&outside_path, &path).unwrap();

        let index = WorkspaceIndex::from_snapshot(&snapshot, WorkspaceLimits::default()).unwrap();

        assert!(index.is_empty());
    }

    #[test]
    fn incremental_updates_keep_discovery_exclusions_and_aggregate_caps() {
        let root = TempWorkspace::new();
        let first = root.write("first.md", "12345");
        let snapshot = WorkspaceSnapshot::discover(&root.0, 1, WorkspaceLimits::default()).unwrap();
        let limits = WorkspaceLimits {
            max_files: 1,
            max_file_bytes: 10,
            max_total_bytes: 6,
            max_results: 10,
        };
        let mut index = WorkspaceIndex::from_snapshot(&snapshot, limits).unwrap();
        let second = root.write("second.md", "12");
        let hidden = root.write("target/generated.md", "x");

        index.upsert(&second).unwrap();
        index.upsert(&hidden).unwrap();

        assert_eq!(index.len(), 1);
        assert_eq!(index.files().next().unwrap().path, first);
    }

    #[test]
    fn canonical_containment_rejects_symlink_escapes() {
        let root = TempWorkspace::new();
        let outside = TempWorkspace::new();
        let outside_file = outside.write("outside.md", "outside");
        std::os::unix::fs::symlink(&outside_file, root.0.join("escape.md")).unwrap();
        let workspace = WorkspaceRoot::open(&root.0).unwrap();

        assert!(!workspace.contains(root.0.join("escape.md")));
        assert!(workspace.contains(root.write("inside.md", "inside")));
    }

    #[test]
    fn empty_queries_are_rejected() {
        assert!(SearchQuery::new("  \n").is_none());
    }
}
