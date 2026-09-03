#![forbid(unsafe_code)]

//! Markdown to self-contained HTML. Knows nothing about AppKit or windows.

pub mod assets;
pub mod chrome;
pub mod document;
pub mod diff;
pub mod escape;
pub mod frontmatter;
pub mod highlight;
pub mod images;
pub mod math;
pub mod page;
pub mod rdiff;
pub mod render;
pub mod theme;

use std::path::{Path, PathBuf};

pub use chrome::Rgb;
pub use document::{Document, DocumentError};
pub use diff::{
    DiffAvailability, DiffError, DiffHunk, DiffLayout, DiffLine, DiffLineKind, GitDiff,
    SourceLayout, SplitRow,
};
pub use highlight::Highlighter;
pub use render::headings;
pub use theme::Theme;

/// A document rendered and ready to hand to a web view.
#[derive(Debug, Clone)]
pub struct RenderedDoc {
    /// A complete, self-contained HTML page.
    pub html: String,
    /// Directory the document lives in; becomes the web view's base URL.
    pub base_dir: PathBuf,
    /// True when the file was not valid UTF-8 and was decoded lossily.
    pub lossy: bool,
}

/// Load and render a Markdown file. This is the entire contract `mdapp` uses.
pub fn render_document(path: impl AsRef<Path>, theme: Theme) -> Result<RenderedDoc, DocumentError> {
    let highlighter = Highlighter::new();
    render_document_with(path, &highlighter, theme)
}

/// Same as `render_document`, reusing a `Highlighter` across renders. Live
/// reload uses this so that saving a file does not rebuild the syntax set.
pub fn render_document_with(
    path: impl AsRef<Path>,
    highlighter: &Highlighter,
    theme: Theme,
) -> Result<RenderedDoc, DocumentError> {
    let doc = Document::load(path)?;
    let body = render::render_body_in(&doc.source, highlighter, Some(&doc.base_dir));
    Ok(RenderedDoc {
        html: page::build_page(&doc, &body, theme),
        base_dir: doc.base_dir.clone(),
        lossy: doc.lossy,
    })
}

/// Render only the body HTML, for live-reload swaps into an already-loaded
/// page. Returns `(body_html, lossy)`.
pub fn render_body_of(
    path: impl AsRef<Path>,
    highlighter: &Highlighter,
) -> Result<(String, bool), DocumentError> {
    let doc = Document::load(path)?;
    let body = render::render_body_in(&doc.source, highlighter, Some(&doc.base_dir));
    Ok((body, doc.lossy))
}

/// Load and render a Markdown file as a Git diff against HEAD.
pub fn render_diff_document_with(
    path: impl AsRef<Path>,
    highlighter: &Highlighter,
    theme: Theme,
    layout: DiffLayout,
) -> Result<RenderedDoc, DiffError> {
    let doc = Document::load(path).map_err(|err| DiffError::Git(err.to_string()))?;
    let diff = diff::load_diff(&doc.path)?;
    let body = diff_body(&doc, &diff, highlighter, layout);
    Ok(RenderedDoc {
        html: page::build_diff_page(&doc, &body, theme, layout),
        base_dir: doc.base_dir.clone(),
        lossy: doc.lossy,
    })
}

/// The body of a diff, in whichever layout was asked for.
///
/// The two source layouts are drawn from Git's hunks; the two rendered ones are
/// drawn from the two versions of the document, and are the only ones that need
/// the document's directory -- their images are real images, and a web view
/// handed a page as a string cannot fetch them for itself.
fn diff_body(
    doc: &Document,
    diff: &GitDiff,
    highlighter: &Highlighter,
    layout: DiffLayout,
) -> String {
    if diff.patch.is_empty() {
        return diff::NO_CHANGES_HTML.to_string();
    }
    let rendered = |columns| {
        rdiff::render_body(
            &diff.old_source,
            &doc.source,
            highlighter,
            Some(&doc.base_dir),
            columns,
        )
    };
    match layout {
        DiffLayout::Unified => {
            diff::render_body(diff, &doc.source, highlighter, SourceLayout::Unified)
        }
        DiffLayout::Split => diff::render_body(diff, &doc.source, highlighter, SourceLayout::Split),
        DiffLayout::Rendered => rendered(rdiff::Columns::Single),
        DiffLayout::RenderedSplit => rendered(rdiff::Columns::Split),
    }
}

/// Render only the Git diff body for live reload.
pub fn render_diff_body_of(
    path: impl AsRef<Path>,
    highlighter: &Highlighter,
    layout: DiffLayout,
) -> Result<(String, bool), DiffError> {
    let doc = Document::load(path).map_err(|err| DiffError::Git(err.to_string()))?;
    let diff = diff::load_diff(&doc.path)?;
    Ok((diff_body(&doc, &diff, highlighter, layout), doc.lossy))
}

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
