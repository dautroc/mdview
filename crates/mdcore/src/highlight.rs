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

/// The scope stacks a real Markdown highlight would produce, and the code
/// scopes to fall back on when a palette says nothing about prose.
///
/// The stacks are not decoration. The eight palettes MDView ships disagree
/// about how a heading is even expressed: Solarized, Monokai Pro and
/// Chiroptera write a plain `markup.heading` rule; the base16 family writes
/// `markup.heading punctuation.definition.heading, entity.name.section`,
/// which a bare `markup.heading` probe does not match; and InspiredGitHub
/// writes `text.html.markdown markup.heading` with a font weight and no
/// colour at all. Handing syntect the stack its own Markdown syntax would
/// push lets its matcher resolve all three, and reports `None` for the third.
type Chain = &'static [&'static [&'static str]];

const HEADING: Chain = &[
    // `markup.heading` first and alone. Putting `entity.name.section` on the
    // same stack loses: it matches the top of the stack and so outscores
    // `markup.heading` further down it, which took Chiroptera's headings from
    // its Title colour to the one it gives labels.
    &["text.html.markdown", "markup.heading.1.markdown"],
    // The base16 family colours nothing but the `#` marks under
    // `markup.heading`; `entity.name.section` is where it puts the text.
    &["entity.name.section"],
    &["entity.name.function"],
    &["keyword"],
];
const LINK: Chain = &[
    &["text.html.markdown", "meta.link.inline.markdown", "markup.underline.link.markdown"],
    &["string.other.link"],
    &["keyword"],
];
const RAW: Chain = &[
    &["text.html.markdown", "markup.raw.inline.markdown"],
    &["string"],
];
const QUOTE: Chain = &[
    &["text.html.markdown", "markup.quote.markdown"],
    &["comment"],
];
// Weight and slant already carry these; a palette that does not colour them
// leaves them at the page foreground rather than borrowing a code hue.
const BOLD: Chain = &[&["text.html.markdown", "markup.bold.markdown"]];
const ITALIC: Chain = &[&["text.html.markdown", "markup.italic.markdown"]];

/// The first stack in `chain` the theme actually colours, skipping any whose
/// colour is the page foreground: `InspiredGitHub` paints `markup.raw.inline`
/// in its own `#323232` text colour, which is a derivation that derives
/// nothing. `foreground: None` from syntect means no rule matched at all.
fn first_colour(
    highlighter: &syntect::highlighting::Highlighter<'_>,
    fg: Option<syntect::highlighting::Color>,
    chain: Chain,
) -> Option<crate::chrome::Rgb> {
    for stack in chain {
        let scopes: Vec<syntect::parsing::Scope> =
            stack.iter().filter_map(|s| syntect::parsing::Scope::new(s).ok()).collect();
        if scopes.len() != stack.len() {
            continue;
        }
        let Some(colour) = highlighter.style_mod_for_stack(&scopes).foreground else {
            continue;
        };
        if fg.is_some_and(|f| (f.r, f.g, f.b) == (colour.r, colour.g, colour.b)) {
            continue;
        }
        return Some(crate::chrome::Rgb { r: colour.r, g: colour.g, b: colour.b });
    }
    None
}

/// What one palette says about Markdown's own constructs, so the document can
/// be painted from the same source as the code inside it.
///
/// Deliberately a sibling of `palette_for` rather than a widening of it: that
/// function has three callers who want the CSS and the two page colours, and
/// nothing else.
pub fn markup_palette_for(name: &str) -> Option<crate::chrome::MarkupPalette> {
    let theme = theme_named(name)?;
    // syntect's own `Highlighter`. The bare name in this module is the local
    // struct above, which wraps a SyntaxSet and is a different thing.
    let highlighter = syntect::highlighting::Highlighter::new(theme);
    let fg = theme.settings.foreground;
    let probe = |chain| first_colour(&highlighter, fg, chain);
    Some(crate::chrome::MarkupPalette {
        heading: probe(HEADING),
        link: probe(LINK),
        raw: probe(RAW),
        quote: probe(QUOTE),
        bold: probe(BOLD),
        italic: probe(ITALIC),
    })
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

    /// A palette that says nothing about a role must still hand back a
    /// colour, or the document loses that element on that theme.
    #[test]
    fn every_named_palette_colours_every_document_role() {
        use crate::theme::Theme;

        let mut checked = 0;
        for theme in Theme::all().iter().filter(|t| **t != Theme::System) {
            let name = theme.syntect_name().expect("named theme has a palette");
            let markup = markup_palette_for(name).expect("named theme has a markup palette");
            let (_, _, fg) = palette_for(name).expect("named theme has a palette");
            for (role, hue) in [
                ("heading", markup.heading),
                ("link", markup.link),
                ("raw", markup.raw),
                ("quote", markup.quote),
            ] {
                let hue = hue
                    .unwrap_or_else(|| panic!("{} has no {role} colour", theme.label()));
                // A hue equal to the page's own text is a derivation that
                // derived nothing; the chain is supposed to keep looking.
                assert_ne!(hue, fg, "{}: {role} is just the foreground", theme.label());
            }
            checked += 1;
        }
        assert_eq!(
            checked,
            Theme::all().len() - 1,
            "a theme was added without a markup palette"
        );
    }

    /// InspiredGitHub styles Markdown structurally -- `markup.heading` carries
    /// a font weight and no colour -- so the chain has to fall through to a
    /// code scope. Pinned because the naive single-scope probe silently
    /// returned the page foreground here.
    #[test]
    fn a_palette_that_only_styles_markdown_structurally_falls_back() {
        let markup = markup_palette_for("InspiredGitHub").expect("built-in palette");
        assert_eq!(markup.heading, Some(crate::chrome::Rgb { r: 0x79, g: 0x5d, b: 0xa3 }));
        assert_eq!(markup.quote, Some(crate::chrome::Rgb { r: 0x96, g: 0x98, b: 0x96 }));
        // Its markup.raw.inline is its own #323232 text colour: nothing said.
        assert_eq!(markup.raw, Some(crate::chrome::Rgb { r: 0x18, g: 0x36, b: 0x91 }));
        // Nothing colours bold or italic; weight and slant carry them.
        assert_eq!(markup.bold, None);
        assert_eq!(markup.italic, None);
    }

    /// The base16 family colours only the `#` marks under `markup.heading`
    /// and puts the heading text under `entity.name.section`. A bare
    /// `markup.heading` probe finds neither.
    #[test]
    fn a_heading_colour_hiding_under_entity_name_section_is_found() {
        let markup = markup_palette_for("base16-eighties.dark").expect("built-in palette");
        assert_eq!(markup.heading, Some(crate::chrome::Rgb { r: 0x66, g: 0x99, b: 0xcc }));
    }

    /// The reverse: a palette with a plain `markup.heading` must use it, not
    /// whatever it happens to give `entity.name.section`. Chiroptera gives
    /// them different colours, which is what makes it the case to pin.
    #[test]
    fn a_plain_markup_heading_wins_over_entity_name_section() {
        let markup = markup_palette_for("Chiroptera Dark Hard").expect("bundled palette");
        // Title, #79caaf -- not the #61b197 it gives labels and sections.
        assert_eq!(markup.heading, Some(crate::chrome::Rgb { r: 0x79, g: 0xca, b: 0xaf }));
        assert_eq!(markup.link, Some(crate::chrome::Rgb { r: 0x85, g: 0xc6, b: 0xc9 }));
        assert_eq!(markup.raw, Some(crate::chrome::Rgb { r: 0xa9, g: 0xa7, b: 0x2d }));
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
