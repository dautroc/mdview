use syntect::highlighting::ThemeSet;
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
}

/// CSS for the highlight classes, as `(light, dark)`. Both are emitted into
/// every page; a `prefers-color-scheme` media query picks one at display time.
pub fn theme_css() -> (String, String) {
    let themes = ThemeSet::load_defaults();
    let light = css_for_theme_with_class_style(&themes.themes[LIGHT_THEME], CLASS_STYLE)
        .expect("bundled light theme must produce css");
    let dark = css_for_theme_with_class_style(&themes.themes[DARK_THEME], CLASS_STYLE)
        .expect("bundled dark theme must produce css");
    (light, dark)
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
}
