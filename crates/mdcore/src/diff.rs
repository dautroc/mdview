//! Git-backed source diffs and their structured representation.

use std::path::{Path, PathBuf};

use crate::escape::escape_html;
use crate::highlight::Highlighter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub content: String,
    pub no_newline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub heading: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitRow {
    pub old: Option<DiffLine>,
    pub new: Option<DiffLine>,
}

#[derive(Debug, thiserror::Error)]
pub enum DiffError {
    #[error("invalid Git diff hunk header: {0}")]
    InvalidHunk(String),
    #[error("Git command failed: {0}")]
    Git(String),
    #[error("Git is not available")]
    GitUnavailable,
    #[error("file is not tracked by Git")]
    Untracked,
    #[error("repository has no HEAD commit")]
    NoHead,
    #[error("invalid Git revision: {0}")]
    InvalidRevision(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffAvailability {
    Available,
    Untracked,
    NoHead,
    GitUnavailable,
}

/// A Git revision safe to pass as one argument to the `git` process.
///
/// Git treats a leading `-` as an option, and `revision:path` uses `:` as a
/// separator. Rejecting both here keeps every caller on the argument-array path
/// and gives later history UI one validation rule rather than several subtly
/// different command-specific ones.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Revision(String);

impl Revision {
    pub fn parse(value: impl Into<String>) -> Result<Self, DiffError> {
        let value = value.into();
        if value.is_empty()
            || value.starts_with('-')
            || value.contains(':')
            || value.contains('\0')
            || value.contains('\n')
            || value.contains('\r')
        {
            return Err(DiffError::InvalidRevision(value));
        }
        Ok(Self(value))
    }

    pub fn head() -> Self {
        Self("HEAD".to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A discovered Git repository. Discovery is separate from file resolution so
/// later history and workspace queries can reuse one root without repeatedly
/// asking Git for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    pub root: PathBuf,
}

impl Repository {
    pub fn discover(path: &Path) -> Result<Self, DiffError> {
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let cwd = if path.is_dir() {
            path.as_path()
        } else {
            path.parent().unwrap_or_else(|| Path::new("."))
        };
        let root = git_output(cwd, &["rev-parse", "--show-toplevel"])
            .ok_or(DiffError::GitUnavailable)?;
        Ok(Self {
            root: PathBuf::from(root.trim()),
        })
    }

    pub fn tracked_path(&self, path: &Path) -> Result<TrackedPath, DiffError> {
        let absolute = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let relative = absolute
            .strip_prefix(&self.root)
            .map_err(|_| DiffError::Untracked)?
            .to_path_buf();
        let relative_arg = relative.to_string_lossy();
        if git_output(
            &self.root,
            &["ls-files", "--error-unmatch", "--", relative_arg.as_ref()],
        )
        .is_none()
        {
            return Err(DiffError::Untracked);
        }
        Ok(TrackedPath { absolute, relative })
    }

    pub fn has_revision(&self, revision: &Revision) -> bool {
        let commit = format!("{}^{{commit}}", revision.as_str());
        git_output(&self.root, &["rev-parse", "--verify", &commit]).is_some()
    }

    fn source_at(
        &self,
        path: &TrackedPath,
        revision: &Revision,
    ) -> Result<Option<String>, DiffError> {
        let object = format!("{}:{}", revision.as_str(), path.relative.to_string_lossy());
        if git_output(&self.root, &["cat-file", "-e", &object]).is_none() {
            return Ok(None);
        }
        git_required(&self.root, &["show", &object]).map(Some)
    }
}

/// One tracked file expressed both for the filesystem and relative to its Git
/// repository. Git commands always receive the relative form after `--`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedPath {
    pub absolute: PathBuf,
    pub relative: PathBuf,
}

/// One commit that touched a file, including the path the file had at that
/// point in history. The path is what lets a selection before a rename load the
/// right blob instead of treating the current name as a newly added file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub revision: Revision,
    pub short_revision: String,
    pub author: String,
    pub date: String,
    pub subject: String,
    pub path: PathBuf,
}

impl HistoryEntry {
    pub fn label(&self) -> String {
        format!("{} — {}", self.short_revision, self.subject)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLayout {
    Unified,
    Split,
    /// Not a layout of the source at all: the document as it renders, with the
    /// blocks that changed marked. Drawn by `crate::rdiff`.
    Rendered,
    /// The same, in two columns: what each block was, beside what it is.
    RenderedSplit,
}

impl DiffLayout {
    /// The value stored in defaults and stamped on the page.
    pub fn as_wire(self) -> &'static str {
        match self {
            DiffLayout::Unified => "unified",
            DiffLayout::Split => "split",
            DiffLayout::Rendered => "rendered",
            DiffLayout::RenderedSplit => "rendered-split",
        }
    }

    /// The layout a stored or posted wire value asks for. `None` for anything
    /// else, so that a stored value from a newer build and a message from a
    /// page that has gone wrong are both refused rather than guessed at.
    pub fn from_wire(wire: &str) -> Option<DiffLayout> {
        match wire {
            "unified" => Some(DiffLayout::Unified),
            "split" => Some(DiffLayout::Split),
            "rendered" => Some(DiffLayout::Rendered),
            "rendered-split" => Some(DiffLayout::RenderedSplit),
            _ => None,
        }
    }
}

/// The layouts that draw the source. `DiffLayout::Rendered` is deliberately
/// not one of them: it renders the document rather than its lines, and this
/// module is about lines and hunks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceLayout {
    Unified,
    Split,
}

/// Shown by every layout when the file matches HEAD.
pub const NO_CHANGES_HTML: &str = "<div class=\"mdview-diff-empty\">No changes against HEAD.</div>";

/// Parse Git's unified patch format. The parser intentionally accepts only
/// the line records needed by the renderer and ignores file headers and other
/// metadata before each hunk.
pub fn parse_patch(patch: &str) -> Result<Vec<DiffHunk>, DiffError> {
    let mut hunks = Vec::new();
    let mut current: Option<DiffHunk> = None;

    for raw in patch.lines() {
        if raw.starts_with("@@ ") {
            if let Some(hunk) = current.take() {
                hunks.push(hunk);
            }
            current = Some(parse_hunk_header(raw)?);
            continue;
        }

        let Some(hunk) = current.as_mut() else {
            continue;
        };
        if raw == "\\ No newline at end of file" {
            if let Some(last) = hunk.lines.last_mut() {
                last.no_newline = true;
            }
            continue;
        }

        let (kind, content) = match raw.as_bytes().first().copied() {
            Some(b' ') => (DiffLineKind::Context, &raw[1..]),
            Some(b'+') => (DiffLineKind::Added, &raw[1..]),
            Some(b'-') => (DiffLineKind::Removed, &raw[1..]),
            _ => continue,
        };

        let old_line = match kind {
            DiffLineKind::Added => None,
            DiffLineKind::Context | DiffLineKind::Removed => {
                let line = hunk.old_start
                    + hunk
                        .lines
                        .iter()
                        .filter(|l| matches!(l.kind, DiffLineKind::Context | DiffLineKind::Removed))
                        .count();
                Some(line)
            }
        };
        let new_line = match kind {
            DiffLineKind::Removed => None,
            DiffLineKind::Context | DiffLineKind::Added => {
                let line = hunk.new_start
                    + hunk
                        .lines
                        .iter()
                        .filter(|l| matches!(l.kind, DiffLineKind::Context | DiffLineKind::Added))
                        .count();
                Some(line)
            }
        };
        hunk.lines.push(DiffLine {
            kind,
            old_line,
            new_line,
            content: content.to_string(),
            no_newline: false,
        });
    }

    if let Some(hunk) = current {
        hunks.push(hunk);
    }
    Ok(hunks)
}

fn parse_hunk_header(raw: &str) -> Result<DiffHunk, DiffError> {
    let rest = raw
        .strip_prefix("@@ ")
        .and_then(|value| value.split_once(" @@"))
        .ok_or_else(|| DiffError::InvalidHunk(raw.to_string()))?;
    let ranges = rest.0.split_whitespace().collect::<Vec<_>>();
    if ranges.len() < 2 {
        return Err(DiffError::InvalidHunk(raw.to_string()));
    }
    let (old_start, old_count) = parse_range(ranges[0], '-')?;
    let (new_start, new_count) = parse_range(ranges[1], '+')?;
    Ok(DiffHunk {
        old_start,
        old_count,
        new_start,
        new_count,
        heading: rest.1.trim().to_string(),
        lines: Vec::new(),
    })
}

fn parse_range(value: &str, prefix: char) -> Result<(usize, usize), DiffError> {
    let value = value
        .strip_prefix(prefix)
        .ok_or_else(|| DiffError::InvalidHunk(value.to_string()))?;
    let (start, count) = value.split_once(',').map_or((value, "1"), |(s, c)| (s, c));
    let start = start
        .parse()
        .map_err(|_| DiffError::InvalidHunk(value.to_string()))?;
    let count = count
        .parse()
        .map_err(|_| DiffError::InvalidHunk(value.to_string()))?;
    Ok((start, count))
}

/// Pair a hunk's delete/add runs into rows suitable for a two-column view.
pub fn split_rows(hunk: &DiffHunk) -> Vec<SplitRow> {
    let mut rows = Vec::new();
    let mut index = 0;
    while index < hunk.lines.len() {
        if hunk.lines[index].kind == DiffLineKind::Context {
            rows.push(SplitRow {
                old: Some(hunk.lines[index].clone()),
                new: Some(hunk.lines[index].clone()),
            });
            index += 1;
            continue;
        }

        let delete_start = index;
        while index < hunk.lines.len() && hunk.lines[index].kind == DiffLineKind::Removed {
            index += 1;
        }
        let delete_end = index;
        let add_start = index;
        while index < hunk.lines.len() && hunk.lines[index].kind == DiffLineKind::Added {
            index += 1;
        }
        let add_end = index;
        let width = (delete_end - delete_start).max(add_end - add_start);
        for offset in 0..width {
            rows.push(SplitRow {
                old: (delete_start + offset < delete_end)
                    .then(|| hunk.lines[delete_start + offset].clone()),
                new: (add_start + offset < add_end)
                    .then(|| hunk.lines[add_start + offset].clone()),
            });
        }
    }
    rows
}

/// Git metadata needed to render a file's diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitDiff {
    pub path: PathBuf,
    pub repo_root: PathBuf,
    pub old_source: String,
    pub patch: Vec<DiffHunk>,
    pub base: Revision,
    pub base_label: String,
}

/// Resolve the repository and confirm the file can be compared with HEAD.
pub fn availability(path: &Path) -> DiffAvailability {
    let repository = match Repository::discover(path) {
        Ok(repository) => repository,
        Err(_) => return DiffAvailability::GitUnavailable,
    };
    if repository.tracked_path(path).is_err() {
        return DiffAvailability::Untracked;
    }
    if !repository.has_revision(&Revision::head()) {
        return DiffAvailability::NoHead;
    }
    DiffAvailability::Available
}

/// Load the current file's Git diff against HEAD.
///
/// Kept as the existing public shorthand while history views call
/// `load_diff_against` with the revision selected by the reader.
pub fn load_diff(path: &Path) -> Result<GitDiff, DiffError> {
    load_diff_against(path, &Revision::head())
}

/// Load the working-tree file's Git diff against an explicit base revision.
pub fn load_diff_against(path: &Path, base: &Revision) -> Result<GitDiff, DiffError> {
    load_diff_from(path, base, None, base.as_str())
}

/// Load the working-tree file against an entry returned by `history_for_path`.
/// Its historical path is significant when the document has been renamed.
pub fn load_diff_from_history(path: &Path, entry: &HistoryEntry) -> Result<GitDiff, DiffError> {
    load_diff_from(path, &entry.revision, Some(&entry.path), &entry.label())
}

fn load_diff_from(
    path: &Path,
    base: &Revision,
    historical_path: Option<&Path>,
    base_label: &str,
) -> Result<GitDiff, DiffError> {
    let repository = Repository::discover(path)?;
    let tracked = repository.tracked_path(path)?;
    if !repository.has_revision(base) {
        if base.as_str() == "HEAD" {
            return Err(DiffError::NoHead);
        }
        return Err(DiffError::Git(format!(
            "revision {:?} does not name a commit",
            base.as_str()
        )));
    }

    let current = tracked.relative.to_string_lossy().into_owned();
    let historical = historical_path
        .unwrap_or(&tracked.relative)
        .to_string_lossy()
        .into_owned();
    let mut args = vec![
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--text",
        "--find-renames",
        "--unified=3",
        base.as_str(),
        "--",
        &current,
    ];
    if historical != current {
        args.push(&historical);
    }
    let patch = git_required(&repository.root, &args)?;
    let historical_tracked = TrackedPath {
        absolute: tracked.absolute.clone(),
        relative: PathBuf::from(&historical),
    };
    let old_source = repository
        .source_at(&historical_tracked, base)?
        .unwrap_or_default();
    Ok(GitDiff {
        path: tracked.absolute,
        repo_root: repository.root,
        old_source,
        patch: parse_patch(&patch)?,
        base: base.clone(),
        base_label: base_label.to_string(),
    })
}

/// Commits that touched `path`, newest first, with the file's name at each
/// commit. Git performs the rename walk; this parser only turns its
/// record-delimited output into typed entries.
pub fn history_for_path(path: &Path, limit: usize) -> Result<Vec<HistoryEntry>, DiffError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let repository = Repository::discover(path)?;
    let tracked = repository.tracked_path(path)?;
    if !repository.has_revision(&Revision::head()) {
        return Err(DiffError::NoHead);
    }

    let count = format!("-{limit}");
    let relative = tracked.relative.to_string_lossy().into_owned();
    let output = git_required(
        &repository.root,
        &[
            "log",
            "--follow",
            &count,
            "--date=short",
            "--format=%x1e%H%x1f%h%x1f%an%x1f%ad%x1f%s",
            "--name-only",
            "--",
            &relative,
        ],
    )?;
    parse_history(&output)
}

fn parse_history(output: &str) -> Result<Vec<HistoryEntry>, DiffError> {
    let mut entries = Vec::new();
    for record in output.split('\x1e').filter(|record| !record.trim().is_empty()) {
        let mut lines = record.lines();
        let metadata = lines.next().unwrap_or_default();
        let fields: Vec<&str> = metadata.splitn(5, '\x1f').collect();
        let [revision, short_revision, author, date, subject] = fields.as_slice() else {
            return Err(DiffError::Git("Git returned malformed history metadata".to_string()));
        };
        let path = lines
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .last()
            .ok_or_else(|| DiffError::Git("Git returned a history entry without a path".to_string()))?;
        entries.push(HistoryEntry {
            revision: Revision::parse((*revision).to_string())?,
            short_revision: (*short_revision).to_string(),
            author: (*author).to_string(),
            date: (*date).to_string(),
            subject: (*subject).to_string(),
            path: PathBuf::from(path),
        });
    }
    Ok(entries)
}

/// Render parsed Git hunks as themed, escaped diff markup.
pub fn render_body(
    diff: &GitDiff,
    working_source: &str,
    highlighter: &Highlighter,
    layout: SourceLayout,
) -> String {
    if diff.patch.is_empty() {
        return empty_html(diff);
    }
    let old_lines = highlighter.render_markdown_lines(&diff.old_source);
    let new_lines = highlighter.render_markdown_lines(working_source);
    let mut html = format!(
        "<div class=\"mdview-diff mdview-diff-{}\" role=\"table\" aria-label=\"Git diff\">",
        match layout {
            SourceLayout::Unified => "unified",
            SourceLayout::Split => "split",
        }
    );
    for hunk in &diff.patch {
        html.push_str(&format!(
            "<section class=\"mdview-diff-hunk\" role=\"rowgroup\"><div class=\"mdview-diff-hunk-head\" role=\"row\">@@ -{},{} +{},{} @@ {}</div>",
            hunk.old_start,
            hunk.old_count,
            hunk.new_start,
            hunk.new_count,
            escape_html(&hunk.heading)
        ));
        match layout {
            SourceLayout::Unified => render_unified_rows(&mut html, hunk, &old_lines, &new_lines),
            SourceLayout::Split => render_split_rows(&mut html, hunk, &old_lines, &new_lines),
        }
        html.push_str("</section>");
    }
    html.push_str("</div>");
    comparison_html(diff, html)
}

/// Label non-HEAD comparisons without changing the established HEAD page.
pub fn comparison_html(diff: &GitDiff, body: String) -> String {
    if diff.base.as_str() == "HEAD" {
        return body;
    }
    format!(
        "<div class=\"mdview-diff-context\">Working tree compared with <strong>{}</strong></div>{body}",
        escape_html(&diff.base_label)
    )
}

pub fn empty_html(diff: &GitDiff) -> String {
    if diff.base.as_str() == "HEAD" {
        return NO_CHANGES_HTML.to_string();
    }
    format!(
        "<div class=\"mdview-diff-empty\">No changes against {}.</div>",
        escape_html(&diff.base_label)
    )
}

fn render_unified_rows(
    html: &mut String,
    hunk: &DiffHunk,
    old_lines: &[String],
    new_lines: &[String],
) {
    for line in &hunk.lines {
        let class = match line.kind {
            DiffLineKind::Context => "context",
            DiffLineKind::Added => "added",
            DiffLineKind::Removed => "removed",
        };
        let fragment = match line.kind {
            DiffLineKind::Removed => line_fragment(old_lines, line.old_line, &line.content),
            DiffLineKind::Context | DiffLineKind::Added => {
                line_fragment(new_lines, line.new_line, &line.content)
            }
        };
        let marker = match line.kind {
            DiffLineKind::Context => " ",
            DiffLineKind::Added => "+",
            DiffLineKind::Removed => "−",
        };
        let suffix = no_newline_suffix(line);
        html.push_str(&format!(
            "<div class=\"mdview-diff-row mdview-diff-row-{class}\" role=\"row\"><span class=\"mdview-diff-num\">{}</span><span class=\"mdview-diff-num\">{}</span><span class=\"mdview-diff-marker\">{marker}</span><code class=\"mdview-diff-code\">{fragment}{suffix}</code></div>",
            line.old_line.map_or(String::new(), |n| n.to_string()),
            line.new_line.map_or(String::new(), |n| n.to_string()),
        ));
    }
}

fn render_split_rows(
    html: &mut String,
    hunk: &DiffHunk,
    old_lines: &[String],
    new_lines: &[String],
) {
    for row in split_rows(hunk) {
        html.push_str("<div class=\"mdview-diff-split-row\" role=\"row\">");
        render_split_side(html, row.old.as_ref(), old_lines, "old");
        render_split_side(html, row.new.as_ref(), new_lines, "new");
        html.push_str("</div>");
    }
}

fn render_split_side(
    html: &mut String,
    line: Option<&DiffLine>,
    highlighted: &[String],
    side: &str,
) {
    let Some(line) = line else {
        html.push_str(&format!(
            "<div class=\"mdview-diff-side mdview-diff-side-{side} mdview-diff-placeholder\" role=\"cell\"><span class=\"mdview-diff-num\"></span><code class=\"mdview-diff-code\"></code></div>"
        ));
        return;
    };
    let class = match line.kind {
        DiffLineKind::Context => "context",
        DiffLineKind::Added => "added",
        DiffLineKind::Removed => "removed",
    };
    let number = if side == "old" { line.old_line } else { line.new_line };
    let fragment = line_fragment(highlighted, number, &line.content);
    let suffix = no_newline_suffix(line);
    html.push_str(&format!(
        "<div class=\"mdview-diff-side mdview-diff-side-{side} mdview-diff-row-{class}\" role=\"cell\"><span class=\"mdview-diff-num\">{}</span><code class=\"mdview-diff-code\">{fragment}{suffix}</code></div>",
        number.map_or(String::new(), |n| n.to_string()),
    ));
}

fn no_newline_suffix(line: &DiffLine) -> &'static str {
    if line.no_newline {
        "<span class=\"mdview-diff-no-newline\" title=\"No newline at end of file\">↵</span>"
    } else {
        ""
    }
}

fn line_fragment(lines: &[String], number: Option<usize>, fallback: &str) -> String {
    number
        .and_then(|n| n.checked_sub(1))
        .and_then(|index| lines.get(index))
        .cloned()
        .unwrap_or_else(|| escape_html(fallback))
}

fn git_output(cwd: &Path, args: &[&str]) -> Option<String> {
    std::process::Command::new("git")
        .current_dir(cwd)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_PAGER", "cat")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn git_required(cwd: &Path, args: &[&str]) -> Result<String, DiffError> {
    let output = std::process::Command::new("git")
        .current_dir(cwd)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_PAGER", "cat")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|_| DiffError::GitUnavailable)?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(DiffError::Git(if stderr.is_empty() {
            format!("exit status {}", output.status)
        } else {
            stderr
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// A repository of this test's own. The three tests that want one run as
    /// parallel threads of ONE process, so the pid does not separate them and
    /// the clock cannot be relied on to: two threads inside the same tick got
    /// the same name, `create_dir_all` was happy to hand both of them the same
    /// directory, and the second `git init` died on a file the first had
    /// already written. The counter is what actually makes the name unique;
    /// the pid and the clock only keep one run's directories clear of the
    /// last's. `create_dir` rather than `create_dir_all` so that a collision
    /// is a failure here, where the name is decided, and not inside git.
    fn temp_repo() -> PathBuf {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "mdview-diff-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .current_dir(&dir)
                .args(args)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .status()
                .unwrap();
            assert!(status.success(), "git command failed: {args:?}");
        };
        run(&["init", "-q"]);
        std::fs::write(dir.join("note.md"), "# Before\n").unwrap();
        run(&["add", "--", "note.md"]);
        run(&[
            "-c",
            "user.name=MDView Test",
            "-c",
            "user.email=mdview@example.test",
            "commit",
            "-qm",
            "initial",
        ]);
        dir
    }

    #[test]
    fn parses_hunks_and_assigns_old_and_new_line_numbers() {
        let patch = "diff --git a/README.md b/README.md\n@@ -2,3 +2,4 @@ Heading\n keep\n-old\n+new\n+extra\n tail\n";
        let hunks = parse_patch(patch).unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].old_start, 2);
        assert_eq!(hunks[0].old_count, 3);
        assert_eq!(hunks[0].new_start, 2);
        assert_eq!(hunks[0].new_count, 4);
        assert_eq!(hunks[0].heading, "Heading");
        assert_eq!(hunks[0].lines[1].old_line, Some(3));
        assert_eq!(hunks[0].lines[1].new_line, None);
        assert_eq!(hunks[0].lines[2].old_line, None);
        assert_eq!(hunks[0].lines[2].new_line, Some(3));
    }

    #[test]
    fn omitted_range_counts_default_to_one_and_marker_marks_last_line() {
        let hunks = parse_patch("@@ -4 +4 @@\n-old\n+new\n\\ No newline at end of file\n").unwrap();
        assert_eq!(hunks[0].old_count, 1);
        assert_eq!(hunks[0].new_count, 1);
        assert!(hunks[0].lines[1].no_newline);
    }

    #[test]
    fn split_rows_pair_changes_and_pad_the_shorter_side() {
        let hunks = parse_patch("@@ -1,4 +1,3 @@\n same\n-old one\n-old two\n+new\n tail\n").unwrap();
        let rows = split_rows(&hunks[0]);
        assert_eq!(rows.len(), 4);
        assert!(rows[1].old.is_some() && rows[1].new.is_some());
        assert!(rows[2].old.is_some() && rows[2].new.is_none());
    }

    #[test]
    fn malformed_hunk_header_is_rejected() {
        assert!(matches!(parse_patch("@@ nope @@\n"), Err(DiffError::InvalidHunk(_))));
    }

    #[test]
    fn revision_rejects_values_that_can_change_git_argument_meaning() {
        for invalid in ["", "--help", "HEAD:note.md", "HEAD\nmain", "HEAD\0main"] {
            assert!(
                matches!(Revision::parse(invalid), Err(DiffError::InvalidRevision(_))),
                "accepted unsafe revision {invalid:?}"
            );
        }
        for valid in ["HEAD", "HEAD~2", "main", "refs/tags/v1.0.0", "abc123"] {
            assert_eq!(Revision::parse(valid).unwrap().as_str(), valid);
        }
    }

    #[test]
    fn loads_head_diff_for_a_tracked_file_and_keeps_paths_out_of_the_shell() {
        if Command::new("git").output().is_err() {
            return;
        }
        let dir = temp_repo();
        let path = dir.join("note with spaces.md");
        std::fs::rename(dir.join("note.md"), &path).unwrap();
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .current_dir(&dir)
                .args(args)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .status()
                .unwrap();
            assert!(status.success(), "git command failed: {args:?}");
        };
        run(&["add", "-A"]);
        run(&[
            "-c",
            "user.name=MDView Test",
            "-c",
            "user.email=mdview@example.test",
            "commit",
            "-qm",
            "rename",
        ]);
        std::fs::write(&path, "# After\n\n<script>alert(1)</script>\n").unwrap();

        assert_eq!(availability(&path), DiffAvailability::Available);
        let diff = load_diff(&path).unwrap();
        assert_eq!(diff.old_source, "# Before\n");
        assert_eq!(diff.patch.len(), 1);
        assert!(diff.patch[0]
            .lines
            .iter()
            .any(|line| line.kind == DiffLineKind::Added && line.content.contains("script")));
    }

    #[test]
    fn an_explicit_revision_can_be_compared_with_the_working_tree() {
        if Command::new("git").output().is_err() {
            return;
        }
        let dir = temp_repo();
        let path = dir.join("note.md");
        std::fs::write(&path, "# Middle\n").unwrap();
        let status = Command::new("git")
            .current_dir(&dir)
            .args([
                "-c",
                "user.name=MDView Test",
                "-c",
                "user.email=mdview@example.test",
                "commit",
                "-am",
                "middle",
                "-q",
            ])
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::write(&path, "# Working tree\n").unwrap();

        let diff = load_diff_against(&path, &Revision::parse("HEAD~1").unwrap()).unwrap();
        assert_eq!(diff.old_source, "# Before\n");
        let text = diff
            .patch
            .iter()
            .flat_map(|hunk| hunk.lines.iter())
            .map(|line| line.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("# Before"));
        assert!(text.contains("# Working tree"));
        assert!(!text.contains("# Middle"));
    }

    #[test]
    fn history_keeps_the_path_each_commit_used_across_a_rename() {
        if Command::new("git").output().is_err() {
            return;
        }
        let dir = temp_repo();
        let old_path = dir.join("note.md");
        std::fs::write(&old_path, "# Middle\n").unwrap();
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .current_dir(&dir)
                .args(args)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .status()
                .unwrap();
            assert!(status.success(), "git command failed: {args:?}");
        };
        run(&[
            "-c",
            "user.name=MDView Test",
            "-c",
            "user.email=mdview@example.test",
            "commit",
            "-am",
            "middle",
            "-q",
        ]);
        let new_path = dir.join("guide.md");
        std::fs::rename(&old_path, &new_path).unwrap();
        run(&["add", "-A"]);
        run(&[
            "-c",
            "user.name=MDView Test",
            "-c",
            "user.email=mdview@example.test",
            "commit",
            "-m",
            "rename the guide",
            "-q",
        ]);
        std::fs::write(&new_path, "# Working tree\n").unwrap();

        let history = history_for_path(&new_path, 20).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].path, PathBuf::from("guide.md"));
        assert_eq!(history.last().unwrap().path, PathBuf::from("note.md"));
        assert_eq!(history[0].subject, "rename the guide");

        let diff = load_diff_from_history(&new_path, history.last().unwrap()).unwrap();
        assert_eq!(diff.old_source, "# Before\n");
        assert_eq!(diff.base_label, history.last().unwrap().label());
        assert!(!diff.patch.is_empty());
    }

    #[test]
    fn history_respects_its_result_limit() {
        if Command::new("git").output().is_err() {
            return;
        }
        let dir = temp_repo();
        let history = history_for_path(&dir.join("note.md"), 1).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history_for_path(&dir.join("note.md"), 0).unwrap(), Vec::new());
    }

    #[test]
    fn diff_is_scoped_to_the_open_file() {
        if Command::new("git").output().is_err() {
            return;
        }
        let dir = temp_repo();
        let current = dir.join("note.md");
        let other = dir.join("other.md");
        std::fs::write(&current, "# Current changed\n").unwrap();
        std::fs::write(&other, "# Other changed\n").unwrap();
        let diff = load_diff(&current).unwrap();
        let text = diff.patch.iter().flat_map(|h| h.lines.iter()).map(|l| l.content.as_str()).collect::<Vec<_>>().join("\n");
        assert!(text.contains("Current changed"));
        assert!(!text.contains("Other changed"));
    }

    #[test]
    fn reports_untracked_and_no_head_files_as_unavailable() {
        if Command::new("git").output().is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!(
            "mdview-diff-empty-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let status = Command::new("git").current_dir(&dir).args(["init", "-q"]).status().unwrap();
        assert!(status.success());
        let untracked = dir.join("untracked.md");
        std::fs::write(&untracked, "draft\n").unwrap();
        assert_eq!(availability(&untracked), DiffAvailability::Untracked);

        let tracked = dir.join("tracked.md");
        std::fs::write(&tracked, "draft\n").unwrap();
        let status = Command::new("git")
            .current_dir(&dir)
            .args(["add", "--", "tracked.md"])
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(availability(&tracked), DiffAvailability::NoHead);
    }

    /// The one test that walks the whole path a keypress takes: a tracked
    /// file, a layout, and a page. `rdiff` is unit-tested without git, and the
    /// page without a document, so nothing else checks that the three are
    /// actually joined up.
    #[test]
    fn the_rendered_layout_renders_the_document_and_says_so_on_the_page() {
        if Command::new("git").output().is_err() {
            return;
        }
        let dir = temp_repo();
        let path = dir.join("note.md");
        std::fs::write(&path, "# After

A new paragraph.
").unwrap();
        let highlighter = Highlighter::new();
        let doc = crate::render_diff_document_with(
            &path,
            &highlighter,
            crate::Theme::System,
            DiffLayout::Rendered,
        )
        .unwrap();
        assert!(doc.html.contains("data-diff-layout=\"rendered\""), "the page is not stamped");
        assert!(
            doc.html.contains("<div class=\"mdview-rdiff mdview-rdiff-single\">"),
            "the body is not the document"
        );
        assert!(
            doc.html.contains("<h1 data-mdview-change=\"changed\">After</h1>"),
            "the changed heading is not marked"
        );
        assert!(
            doc.html.contains("<template><h1>Before</h1></template>"),
            "the version that was there is gone"
        );
        // The same file in two columns: the same pairing, laid out in rows.
        let side_by_side = crate::render_diff_document_with(
            &path,
            &highlighter,
            crate::Theme::System,
            DiffLayout::RenderedSplit,
        )
        .unwrap();
        assert!(side_by_side.html.contains("data-diff-layout=\"rendered-split\""));
        assert!(
            side_by_side.html.contains("<div class=\"mdview-rdiff-side mdview-rdiff-old\"><h1>Before</h1></div>"),
            "the older document is not in the left column"
        );

        // And in a source layout it is still rows of source.
        let unified = crate::render_diff_document_with(
            &path,
            &highlighter,
            crate::Theme::System,
            DiffLayout::Unified,
        )
        .unwrap();
        assert!(unified.html.contains("mdview-diff-unified"), "the layouts have merged");
    }

    #[test]
    fn head_diff_includes_staged_and_unstaged_working_tree_changes() {
        if Command::new("git").output().is_err() {
            return;
        }
        let dir = temp_repo();
        let path = dir.join("note.md");
        std::fs::write(&path, "# Staged\n").unwrap();
        let status = Command::new("git")
            .current_dir(&dir)
            .args(["add", "--", "note.md"])
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::write(&path, "# Unstaged\n").unwrap();
        let diff = load_diff(&path).unwrap();
        assert!(diff.patch[0]
            .lines
            .iter()
            .any(|line| line.kind == DiffLineKind::Added && line.content == "# Unstaged"));
    }

    #[test]
    fn renders_unified_and_split_markup_with_escaped_source() {
        let diff = GitDiff {
            path: PathBuf::from("note.md"),
            repo_root: PathBuf::from("."),
            old_source: "# Before\n".to_string(),
            patch: parse_patch("@@ -1 +1,2 @@\n-# Before\n+# After\n+<script>alert(1)</script>\n").unwrap(),
            base: Revision::head(),
            base_label: "HEAD".to_string(),
        };
        let highlighter = Highlighter::new();
        let unified = render_body(&diff, "# After\n<script>alert(1)</script>\n", &highlighter, SourceLayout::Unified);
        let split = render_body(&diff, "# After\n<script>alert(1)</script>\n", &highlighter, SourceLayout::Split);
        assert!(unified.contains("mdview-diff-unified"));
        assert!(split.contains("mdview-diff-split"));
        assert!(unified.contains("&lt;") && unified.contains("&gt;"));
        assert!(!unified.contains("<script>alert"));
        assert!(split.contains("mdview-diff-placeholder") || split.contains("mdview-diff-side"));
    }
}
