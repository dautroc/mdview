use crate::assets;
use crate::document::Document;
use crate::highlight::theme_css;

/// Content Security Policy for the rendered page.
///
/// `img-src` permits remote images because documents legitimately contain
/// them; every executable source is denied, so the only scripts that can run
/// are the ones compiled into this binary.
pub const CSP: &str = "default-src 'none'; img-src 'self' data: file: https:; \
style-src 'unsafe-inline'; script-src 'unsafe-inline'; font-src data:;";

/// Assemble a complete, self-contained HTML document around rendered body HTML.
pub fn build_page(doc: &Document, body_html: &str) -> String {
    let (light_css, dark_css) = theme_css();
    let title = doc
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".to_string());

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="{csp}">
<title>{title}</title>
<style>{page_css}</style>
<style>{katex_css}</style>
<style>{light_css}</style>
<style>@media (prefers-color-scheme: dark) {{ {dark_css} }}</style>
</head>
<body>
<div id="mdview-banners"></div>
<div id="mdview-content">{body}</div>
<script>{katex_js}</script>
<script>{mermaid_js}</script>
<script>{init_js}</script>
</body>
</html>
"#,
        csp = CSP,
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
        let html = build_page(&doc(), "<p>hi</p>");
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<p>hi</p>"));
        assert!(html.trim_end().ends_with("</html>"));
    }

    #[test]
    fn csp_is_present_and_exact() {
        let html = build_page(&doc(), "");
        assert!(
            html.contains(&format!("content=\"{CSP}\"")),
            "CSP must be embedded verbatim"
        );
    }

    #[test]
    fn both_highlight_themes_are_emitted_under_a_media_query() {
        let html = build_page(&doc(), "");
        assert!(html.contains("@media (prefers-color-scheme: dark)"));
        // The syntect class prefix appears in both light and dark blocks.
        let occurrences = html.matches(".code").count();
        assert!(occurrences >= 2, "expected both themes, saw {occurrences}");
    }

    #[test]
    fn assets_are_inlined_not_linked() {
        let html = build_page(&doc(), "");
        assert!(!html.contains("<link"), "no external stylesheets");
        assert!(!html.contains("src=\"http"), "no external scripts");
        assert!(html.contains("katex"), "katex must be inlined");
        assert!(html.contains("mermaid"), "mermaid must be inlined");
    }

    #[test]
    fn title_is_the_file_name() {
        let html = build_page(&doc(), "");
        assert!(html.contains("<title>x.md</title>"), "got title mismatch");
    }

    #[test]
    fn banner_container_exists_outside_the_swappable_body() {
        let html = build_page(&doc(), "<p>hi</p>");
        let banners = html.find("id=\"mdview-banners\"").expect("banner container");
        let content = html.find("id=\"mdview-content\"").expect("content container");
        assert!(banners < content, "banners must precede the swappable content");
    }
}
