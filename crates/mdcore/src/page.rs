use std::hash::{BuildHasher, Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::assets;
use crate::chrome;
use crate::document::Document;
use crate::highlight;
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

/// Build the theme picker markup listing all themes, marking `selected` as
/// the current one via `aria-checked` so the list reflects the page's own
/// theme rather than relying on JS to discover it after the fact.
fn build_theme_picker(selected: Theme) -> String {
    let mut html = String::from("<details id=\"mdview-theme\"><summary aria-label=\"Theme\" title=\"Theme\"><svg width=\"16\" height=\"16\" viewBox=\"0 0 16 16\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.5\"><circle cx=\"8\" cy=\"8\" r=\"6.25\"/><path d=\"M8 1.75a6.25 6.25 0 0 1 0 12.5z\" fill=\"currentColor\" stroke=\"none\"/></svg></summary>\n<div class=\"mdview-theme-list\" role=\"menu\">\n");

    // Group themes: System, then Light, then Dark
    let mut groups: Vec<(Option<bool>, Vec<Theme>)> = vec![
        (None, vec![]),
        (Some(false), vec![]),
        (Some(true), vec![]),
    ];

    for theme in Theme::all() {
        let is_dark = theme.is_dark();
        if let Some(group) = groups.iter_mut().find(|(k, _)| *k == is_dark) {
            group.1.push(*theme);
        }
    }

    for (is_dark, themes) in groups {
        if !themes.is_empty() {
            if let Some(false) = is_dark {
                html.push_str("<div class=\"mdview-theme-group\"><div class=\"mdview-theme-group-label\">Light</div>\n");
            } else if let Some(true) = is_dark {
                html.push_str("<div class=\"mdview-theme-group\"><div class=\"mdview-theme-group-label\">Dark</div>\n");
            }

            for theme in themes {
                html.push_str(&format!(
                    "<button type=\"button\" class=\"mdview-theme-item\" role=\"menuitemradio\" aria-checked=\"{}\" data-theme-id=\"{}\">{}</button>\n",
                    theme == selected,
                    theme.as_wire(),
                    theme.label()
                ));
            }

            if is_dark.is_some() {
                html.push_str("</div>\n");
            }
        }
    }

    html.push_str("</div></details>");
    html
}

/// Assemble a complete, self-contained HTML document around rendered body HTML.
pub fn build_page(doc: &Document, body_html: &str, theme: Theme) -> String {
    let (light_css, dark_css) = highlight::theme_css();
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
    // A named theme's darkness cannot be recovered on the JS side from its
    // wire value ("mocha", "github", ...) -- only Rust knows it, via
    // `Theme::is_dark`. Stamp it explicitly so `effectiveTheme()` in
    // init.js does not have to guess and fall through to the OS media
    // query, which would render Mermaid diagrams in the wrong palette.
    // System alone gets no stamp and defers to the OS, as before.
    let dark_attr = match theme.is_dark() {
        Some(true) => " data-dark=\"1\"".to_string(),
        Some(false) => " data-dark=\"0\"".to_string(),
        None => String::new(),
    };

    // Build chrome tokens and syntax CSS based on the selected theme
    let (chrome_css, named_theme_css) = match theme {
        Theme::System => {
            // For System, emit nothing special (page.css handles the defaults)
            (String::new(), String::new())
        }
        _ => {
            // For named themes, derive chrome from the palette and emit the syntect CSS
            let syntect_name = theme.syntect_name().unwrap();
            if let Some((css, bg, fg)) = highlight::palette_for(syntect_name) {
                let is_dark = theme.is_dark().unwrap();
                let tokens = chrome::tokens(bg, fg, is_dark);
                let chrome_vars = tokens.to_css_vars();
                let chrome_style = format!(":root {{{}}}", chrome_vars);
                (format!("<style>{}</style>\n", chrome_style), format!("<style media=\"all\">{}</style>\n", css))
            } else {
                (String::new(), String::new())
            }
        }
    };

    // Syntect's stylesheets are full rulesets, so they cannot be wrapped in a
    // `:root[data-theme=…]` block — that is CSS Nesting, unsupported by WebKit
    // before macOS 13.4 while this app supports 11.0. For System, select sheets
    // with `media` attributes. For named themes, emit the theme's own syntect CSS
    // with media="all", and disable the System sheets with media="not all".
    let (light_media, dark_media) = match theme {
        Theme::System => ("all", "(prefers-color-scheme: dark)"),
        _ => ("not all", "not all"),
    };

    let theme_picker = build_theme_picker(theme);

    format!(
        r#"<!DOCTYPE html>
<html{theme_attr}{dark_attr}>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="{csp}">
<title>{title}</title>
<style>{page_css}</style>
<style>{katex_css}</style>
{chrome_css}{named_theme_css}<style id="mdview-hl-light" media="{light_media}">{light_css}</style>
<style id="mdview-hl-dark" media="{dark_media}">{dark_css}</style>
</head>
<body>
<div id="mdview-banners"></div>
<div id="mdview-layout">
<main id="mdview-main"><div id="mdview-content">{body}</div></main>
<aside id="mdview-sidebar" hidden>
<header class="mdview-sidebar-head">
<nav class="mdview-tabs" role="tablist">
<button type="button" class="mdview-tab" role="tab" aria-controls="mdview-sidebar-body" data-tab="outline" aria-label="Outline" title="Outline"><svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><line x1="2" y1="3" x2="8" y2="3"/><line x1="2" y1="8" x2="12" y2="8"/><line x1="2" y1="13" x2="14" y2="13"/></svg></button>
<button type="button" class="mdview-tab" role="tab" aria-controls="mdview-sidebar-body" data-tab="bookmarks" aria-label="Bookmarks" title="Bookmarks"><svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M3 2h10v9.5L8 9l-5 2.5V2z"/></svg></button>
</nav>
<div class="mdview-sidebar-actions">
<button type="button" id="mdview-star" aria-label="Bookmark this document" title="Bookmark this document" aria-pressed="false"><svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"><path d="M8 1.9l1.88 3.8 4.2.61-3.04 2.96.72 4.18L8 11.48l-3.76 1.97.72-4.18L1.92 6.31l4.2-.61z"/></svg></button>
{theme_picker}
</div>
</header>
<div id="mdview-sidebar-body" role="tabpanel"></div>
</aside>
<button type="button" id="mdview-sidebar-toggle" aria-label="Toggle sidebar" title="Toggle sidebar" aria-expanded="false"><svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5"><line x1="2" y1="4" x2="18" y2="4"/><line x1="2" y1="10" x2="18" y2="10"/><line x1="2" y1="16" x2="18" y2="16"/></svg></button>
</div>
<script nonce="{nonce}">{katex_js}</script>
<script nonce="{nonce}">{mermaid_js}</script>
<script nonce="{nonce}">{init_js}</script>
</body>
</html>
"#,
        theme_attr = theme_attr,
        dark_attr = dark_attr,
        csp = csp,
        nonce = nonce,
        title = crate::escape::escape_html(&title),
        page_css = assets::PAGE_CSS,
        katex_css = assets::KATEX_CSS,
        chrome_css = chrome_css,
        named_theme_css = named_theme_css,
        light_media = light_media,
        dark_media = dark_media,
        light_css = light_css,
        dark_css = dark_css,
        body = body_html,
        theme_picker = theme_picker,
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
    fn system_pins_no_theme_and_keeps_the_media_query() {
        let html = build_page(&doc(), "", Theme::System);
        assert!(!html.contains("<html data-theme="), "System must not pin a theme");
        assert!(html.contains("@media (prefers-color-scheme: dark)"));
    }

    #[test]
    fn a_named_theme_pins_the_attribute_and_emits_derived_chrome() {
        let html = build_page(&doc(), "", Theme::Mocha);
        assert!(html.contains("<html data-theme=\"mocha\""));
        // Chrome comes from the syntect palette, so Mocha's own background
        // must appear as the --bg token rather than a hand-written colour.
        assert!(html.contains("--bg:#3b3228"), "chrome not derived from the palette");
    }

    /// Pull just the `<html ...>` opening tag, so an assertion about its
    /// attributes cannot be satisfied by a coincidental match elsewhere in
    /// the page (e.g. the bundled JS's own comments and string literals
    /// naming the same attribute).
    fn html_tag(html: &str) -> &str {
        let start = html.find("<html").expect("html tag");
        let end = html[start..].find('>').map(|i| start + i).expect("html tag close");
        &html[start..=end]
    }

    #[test]
    fn a_dark_named_theme_is_stamped_data_dark_1() {
        let html = build_page(&doc(), "", Theme::Mocha);
        assert!(
            html_tag(&html).contains("data-dark=\"1\""),
            "dark theme must be stamped data-dark=1: {}",
            html_tag(&html)
        );
    }

    #[test]
    fn a_light_named_theme_is_stamped_data_dark_0() {
        let html = build_page(&doc(), "", Theme::GitHub);
        assert!(
            html_tag(&html).contains("data-dark=\"0\""),
            "light theme must be stamped data-dark=0: {}",
            html_tag(&html)
        );
    }

    #[test]
    fn system_emits_no_data_dark_attribute() {
        // JS cannot derive System's darkness -- it must fall through to the
        // OS media query -- so no stamp at all must appear for it.
        let html = build_page(&doc(), "", Theme::System);
        assert!(
            !html_tag(&html).contains("data-dark"),
            "System must not be stamped data-dark: {}",
            html_tag(&html)
        );
    }

    #[test]
    fn the_picker_marks_only_the_selected_theme_as_checked() {
        let html = build_page(&doc(), "", Theme::Mocha);
        let occurrences = html.matches("aria-checked=\"true\"").count();
        assert_eq!(occurrences, 1, "expected exactly one item marked checked, saw {occurrences}");

        let idx = html.find("aria-checked=\"true\"").expect("a checked item");
        let tag_start = html[..idx].rfind("<button").expect("enclosing button tag");
        let tag_end = html[idx..].find('>').map(|i| idx + i).expect("tag close");
        let tag = &html[tag_start..tag_end];
        assert!(
            tag.contains("data-theme-id=\"mocha\""),
            "the checked item must be the selected theme, got: {tag}"
        );

        assert!(html.contains("role=\"menu\""), "picker list must be a menu");
        assert!(html.contains("role=\"menuitemradio\""), "picker items must be menuitemradio");
    }

    #[test]
    fn each_named_theme_emits_a_distinct_page() {
        let mut seen = std::collections::HashSet::new();
        for theme in Theme::all().iter().filter(|t| **t != Theme::System) {
            assert!(
                seen.insert(build_page(&doc(), "", *theme)),
                "{} produced a duplicate page",
                theme.label()
            );
        }
    }

    #[test]
    fn the_picker_lists_every_theme_by_label() {
        let html = build_page(&doc(), "", Theme::System);
        for theme in Theme::all() {
            assert!(
                html.contains(&format!("data-theme-id=\"{}\"", theme.as_wire())),
                "picker missing {}",
                theme.label()
            );
            assert!(html.contains(theme.label()), "picker missing label {}", theme.label());
        }
    }

    #[test]
    fn no_css_nesting_is_emitted() {
        // WebKit before macOS 13.4 cannot parse nested rules and the bundle
        // declares 11.0; a nested block fails silently at parse time.
        let html = build_page(&doc(), "", Theme::Mocha);
        assert!(!html.contains(":root[data-theme=\"mocha\"] { /*"));
    }

    #[test]
    fn sidebar_lives_outside_the_swappable_content() {
        let html = build_page(&doc(), "<p>hi</p>", Theme::System);
        let sidebar = html.find("id=\"mdview-sidebar\"").expect("sidebar missing");
        let content_open = html.find("id=\"mdview-content\"").expect("content missing");
        let content_close = html.find("</main>").expect("main must close");
        // Live reload replaces #mdview-content's innerHTML. The sidebar must
        // not be inside it, or every save would destroy the sidebar.
        assert!(
            sidebar < content_open || sidebar > content_close,
            "sidebar must not sit inside #mdview-content"
        );
    }

    #[test]
    fn sidebar_exposes_the_contract_later_tasks_fill_in() {
        let html = build_page(&doc(), "", Theme::System);
        assert!(html.contains("id=\"mdview-sidebar-body\""));
        assert!(html.contains("data-tab=\"outline\""));
        assert!(html.contains("data-tab=\"bookmarks\""));
    }

    #[test]
    fn the_sidebar_toggle_survives_a_hidden_sidebar() {
        let html = build_page(&doc(), "<p>hi</p>", Theme::System);
        let toggle = html.find("id=\"mdview-sidebar-toggle\"").expect("toggle missing");
        let aside_end = html.find("</aside>").expect("aside must close");
        let content = html.find("id=\"mdview-content\"").expect("content missing");
        let main_end = html.find("</main>").expect("main must close");
        // Outside the aside, or hiding the sidebar hides its own toggle.
        assert!(toggle > aside_end, "toggle must not live inside the sidebar");
        // Outside the swapped content, or a live-reload save destroys it.
        assert!(toggle < content || toggle > main_end, "toggle must not live inside #mdview-content");
    }

    /// Reads the z-index of a rule block in page.css.
    fn z_index_of(selector: &str) -> i32 {
        let css = assets::PAGE_CSS;
        let start = css.find(selector).unwrap_or_else(|| panic!("no rule for {selector}"));
        let block = &css[start..];
        let end = block.find('}').expect("unterminated rule");
        let decl = block[..end]
            .split("z-index:")
            .nth(1)
            .unwrap_or_else(|| panic!("{selector} declares no z-index"));
        decl.trim_start()
            .trim_end_matches(|c: char| !c.is_ascii_digit() && c != '-')
            .trim()
            .trim_end_matches(';')
            .parse()
            .expect("z-index must be an integer")
    }

    #[test]
    fn the_sidebar_toggle_paints_above_the_banner_bar() {
        // #mdview-banners is a full-viewport-width sticky bar at top:0, and the
        // toggle is fixed at top:1rem — they overlap. The toggle must win, or a
        // live-reload banner hides the only control that opens the sidebar.
        assert!(
            z_index_of("#mdview-sidebar-toggle {") > z_index_of("#mdview-banners {"),
            "the sidebar toggle must paint above the banner bar"
        );
    }

    #[test]
    fn the_sidebar_reserves_room_for_the_fixed_toggle() {
        // The header's controls (tabs, star, theme picker) would otherwise sit
        // under the toggle, which is fixed at top-right and paints above them.
        // The panel clears it vertically so the toolbar can span the full width.
        let css = assets::PAGE_CSS;
        let head = css.find(".mdview-sidebar-head {").expect("no sidebar-head rule");
        let head_block = &css[head..head + css[head..].find('}').expect("unterminated rule")];
        assert!(
            head_block.contains("padding-right:"),
            "the toolbar must reserve the fixed toggle's slot at the end of its row"
        );
        let start = css.find("#mdview-sidebar {").expect("no sidebar rule");
        let block = &css[start..start + css[start..].find('}').expect("unterminated rule")];
        // That padding is added to height:100vh unless the box is border-box,
        // which would push the panel past the bottom of the viewport.
        assert!(
            !block.contains("height: 100vh") || block.contains("box-sizing: border-box"),
            "a 100vh sidebar with padding must be border-box"
        );
    }

    #[test]
    fn the_theme_picker_hides_its_disclosure_triangle_when_closed() {
        // Scoping the marker rule to [open] left a stray triangle on the closed
        // picker, which is the state it is in almost all of the time.
        let css = assets::PAGE_CSS;
        let marker = css
            .find("summary::-webkit-details-marker")
            .expect("no marker rule");
        let line_start = css[..marker].rfind('\n').map_or(0, |i| i + 1);
        assert!(
            !css[line_start..marker].contains("[open]"),
            "the disclosure triangle must be hidden in both states"
        );
    }

    #[test]
    fn old_sidebar_button_ids_are_gone() {
        let html = build_page(&doc(), "", Theme::System);
        assert!(
            !html.contains("id=\"mdview-sidebar-open\""),
            "mdview-sidebar-open should be replaced by mdview-sidebar-toggle"
        );
        assert!(
            !html.contains("id=\"mdview-sidebar-close\""),
            "mdview-sidebar-close should be replaced by mdview-sidebar-toggle"
        );
    }

    #[test]
    fn sidebar_tabs_have_icon_markup() {
        let html = build_page(&doc(), "", Theme::System);
        // Count SVG elements that serve as icons
        let svg_count = html.matches("<svg").count();
        assert!(
            svg_count >= 2,
            "tabs should have SVG icons for outline and bookmarks, got {svg_count} svgs"
        );
    }

    #[test]
    fn sidebar_toggle_has_aria_expanded() {
        let html = build_page(&doc(), "", Theme::System);
        let toggle_start = html
            .find("id=\"mdview-sidebar-toggle\"")
            .expect("toggle missing");
        let toggle_end = html[toggle_start..].find('>').expect("tag close") + toggle_start;
        let toggle_tag = &html[toggle_start..=toggle_end];
        assert!(
            toggle_tag.contains("aria-expanded"),
            "toggle must have aria-expanded attribute, got: {toggle_tag}"
        );
    }

    #[test]
    fn sidebar_tabs_have_accessible_labels() {
        let html = build_page(&doc(), "", Theme::System);
        assert!(html.contains("aria-label=\"Outline\""), "outline tab needs aria-label");
        assert!(
            html.contains("aria-label=\"Bookmarks\""),
            "bookmarks tab needs aria-label"
        );
        assert!(html.contains("title=\"Outline\""), "outline tab needs title");
        assert!(html.contains("title=\"Bookmarks\""), "bookmarks tab needs title");
    }

    #[test]
    fn no_inline_click_handlers() {
        let html = build_page(&doc(), "", Theme::System);
        assert!(
            !html.contains("onclick="),
            "no inline onclick= handlers allowed; use addEventListener"
        );
        assert!(
            !html.contains("onchange="),
            "no inline onchange= handlers allowed; use addEventListener"
        );
    }
}
