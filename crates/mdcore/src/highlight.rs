use std::io::Cursor;
use std::sync::OnceLock;

use syntect::highlighting::{Theme, ThemeSet};
use syntect::html::{css_for_theme_with_class_style, ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use crate::escape::{escape_attr, escape_html};

const CLASS_STYLE: ClassStyle = ClassStyle::Spaced;
const LIGHT_THEME: &str = "InspiredGitHub";
const DARK_THEME: &str = "base16-ocean.dark";

/// Turns fenced code blocks into HTML. Loading the syntax set is expensive
/// (tens of milliseconds), so build one `Highlighter` and reuse it.
pub struct Highlighter {
    syntaxes: SyntaxSet,
}

impl Default for Highlighter {
    fn default() -> Self {
        Self::new()
    }
}

impl Highlighter {
    pub fn new() -> Self {
        Self {
            syntaxes: SyntaxSet::load_defaults_newlines(),
        }
    }

    /// Render one fenced block. `lang` is the fence info string; `code` is the
    /// raw body. Never fails: unknown languages degrade to plain escaped text.
    pub fn render_block(&self, lang: &str, code: &str) -> String {
        // `` ```rust,ignore `` and `` ```rust {highlight} `` both mean "rust".
        let token = lang
            .split(|c: char| c.is_whitespace() || c == ',')
            .next()
            .unwrap_or("");

        // Mermaid is not a language to highlight; it is a diagram for the
        // bundled JS to transform after load.
        if token.eq_ignore_ascii_case("mermaid") {
            return format!("<pre class=\"mermaid\">{}</pre>", escape_html(code));
        }

        match self.syntaxes.find_syntax_by_token(token) {
            Some(syntax) => {
                let mut generator =
                    ClassedHTMLGenerator::new_with_class_style(syntax, &self.syntaxes, CLASS_STYLE);
                for line in LinesWithEndings::from(code) {
                    // A malformed line is not worth failing a render over.
                    let _ = generator.parse_html_for_line_which_includes_newline(line);
                }
                format!(
                    "<pre class=\"code\"><code class=\"lang-{}\">{}</code></pre>",
                    escape_attr(token),
                    generator.finalize()
                )
            }
            None => format!(
                "<pre class=\"code\"><code>{}</code></pre>",
                escape_html(code)
            ),
        }
    }

    /// Highlight a complete Markdown source and return independently
    /// renderable fragments for each source line. The syntax parser runs over
    /// the complete document first, then span tags are balanced at line
    /// boundaries so a diff row can safely move one line without corrupting
    /// the surrounding HTML.
    pub fn render_markdown_lines(&self, source: &str) -> Vec<String> {
        if source.is_empty() {
            return Vec::new();
        }
        let Some(syntax) = self.syntaxes.find_syntax_by_name("Markdown") else {
            return source.lines().map(crate::escape::escape_html).collect();
        };
        let mut generator =
            ClassedHTMLGenerator::new_with_class_style(syntax, &self.syntaxes, CLASS_STYLE);
        for line in LinesWithEndings::from(source) {
            let _ = generator.parse_html_for_line_which_includes_newline(line);
        }
        let mut lines = split_balanced_lines(&generator.finalize());
        lines.truncate(source.lines().count());
        lines
    }
}

fn split_balanced_lines(html: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut open_tags: Vec<String> = Vec::new();
    let mut cursor = 0;
    while cursor < html.len() {
        let rest = &html[cursor..];
        if rest.starts_with('\n') {
            append_closures(&mut current, &open_tags);
            lines.push(current);
            current = String::new();
            for tag in &open_tags {
                current.push_str(tag);
            }
            cursor += 1;
            continue;
        }
        if rest.starts_with("<span ") {
            if let Some(end) = rest.find('>') {
                let tag = &rest[..=end];
                open_tags.push(tag.to_string());
                current.push_str(tag);
                cursor += end + 1;
                continue;
            }
        }
        if rest.starts_with("</span>") {
            current.push_str("</span>");
            open_tags.pop();
            cursor += "</span>".len();
            continue;
        }
        let ch = rest.chars().next().expect("cursor is within html");
        current.push(ch);
        cursor += ch.len_utf8();
    }
    if !current.is_empty() || (lines.is_empty() && !html.is_empty()) {
        append_closures(&mut current, &open_tags);
        lines.push(current);
    }
    lines
}

fn append_closures(target: &mut String, open_tags: &[String]) {
    for _ in open_tags.iter().rev() {
        target.push_str("</span>");
    }
}

/// syntect's built-in palettes. Parsing them costs a few milliseconds and
/// `build_page` needs one per theme, so the set is parsed once per process.
fn default_themes() -> &'static ThemeSet {
    static DEFAULTS: OnceLock<ThemeSet> = OnceLock::new();
    DEFAULTS.get_or_init(ThemeSet::load_defaults)
}

/// Palettes MDView ships itself, for themes syntect has no built-in for.
/// A malformed asset yields an empty set rather than a panic; the tests below
/// are what turn a broken tmTheme into a red build instead of an unstyled
/// page at runtime.
fn bundled_themes() -> &'static ThemeSet {
    static BUNDLED: OnceLock<ThemeSet> = OnceLock::new();
    BUNDLED.get_or_init(|| {
        let mut set = ThemeSet::new();
        for source in [
            crate::assets::MONOKAI_PRO_THEME,
            crate::assets::CHIROPTERA_DARK_HARD_THEME,
        ] {
            if let Ok(theme) = ThemeSet::load_from_reader(&mut Cursor::new(source)) {
                // Keyed by the name the file declares, so an asset whose name
                // drifts from `Theme::syntect_name` fails to resolve loudly.
                set.themes.insert(theme.name.clone().unwrap_or_default(), theme);
            }
        }
        set
    })
}

/// Resolve a palette by name: syntect's defaults first, then MDView's own.
fn theme_named(name: &str) -> Option<&'static Theme> {
    default_themes()
        .themes
        .get(name)
        .or_else(|| bundled_themes().themes.get(name))
}

/// CSS for the highlight classes, as `(light, dark)`. Both are emitted into
/// every page; a `prefers-color-scheme` media query picks one at display time.
pub fn theme_css() -> (String, String) {
    let themes = default_themes();
    let light = css_for_theme_with_class_style(&themes.themes[LIGHT_THEME], CLASS_STYLE)
        .expect("bundled light theme must produce css");
    let dark = css_for_theme_with_class_style(&themes.themes[DARK_THEME], CLASS_STYLE)
        .expect("bundled dark theme must produce css");
    (light, dark)
}

/// Class-based CSS for one syntect theme, plus its background and foreground
/// so the page chrome can be derived from the same source.
pub fn palette_for(name: &str) -> Option<(String, crate::chrome::Rgb, crate::chrome::Rgb)> {
    let theme = theme_named(name)?;
    let css = css_for_theme_with_class_style(theme, CLASS_STYLE).ok()?;
    let to_rgb = |c: syntect::highlighting::Color| crate::chrome::Rgb { r: c.r, g: c.g, b: c.b };
    Some((
        css,
        to_rgb(theme.settings.background?),
        to_rgb(theme.settings.foreground?),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_language_produces_class_markup() {
        let hl = Highlighter::new();
        let html = hl.render_block("rust", "fn main() {}\n");
        assert!(html.contains("<pre class=\"code\""), "got: {html}");
        assert!(html.contains("class=\"lang-rust\""), "got: {html}");
        // syntect class-style output uses `class="source rust"`-ish spans
        assert!(html.contains("<span class="), "expected highlight spans: {html}");
    }

    #[test]
    fn unknown_language_falls_back_to_plain_escaped_code() {
        let hl = Highlighter::new();
        let html = hl.render_block("wubbalubba", "a < b & c\n");
        assert!(html.contains("a &lt; b &amp; c"), "got: {html}");
        assert!(!html.contains("<span class="), "must not highlight: {html}");
    }

    #[test]
    fn absent_language_falls_back_to_plain_escaped_code() {
        let hl = Highlighter::new();
        let html = hl.render_block("", "<script>alert(1)</script>\n");
        assert!(html.contains("&lt;script&gt;"), "got: {html}");
        assert!(!html.contains("<script>"), "raw script must never survive: {html}");
    }

    #[test]
    fn mermaid_fence_becomes_a_mermaid_pre_with_escaped_body() {
        let hl = Highlighter::new();
        let html = hl.render_block("mermaid", "graph TD;\n  A --> B;\n");
        assert!(html.starts_with("<pre class=\"mermaid\">"), "got: {html}");
        assert!(html.contains("A --&gt; B"), "body must be escaped: {html}");
        assert!(!html.contains("<span class="), "mermaid is not highlighted: {html}");
    }

    #[test]
    fn mermaid_match_is_case_insensitive() {
        let hl = Highlighter::new();
        assert!(hl.render_block("Mermaid", "graph TD;\n").starts_with("<pre class=\"mermaid\">"));
    }

    #[test]
    fn info_string_with_attributes_uses_only_the_first_token() {
        let hl = Highlighter::new();
        let html = hl.render_block("rust,ignore", "fn main() {}\n");
        assert!(html.contains("class=\"lang-rust\""), "got: {html}");
    }

    #[test]
    fn theme_css_returns_two_distinct_stylesheets() {
        let (light, dark) = theme_css();
        assert!(!light.is_empty() && !dark.is_empty());
        assert_ne!(light, dark);
    }

    #[test]
    fn markdown_lines_keep_multiline_spans_balanced() {
        let hl = Highlighter::new();
        let lines = hl.render_markdown_lines("# Title\n\n**bold**\n");
        assert_eq!(lines.len(), 3);
        for line in &lines {
            assert_eq!(line.matches("<span ").count(), line.matches("</span>").count());
        }
        assert!(lines[0].contains("Title"));
        assert!(lines[2].contains("bold"));
    }

    #[test]
    fn empty_markdown_has_no_diff_lines() {
        assert!(Highlighter::new().render_markdown_lines("").is_empty());
    }

    #[test]
    fn the_bundled_tmthemes_parse_and_expose_a_palette() {
        // Without this, a malformed or renamed asset would ship as an
        // unstyled page at runtime with the whole suite still green: the
        // bundled set swallows a parse failure by staying empty.
        let bundled = bundled_themes();
        let expected = [
            ("Monokai Pro", crate::chrome::Rgb { r: 0x2d, g: 0x2a, b: 0x2e },
                crate::chrome::Rgb { r: 0xfc, g: 0xfc, b: 0xfa }),
            ("Chiroptera Dark Hard", crate::chrome::Rgb { r: 0x2d, g: 0x2d, b: 0x2e },
                crate::chrome::Rgb { r: 0xa4, g: 0xa2, b: 0x9b }),
        ];
        assert_eq!(
            bundled.themes.len(),
            expected.len(),
            "a bundled tmTheme failed to parse; got {:?}",
            bundled.themes.keys().collect::<Vec<_>>()
        );

        for (name, want_bg, want_fg) in expected {
            assert!(
                bundled.themes.contains_key(name),
                "bundled tmTheme failed to parse or declares a different name; got {:?}",
                bundled.themes.keys().collect::<Vec<_>>()
            );
            let (css, bg, fg) = palette_for(name).expect("bundled palette must resolve");
            assert_eq!(bg, want_bg, "{name} background");
            assert_eq!(fg, want_fg, "{name} foreground");
            // A tmTheme with only bg/fg would satisfy the checks above while
            // leaving code blocks background-swapped and otherwise uncoloured.
            assert!(css.contains(".comment"), "{name} comments must be coloured: {css}");
            assert!(css.contains(".string"), "{name} strings must be coloured: {css}");
            assert!(css.contains(".keyword"), "{name} keywords must be coloured: {css}");
        }
    }

    #[test]
    fn palette_for_still_resolves_a_syntect_builtin() {
        // The bundled set is a fallback, not a replacement -- the defaults
        // path has to keep working after it was introduced.
        let (css, bg, fg) = palette_for("base16-mocha.dark").expect("built-in must still resolve");
        assert!(!css.is_empty());
        assert_ne!(bg, fg);
    }

    #[test]
    fn an_unknown_palette_name_resolves_through_neither_set() {
        assert!(palette_for("Tokyo Night").is_none());
    }

    #[test]
    fn every_named_theme_yields_a_usable_palette() {
        // The resolves-to-a-real-theme test only checks the name exists. This
        // checks the theme actually carries the background and foreground the
        // chrome derivation needs — a bundled theme missing either would make
        // palette_for return None indistinguishably from "no such name".
        for theme in crate::theme::Theme::all() {
            if let Some(name) = theme.syntect_name() {
                let got = palette_for(name);
                assert!(got.is_some(), "{name} has no usable palette");
                let (css, bg, fg) = got.unwrap();
                assert!(!css.is_empty(), "{name} produced empty CSS");
                assert_ne!(bg, fg, "{name} background and foreground are identical");
            }
        }
    }
}
