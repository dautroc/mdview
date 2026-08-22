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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffAvailability {
    Available,
    Untracked,
    NoHead,
    GitUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLayout {
    Unified,
    Split,
}

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
}

/// Resolve the repository and confirm the file can be compared with HEAD.
pub fn availability(path: &Path) -> DiffAvailability {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let Some(root) = git_output(
        path.parent().unwrap_or_else(|| Path::new(".")),
        &["rev-parse", "--show-toplevel"],
    ) else {
        return DiffAvailability::GitUnavailable;
    };
    let root = PathBuf::from(root.trim());
    let Ok(relative) = path.strip_prefix(&root) else {
        return DiffAvailability::Untracked;
    };
    let relative = relative.to_string_lossy().into_owned();
    if git_output(&root, &["ls-files", "--error-unmatch", "--", &relative]).is_none() {
        return DiffAvailability::Untracked;
    }
    if git_output(&root, &["rev-parse", "--verify", "HEAD^{commit}"]).is_none() {
        return DiffAvailability::NoHead;
    }
    DiffAvailability::Available
}

/// Load the current file's Git diff against HEAD.
pub fn load_diff(path: &Path) -> Result<GitDiff, DiffError> {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let root = match git_output(
        path.parent().unwrap_or_else(|| Path::new(".")),
        &["rev-parse", "--show-toplevel"],
    ) {
        Some(root) => PathBuf::from(root.trim()),
        None => return Err(DiffError::GitUnavailable),
    };
    let relative = path
        .strip_prefix(&root)
        .map_err(|_| DiffError::Untracked)?
        .to_string_lossy()
        .into_owned();
    if git_output(&root, &["ls-files", "--error-unmatch", "--", &relative]).is_none() {
        return Err(DiffError::Untracked);
    }
    if git_output(&root, &["rev-parse", "--verify", "HEAD^{commit}"]).is_none() {
        return Err(DiffError::NoHead);
    }

    let patch = git_required(
        &root,
        &[
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--text",
            "--no-renames",
            "--unified=3",
            "HEAD",
            "--",
            &relative,
        ],
    )?;
    let old_source = match git_output(&root, &["cat-file", "-e", &format!("HEAD:{relative}")]) {
        Some(_) => git_required(&root, &["show", &format!("HEAD:{relative}")])?,
        None => String::new(),
    };
    Ok(GitDiff {
        path,
        repo_root: root,
        old_source,
        patch: parse_patch(&patch)?,
    })
}

/// Render parsed Git hunks as themed, escaped diff markup.
pub fn render_body(
    diff: &GitDiff,
    working_source: &str,
    highlighter: &Highlighter,
    layout: DiffLayout,
) -> String {
    if diff.patch.is_empty() {
        return "<div class=\"mdview-diff-empty\">No changes against HEAD.</div>".to_string();
    }
    let old_lines = highlighter.render_markdown_lines(&diff.old_source);
    let new_lines = highlighter.render_markdown_lines(working_source);
    let mut html = format!(
        "<div class=\"mdview-diff mdview-diff-{}\" role=\"table\" aria-label=\"Git diff\">",
        match layout {
            DiffLayout::Unified => "unified",
            DiffLayout::Split => "split",
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
            DiffLayout::Unified => render_unified_rows(&mut html, hunk, &old_lines, &new_lines),
            DiffLayout::Split => render_split_rows(&mut html, hunk, &old_lines, &new_lines),
        }
        html.push_str("</section>");
    }
    html.push_str("</div>");
    html
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

    fn temp_repo() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mdview-diff-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
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
        };
        let highlighter = Highlighter::new();
        let unified = render_body(&diff, "# After\n<script>alert(1)</script>\n", &highlighter, DiffLayout::Unified);
        let split = render_body(&diff, "# After\n<script>alert(1)</script>\n", &highlighter, DiffLayout::Split);
        assert!(unified.contains("mdview-diff-unified"));
        assert!(split.contains("mdview-diff-split"));
        assert!(unified.contains("&lt;") && unified.contains("&gt;"));
        assert!(!unified.contains("<script>alert"));
        assert!(split.contains("mdview-diff-placeholder") || split.contains("mdview-diff-side"));
    }
}
