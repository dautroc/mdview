use pulldown_cmark::{html, CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd};

use crate::highlight::Highlighter;

/// The parser feature set. Kept in one place because every transform in this
/// crate assumes exactly these extensions are on.
pub fn markdown_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_MATH
}

/// Render Markdown to the inner HTML of `<body>`. No page chrome, no assets.
pub fn render_body(markdown: &str) -> String {
    let highlighter = Highlighter::new();
    render_body_with(markdown, &highlighter)
}

/// Same as `render_body`, but reuses a `Highlighter`. Live reload calls this
/// on every save, and rebuilding the syntax set each time would be wasteful.
pub fn render_body_with(markdown: &str, highlighter: &Highlighter) -> String {
    let parser = Parser::new_ext(markdown, markdown_options());
    let events = highlight_code_blocks(parser, highlighter);
    let mut out = String::new();
    html::push_html(&mut out, events.into_iter());
    out
}

/// Collapse each fenced/indented code block into a single pre-rendered
/// `Event::Html`. Done during the event stream rather than by post-processing
/// the HTML string, because regex over generated HTML mangles code contents.
fn highlight_code_blocks<'a>(
    events: impl Iterator<Item = Event<'a>>,
    highlighter: &Highlighter,
) -> Vec<Event<'a>> {
    let mut out = Vec::new();
    // Some((language, accumulated_code)) while inside a code block.
    let mut current: Option<(String, String)> = None;

    for event in events {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                let lang = match &kind {
                    CodeBlockKind::Fenced(info) => info.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                current = Some((lang, String::new()));
            }
            Event::End(TagEnd::CodeBlock) => {
                // `current` is always Some here: pulldown-cmark guarantees
                // balanced Start/End pairs, including for unterminated fences.
                if let Some((lang, code)) = current.take() {
                    out.push(Event::Html(CowStr::from(
                        highlighter.render_block(&lang, &code),
                    )));
                }
            }
            Event::Text(text) if current.is_some() => {
                current
                    .as_mut()
                    .expect("checked by guard")
                    .1
                    .push_str(&text);
            }
            other => out.push(other),
        }
    }

    out
}
