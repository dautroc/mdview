#![forbid(unsafe_code)]

//! Markdown to self-contained HTML. Knows nothing about AppKit or windows.

pub mod assets;
pub mod chrome;
pub mod document;
pub mod diff;
pub mod escape;
pub mod highlight;
pub mod images;
pub mod math;
pub mod page;
pub mod render;
pub mod theme;

use std::path::{Path, PathBuf};

pub use chrome::Rgb;
pub use document::{Document, DocumentError};
pub use diff::{DiffAvailability, DiffError, DiffHunk, DiffLayout, DiffLine, DiffLineKind, GitDiff, SplitRow};
pub use highlight::Highlighter;
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
    let body = diff::render_body(&diff, &doc.source, highlighter, layout);
    Ok(RenderedDoc {
        html: page::build_page(&doc, &body, theme),
        base_dir: doc.base_dir.clone(),
        lossy: doc.lossy,
    })
}

/// Render only the Git diff body for live reload.
pub fn render_diff_body_of(
    path: impl AsRef<Path>,
    highlighter: &Highlighter,
    layout: DiffLayout,
) -> Result<(String, bool), DiffError> {
    let doc = Document::load(path).map_err(|err| DiffError::Git(err.to_string()))?;
    let diff = diff::load_diff(&doc.path)?;
    Ok((
        diff::render_body(&diff, &doc.source, highlighter, layout),
        doc.lossy,
    ))
}

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
