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
///
/// The list is flat: the names carry their own light/dark sense, so grouping
/// headings only added rows to scan past. Each item also carries the theme's
/// darkness, which the wire value alone does not reveal, so a hover preview
/// can stamp `data-dark` without a round trip to Rust.
/// The theme list, as data for the page's palette to build itself from.
///
/// This used to be the picker's markup. It is emitted as a plain array because
/// only Rust knows the list, the labels, and each theme's darkness -- a wire
/// value like "mocha" does not say whether it is dark, and `Theme::is_dark` is
/// the only thing that does.
fn build_theme_catalogue() -> String {
    let mut entries = Vec::new();
    for theme in Theme::all() {
        let dark = match theme.is_dark() {
            Some(true) => "1",
            Some(false) => "0",
            None => "",
        };
        entries.push(format!(
            "{{id:{},label:{},dark:{}}}",
            crate::escape::js_string_literal(theme.as_wire()),
            crate::escape::js_string_literal(theme.label()),
            crate::escape::js_string_literal(dark),
        ));
    }
    format!("[{}]", entries.join(","))
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
    let view_attr = if body_html.contains("class=\"mdview-diff ") {
        " data-view=\"diff\""
    } else {
        ""
    };
    let diff_layout_attr = if body_html.contains("mdview-diff-split") {
        " data-diff-layout=\"split\""
    } else if body_html.contains("mdview-diff-unified") {
        " data-diff-layout=\"unified\""
    } else {
        ""
    };

    // Emit chrome and syntax CSS for *every* named theme, not just the active
    // one. Applying a theme is then a matter of flipping `data-theme` and the
    // sheets' `media` attributes, which is what lets init.js preview a theme on
    // hover without asking Rust to rebuild and reload the page.
    //
    // The chrome blocks are scoped `:root[data-theme="…"]`, a plain attribute
    // selector rather than CSS Nesting, so they work on the WebKit shipped with
    // macOS 11. Syntect's sheets are full rulesets and cannot be scoped that
    // way at all, hence the `media` toggle.
    let mut chrome_css = String::new();
    let mut named_theme_css = String::new();
    for candidate in Theme::all() {
        let Some(syntect_name) = candidate.syntect_name() else {
            continue;
        };
        let Some((css, bg, fg)) = highlight::palette_for(syntect_name) else {
            continue;
        };
        let is_dark = candidate.is_dark().unwrap_or(false);
        let tokens = chrome::tokens(bg, fg, is_dark);
        chrome_css.push_str(&format!(
            "<style>:root[data-theme=\"{}\"]{{{}}}</style>\n",
            candidate.as_wire(),
            tokens.to_css_vars()
        ));
        let media = if *candidate == theme { "all" } else { "not all" };
        named_theme_css.push_str(&format!(
            "<style id=\"mdview-hl-{}\" media=\"{}\">{}</style>\n",
            candidate.as_wire(),
            media,
            css
        ));
    }

    let (light_media, dark_media) = match theme {
        Theme::System => ("all", "(prefers-color-scheme: dark)"),
        _ => ("not all", "not all"),
    };

    let theme_catalogue = build_theme_catalogue();

    format!(
        r#"<!DOCTYPE html>
<html{theme_attr}{dark_attr}{view_attr}{diff_layout_attr}>
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
<main id="mdview-main">
<div id="mdview-find" role="search" hidden>
<input type="text" id="mdview-find-input" placeholder="Find" aria-label="Find in document" autocomplete="off" autocorrect="off" spellcheck="false">
<span id="mdview-find-count" role="status" aria-live="polite"></span>
</div>
<div id="mdview-comment" hidden>
<input type="text" id="mdview-comment-input" placeholder="Comment" aria-label="Comment on the selection" autocomplete="off" autocorrect="off" spellcheck="false">
<span id="mdview-comment-quote"></span>
</div>
<div id="mdview-content">{body}</div>
</main>
<div id="mdview-sidebar-resizer" role="separator" aria-orientation="vertical" aria-label="Resize sidebar" hidden></div>
<aside id="mdview-sidebar" hidden>
<header class="mdview-sidebar-head">
<h2 id="mdview-sidebar-title">Outline</h2>
</header>
<div id="mdview-sidebar-body" role="tabpanel"></div>
</aside>
</div>
<script nonce="{nonce}">window.mdviewThemes={theme_catalogue};</script>
<script nonce="{nonce}">{katex_js}</script>
<script nonce="{nonce}">{mermaid_js}</script>
<script nonce="{nonce}">{init_js}</script>
</body>
</html>
"#,
        theme_attr = theme_attr,
        dark_attr = dark_attr,
        view_attr = view_attr,
        diff_layout_attr = diff_layout_attr,
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
        theme_catalogue = theme_catalogue,
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

    #[test]
    fn page_emits_the_find_bar_hidden_with_its_field() {
        let html = build_page(&doc(), "<p>hi</p>", Theme::System);
        assert!(html.contains("id=\"mdview-find\" role=\"search\" hidden"));
        assert!(html.contains("id=\"mdview-find-input\""));
        assert!(html.contains("id=\"mdview-find-count\""));
        // Stepping and closing are n, N and esc; the bar has no buttons.
        assert!(!html.contains("id=\"mdview-find-prev\""));
        assert!(!html.contains("id=\"mdview-find-next\""));
        assert!(!html.contains("id=\"mdview-find-close\""));
        // The bar sits outside #mdview-content: a live reload replaces that
        // div wholesale and would take the bar (and the field's focus) with it.
        let bar = html.find("id=\"mdview-find\"").unwrap();
        let content = html.find("id=\"mdview-content\"").unwrap();
        assert!(bar < content);
    }

    #[test]
    fn page_emits_the_comment_bar_hidden_with_its_field() {
        let html = build_page(&doc(), "<p>hi</p>", Theme::System);
        assert!(html.contains("id=\"mdview-comment\" hidden"));
        assert!(html.contains("id=\"mdview-comment-input\""));
        // Same reason as the find bar: a live reload replaces #mdview-content
        // wholesale and would take the bar, and the field's focus, with it.
        let bar = html.find("id=\"mdview-comment\"").unwrap();
        let content = html.find("id=\"mdview-content\"").unwrap();
        assert!(bar < content);
    }

    /// The anchor colours are the one pair a named theme does NOT have to
    /// supply. Under the default System theme no `data-theme` is stamped, so
    /// `chrome::tokens` never runs and an undefined var paints nothing — the
    /// bug that would make every comment invisible out of the box, and the one
    /// `--find-hit-bg` only escapes because `<mark>` has a UA default.
    #[test]
    fn the_comment_anchor_colour_has_a_fallback_for_the_system_theme() {
        let html = build_page(&doc(), "<p>hi</p>", Theme::System);
        // The catalogue of named-theme blocks is always emitted; what System
        // leaves off is the attribute on <html> that would select one.
        let root_tag = &html[..html.find('>').expect("the html tag")];
        assert!(!root_tag.contains("data-theme="), "System stamps no theme");
        let root = html.find(":root {").expect("the base palette");
        let dark = html.find("@media (prefers-color-scheme: dark)").expect("the dark palette");
        let vars = &html[root..dark];
        assert!(vars.contains("--comment-bg:"), "no light fallback for --comment-bg");
        assert!(vars.contains("--comment-fg:"), "no light fallback for --comment-fg");
        assert!(html[dark..].contains("--comment-bg:"), "no dark fallback");
    }

    #[test]
    fn diff_body_marks_page_for_full_width_diff_layout() {
        let html = build_page(&doc(), "<div class=\"mdview-diff mdview-diff-unified\"></div>", Theme::System);
        assert!(html.contains("<html data-view=\"diff\">") || html.contains(" data-view=\"diff\""));
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

        let tags = html.matches("<script").count();
        let nonced = html.matches(&format!("<script nonce=\"{nonce}\">")).count();
        assert!(tags > 0, "the page should bundle scripts at all");
        assert_eq!(
            nonced, tags,
            "every bundled <script> must carry the CSP nonce, or it is dead under the policy"
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
        // The badge is gone; a cursor is the affordance now, so it can be there
        // for a mouse without putting a control on the page.
        assert!(!html.contains(".mdview-zoom-btn"), "the zoom badge should be gone");
        assert!(html.contains("cursor: zoom-in"), "zoomables must still say they are clickable");
        assert!(html.contains("mdview-lightbox"), "lightbox styles/markup missing");
        // JS reached the page
        assert!(html.contains("enhanceZoomables"), "zoom JS missing");
        // Still no remote assets
        assert!(!html.contains("<link"), "no external stylesheets");
        assert!(!html.contains("src=\"http"), "no external scripts");
    }

    /// The point of the whole thing. Every control the page used to draw has a
    /// key and, where a mouse needs one, a menu item; what is left has to stay
    /// left, and one assertion is a cheaper guard than the thirteen markup
    /// tests it replaced.
    ///
    /// The lightbox, the cheat sheet and the theme palette do have buttons, but
    /// JS builds them on demand, so they cannot appear here.
    #[test]
    fn the_emitted_page_contains_no_buttons() {
        for theme in Theme::all() {
            let html = build_page(&doc(), "<p>hi</p>", *theme);
            assert!(
                !html.contains("<button"),
                "{} emits chrome: the page must draw no buttons",
                theme.label()
            );
        }
    }

    /// Find tears its marks down by replacing each with a flat text node,
    /// which deletes whatever was nested inside. So comment anchors have to be
    /// the OUTER wrapper: applied before find re-runs, never after. Get this
    /// backwards and closing the find bar silently strips every anchor it
    /// happened to overlap — invisible until someone loses a comment.
    #[test]
    fn comment_anchors_are_applied_before_the_search_re_runs() {
        let js = assets::INIT_JS;
        let start = js
            .find("function refreshHighlights(")
            .expect("the one funnel for both highlight layers");
        let end = start + js[start..].find("\n  }").expect("end of fn");
        let body = &js[start..end];
        let anchors = body.find("applyCommentAnchors()").expect("anchors are applied");
        let find = body.find("refreshFind()").expect("find is re-run");
        assert!(anchors < find, "find would delete the anchors nested inside it");
        assert!(body.contains("clearFindHighlights()"), "stale find marks would nest");
        // The rail places each card level with its highlight, so it can only
        // run once the highlights exist.
        let rail = body.find("layoutCommentRail()").expect("the rail is laid out");
        assert!(anchors < rail, "the rail would have no anchors to measure");
    }

    #[test]
    fn keyboard_shortcuts_are_embedded_in_the_page() {
        let html = build_page(&doc(), "<p>hi</p>", Theme::System);
        assert!(html.contains("#mdview-shortcuts"), "cheat sheet CSS missing");
        assert!(html.contains("mdviewToggleShortcuts"), "cheat sheet hook missing");
        assert!(html.contains("onDocumentKeyDown"), "key dispatcher missing");
    }

    /// Every binding lives in init.js's one SHORTCUTS table, which feeds both
    /// the dispatcher and the `?` sheet. Losing an entry silently drops the
    /// key AND its documentation, with nothing else to notice.
    #[test]
    fn every_advertised_single_key_binding_is_still_bound() {
        for key in [
            "\"j\"", "\"k\"", "\"d\"", "\"u\"", "\" \"", "\"Shift+Space\"", "\"G\"",
            "\"]\"", "\"[\"", "\"}\"", "\"{\"",
            "\"/\"", "\"n\"", "\"N\"",
            "\"s\"", "\"o\"", "\"b\"", "\"m\"", "\"t\"",
            "\"D\"", "\"l\"", "\"z\"", "\"w\"", "\"r\"", "\"+\"", "\"=\"", "\"-\"", "\"0\"", "\"?\"",
            "\"c\"", "\"e\"", "\"x\"", "\"C\"",
        ] {
            let needle = format!("keys: [{}", key);
            let alt = format!(", {}]", key);
            assert!(
                assets::INIT_JS.contains(&needle) || assets::INIT_JS.contains(&alt),
                "no binding for {key} in the SHORTCUTS table"
            );
        }
        // "gg" is a chord, so it is dispatched by hand rather than from the
        // table; it still has to be documented there.
        assert!(assets::INIT_JS.contains("hint: \"g g\""));
        // b opens the bookmarks list, so it must not also be paging: the two
        // would fight and only whichever the table lists last would run.
        assert!(
            !assets::INIT_JS.contains("keys: [\"f\""),
            "f is unbound; paging is space and ⇧space"
        );
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
    fn the_catalogue_lists_every_theme_by_label() {
        let html = build_page(&doc(), "", Theme::System);
        for theme in Theme::all() {
            assert!(
                html.contains(&format!("id:\"{}\"", theme.as_wire())),
                "catalogue missing {}",
                theme.label()
            );
            assert!(
                html.contains(&format!("label:\"{}\"", theme.label())),
                "catalogue missing label {}",
                theme.label()
            );
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
    fn the_sidebar_names_the_panel_it_is_showing() {
        let html = build_page(&doc(), "<p>hi</p>", Theme::System);
        // The tabs are gone, so this heading is the only thing distinguishing
        // the outline from the bookmarks. init.js rewrites its text.
        assert!(html.contains("id=\"mdview-sidebar-title\""));
        assert!(html.contains("id=\"mdview-sidebar-body\""));
        assert!(!html.contains("class=\"mdview-tab\""));
    }

    #[test]
    fn fullwidth_state_removes_only_the_content_width_cap() {
        let css = assets::PAGE_CSS;
        let content_start = css.find("#mdview-content {").expect("content rule");
        let content = &css[content_start
            ..content_start + css[content_start..].find('}').expect("content rule close")];
        assert!(
            content.contains("max-width: 46rem"),
            "centered view must remain the default"
        );
        assert!(
            content.contains("padding: 3rem 1.5rem 6rem"),
            "both width modes must retain the current page padding"
        );
        let full_start = css
            .find(":root[data-fullwidth=\"1\"] #mdview-content {")
            .expect("fullwidth override");
        let full =
            &css[full_start..full_start + css[full_start..].find('}').expect("override close")];
        assert!(
            full.contains("max-width: none"),
            "fullwidth must remove the cap"
        );
        assert!(
            !full.contains("padding:"),
            "fullwidth must inherit the base padding"
        );
    }

    #[test]
    fn every_theme_ships_its_own_chrome_and_highlight_sheet() {
        // A hover preview is only an attribute flip if the CSS for the theme
        // being previewed is already on the page.
        let html = build_page(&doc(), "<p>hi</p>", Theme::Mocha);
        for theme in Theme::all() {
            let Some(wire) = theme.syntect_name().map(|_| theme.as_wire()) else {
                continue;
            };
            assert!(
                html.contains(&format!(":root[data-theme=\"{wire}\"]")),
                "{wire} must ship a scoped chrome block"
            );
            assert!(
                html.contains(&format!("id=\"mdview-hl-{wire}\"")),
                "{wire} must ship its own highlight sheet"
            );
            assert!(
                html.contains(&format!("id:\"{wire}\"")),
                "{wire} must reach the palette's catalogue"
            );
        }
        // Only the active theme's sheet is live; the rest wait behind `not all`.
        assert!(html.contains("id=\"mdview-hl-mocha\" media=\"all\""));
        assert!(html.contains("id=\"mdview-hl-github\" media=\"not all\""));
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
