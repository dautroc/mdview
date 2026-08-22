use std::hash::{BuildHasher, Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::assets;
use crate::document::Document;
use crate::highlight::theme_css;
use crate::theme::Theme;

/// Process-lifetime counter mixed into every nonce, so that two pages built
/// back-to-back are guaranteed distinct even if the clock below doesn't
/// advance between them.
static PAGE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generate a per-page CSP nonce.
///
/// This crate has no `rand` dependency and must not add one, so the nonce is
/// derived from a process-lifetime counter, the current time, and a
/// `RandomState`-derived hash (seeded from OS randomness once per process) —
/// not a cryptographic RNG. That is adequate for the actual threat: a
/// document's own `<script>` tag guessing the nonce of the page it is being
/// rendered into, so it can stamp a matching `nonce="..."` on itself and run
/// under our CSP. The page — and its nonce — is generated fresh per render,
/// strictly after the document's own content is already fixed on disk, so
/// the document has no channel to observe the nonce before it is embedded in
/// HTML the document does not control. There is no need to defend against a
/// stronger adversary (e.g. one that can already run code in the process)
/// here.
fn generate_nonce() -> String {
    let counter = PAGE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    counter.hash(&mut hasher);
    nanos.hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Content Security Policy for the rendered page, parameterized on a
/// per-page nonce.
///
/// `img-src` permits remote images because documents legitimately contain
/// them, and that is a deliberately accepted (documented) exfiltration
/// channel — `default-src 'none'` still blocks fetch/XHR/WebSocket. Every
/// *executable* source is restricted to the nonce: `script-src 'nonce-...'`
/// means only the three `<script nonce="...">` tags this module stamps with
/// the same value can run, so a `<script>` embedded in the Markdown document
/// itself (passed through by `render.rs`, since raw HTML pass-through is not
/// in scope to remove) is inert. `style-src` keeps `'unsafe-inline'`: KaTeX
/// emits inline `style="..."` attributes it needs, and that is far less
/// dangerous than inline script.
fn csp_header(nonce: &str) -> String {
    format!(
        "default-src 'none'; img-src 'self' data: file: https:; \
style-src 'unsafe-inline'; script-src 'nonce-{nonce}'; font-src data:;"
    )
}

/// Assemble a complete, self-contained HTML document around rendered body HTML.
pub fn build_page(doc: &Document, body_html: &str, theme: Theme) -> String {
    let (light_css, dark_css) = theme_css();
    let title = doc
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".to_string());
    let nonce = generate_nonce();
    let csp = csp_header(&nonce);
    let theme_attr = match theme {
        Theme::System => String::new(),
        other => format!(" data-theme=\"{}\"", other.as_wire()),
    };

    format!(
        r#"<!DOCTYPE html>
<html{theme_attr}>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="{csp}">
<title>{title}</title>
<style>{page_css}</style>
<style>{katex_css}</style>
<style>{light_css}</style>
<style>@media (prefers-color-scheme: dark) {{ {dark_css} }}</style>
<style>:root[data-theme="dark"] {{ {dark_css} }}</style>
<style>:root[data-theme="light"] {{ {light_css} }}</style>
</head>
<body>
<div id="mdview-banners"></div>
<div id="mdview-content">{body}</div>
<script nonce="{nonce}">{katex_js}</script>
<script nonce="{nonce}">{mermaid_js}</script>
<script nonce="{nonce}">{init_js}</script>
</body>
</html>
"#,
        theme_attr = theme_attr,
        csp = csp,
        nonce = nonce,
        title = crate::escape::escape_html(&title),
        page_css = assets::PAGE_CSS,
        katex_css = assets::KATEX_CSS,
        light_css = light_css,
        dark_css = dark_css,
        body = body_html,
        katex_js = assets::KATEX_JS,
        mermaid_js = assets::MERMAID_JS,
        init_js = assets::INIT_JS,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn doc() -> Document {
        Document {
            path: PathBuf::from("/tmp/notes/x.md"),
            base_dir: PathBuf::from("/tmp/notes"),
            source: String::new(),
            lossy: false,
        }
    }

    #[test]
    fn page_is_a_complete_html_document() {
        let html = build_page(&doc(), "<p>hi</p>", Theme::System);
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<p>hi</p>"));
        assert!(html.trim_end().ends_with("</html>"));
    }

    /// Pull the `script-src` directive's value out of the CSP meta tag, e.g.
    /// `"'nonce-abc123'"` — the piece between `script-src ` and the next `;`.
    ///
    /// Locates the CSP `<meta>` tag specifically (not just the first
    /// `content="..."` in the document — the viewport `<meta>` tag has one of
    /// those too, and comes first).
    fn script_src(html: &str) -> String {
        let meta_marker = "http-equiv=\"Content-Security-Policy\" content=\"";
        let meta_start = html.find(meta_marker).expect("CSP meta tag") + meta_marker.len();
        let csp_end = html[meta_start..].find('"').expect("closing quote") + meta_start;
        let csp = &html[meta_start..csp_end];
        let directive_start = csp.find("script-src ").expect("script-src directive") + "script-src ".len();
        let directive_end = csp[directive_start..].find(';').map(|i| directive_start + i).unwrap_or(csp.len());
        csp[directive_start..directive_end].trim().to_string()
    }

    #[test]
    fn script_src_carries_a_nonce_matching_the_bundled_scripts() {
        let html = build_page(&doc(), "", Theme::System);
        let script_src = script_src(&html);

        assert!(
            script_src.starts_with("'nonce-") && script_src.ends_with('\''),
            "script-src should be a single nonce source, got: {script_src}"
        );
        let nonce = script_src
            .trim_start_matches("'nonce-")
            .trim_end_matches('\'');
        assert!(!nonce.is_empty());

        let tag = format!("<script nonce=\"{nonce}\">");
        let occurrences = html.matches(&tag).count();
        assert_eq!(
            occurrences, 3,
            "expected all three bundled <script> tags to carry the CSP nonce, got {occurrences}: {html}"
        );
    }

    #[test]
    fn unsafe_inline_is_not_permitted_in_script_src() {
        let html = build_page(&doc(), "", Theme::System);
        assert!(
            !script_src(&html).contains("unsafe-inline"),
            "script-src must not carry 'unsafe-inline', or any inline <script> in \
             the document itself would execute"
        );
    }

    #[test]
    fn style_src_still_allows_inline_for_katex() {
        let html = build_page(&doc(), "", Theme::System);
        assert!(
            html.contains("style-src 'unsafe-inline'"),
            "KaTeX emits inline style attributes and needs this"
        );
    }

    #[test]
    fn two_pages_get_different_nonces() {
        let html_a = build_page(&doc(), "", Theme::System);
        let html_b = build_page(&doc(), "", Theme::System);
        assert_ne!(
            script_src(&html_a),
            script_src(&html_b),
            "each page must get its own nonce"
        );
    }

    #[test]
    fn a_script_tag_in_the_document_cannot_execute_under_the_emitted_csp() {
        // The raw HTML still passes through into the body (a separate,
        // out-of-scope product decision) — but the CSP that governs the page
        // it lands in must not permit it to run.
        let html = build_page(&doc(), "<script>alert(1)</script>", Theme::System);
        assert!(html.contains("<script>alert(1)</script>"), "sanity: raw HTML still passes through");
        assert!(
            !script_src(&html).contains("unsafe-inline"),
            "script-src must not allow the document's own inline <script> to execute"
        );
    }

    #[test]
    fn both_highlight_themes_are_emitted_under_a_media_query() {
        let html = build_page(&doc(), "", Theme::System);
        assert!(html.contains("@media (prefers-color-scheme: dark)"));
        // The syntect class prefix appears in both light and dark blocks.
        let occurrences = html.matches(".code").count();
        assert!(occurrences >= 2, "expected both themes, saw {occurrences}");
    }

    #[test]
    fn assets_are_inlined_not_linked() {
        let html = build_page(&doc(), "", Theme::System);
        assert!(!html.contains("<link"), "no external stylesheets");
        assert!(!html.contains("src=\"http"), "no external scripts");
        assert!(html.contains("katex"), "katex must be inlined");
        assert!(html.contains("mermaid"), "mermaid must be inlined");
    }

    #[test]
    fn title_is_the_file_name() {
        let html = build_page(&doc(), "", Theme::System);
        assert!(html.contains("<title>x.md</title>"), "got title mismatch");
    }

    #[test]
    fn banner_container_exists_outside_the_swappable_body() {
        let html = build_page(&doc(), "<p>hi</p>", Theme::System);
        let banners = html.find("id=\"mdview-banners\"").expect("banner container");
        let content = html.find("id=\"mdview-content\"").expect("content container");
        assert!(banners < content, "banners must precede the swappable content");
    }

    #[test]
    fn zoom_affordances_are_embedded_in_the_page() {
        let html = build_page(&doc(), "<p>hi</p>", Theme::System);
        // CSS reached the page
        assert!(html.contains(".mdview-zoomable"), "zoom CSS missing");
        assert!(html.contains(".mdview-zoom-btn"), "zoom button CSS missing");
        assert!(html.contains("mdview-lightbox"), "lightbox styles/markup missing");
        // JS reached the page
        assert!(html.contains("enhanceZoomables"), "zoom JS missing");
        // Still no remote assets
        assert!(!html.contains("<link"), "no external stylesheets");
        assert!(!html.contains("src=\"http"), "no external scripts");
    }

    #[test]
    fn system_theme_emits_no_data_theme_attribute() {
        // With no attribute the existing prefers-color-scheme query decides,
        // which is exactly what "System" means.
        let html = build_page(&doc(), "", crate::theme::Theme::System);
        assert!(!html.contains("<html data-theme="), "System must not pin a theme");
    }

    #[test]
    fn explicit_themes_pin_the_attribute() {
        assert!(build_page(&doc(), "", crate::theme::Theme::Dark).contains("data-theme=\"dark\""));
        assert!(build_page(&doc(), "", crate::theme::Theme::Light).contains("data-theme=\"light\""));
    }

    #[test]
    fn theme_overrides_beat_the_media_query() {
        let html = build_page(&doc(), "", crate::theme::Theme::System);
        // Both override blocks ship in every page; only the attribute changes.
        assert!(html.contains("[data-theme=\"dark\"]"), "dark override CSS missing");
        assert!(html.contains("[data-theme=\"light\"]"), "light override CSS missing");
    }
}
