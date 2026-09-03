use pulldown_cmark::{html, CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd};

use crate::highlight::Highlighter;
use crate::math::math_event_html;

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
    render_body_in(markdown, highlighter, None)
}

/// Same again, resolving the document's own images against `base_dir` and
/// embedding them. A page handed to WKWebView as a string cannot read `file:`
/// subresources, so without this a document's pictures silently do not appear.
/// Pass `None` when there is no directory to resolve against; destinations are
/// then left exactly as written.
pub fn render_body_in(
    markdown: &str,
    highlighter: &Highlighter,
    base_dir: Option<&std::path::Path>,
) -> String {
    // Frontmatter comes off before the parser sees it. Doing it here rather
    // than in `Document` keeps it out of the diff view, which shows the file
    // as it is on disk and would be wrong to hide a metadata edit from.
    let markdown = crate::frontmatter::strip(markdown);
    let parser = Parser::new_ext(markdown, markdown_options());
    let events = transform_events(parser, highlighter);
    let events = inline_images(events, base_dir);
    let mut out = String::new();
    html::push_html(&mut out, events.into_iter());
    out
}

/// Replace each image destination with a `data:` URI where one can be built.
fn inline_images<'a>(events: Vec<Event<'a>>, base_dir: Option<&std::path::Path>) -> Vec<Event<'a>> {
    let Some(base_dir) = base_dir else {
        return events;
    };
    events
        .into_iter()
        .map(|event| match event {
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let dest_url = match crate::images::inline(&dest_url, base_dir) {
                    Some(data) => CowStr::from(data),
                    None => dest_url,
                };
                Event::Start(Tag::Image {
                    link_type,
                    dest_url,
                    title,
                    id,
                })
            }
            other => other,
        })
        .collect()
}

/// One top-level block of a document: the Markdown it came from, and the HTML
/// it rendered to. `range` is a byte range into the document *after*
/// frontmatter is stripped, which is the only text the parser ever sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub html: String,
    pub source: String,
    pub range: std::ops::Range<usize>,
}

/// Separates one block's HTML from the next inside a single `push_html` pass.
/// U+0001 cannot reach here from a document -- the parser would hand it back as
/// text and the writer would escape it -- and `render_blocks` counts the pieces
/// afterwards in case it ever does.
const BLOCK_SENTINEL: &str = "\u{1}mdview-block\u{1}";

/// The document's top-level blocks, each with its own HTML.
///
/// The rendered diff needs a handle on one block at a time, which the whole-body
/// renderer does not give it. Two things here are deliberate, and both are about
/// rendering the *same* document the normal view renders.
///
/// The document is parsed once. A block re-parsed from its own source slice
/// would lose every link reference definition and footnote defined elsewhere in
/// the file, and would quietly render `[spec]` as literal text.
///
/// The HTML is pushed once too, with a sentinel between the groups, because
/// `push_html` restarts its footnote numbering on every call: a document with
/// footnotes in two paragraphs would come back numbered `1` and `1`.
pub fn render_blocks(
    markdown: &str,
    highlighter: &Highlighter,
    base_dir: Option<&std::path::Path>,
) -> Vec<Block> {
    let markdown = crate::frontmatter::strip(markdown);
    let mut ranges = Vec::new();
    let mut groups = Vec::new();
    for (range, events) in group_events(markdown) {
        groups.push(inline_images(
            transform_events(events.into_iter(), highlighter),
            base_dir,
        ));
        ranges.push(range);
    }
    if groups.is_empty() {
        return Vec::new();
    }

    let mut combined: Vec<Event> = Vec::new();
    for (index, events) in groups.iter().enumerate() {
        if index > 0 {
            combined.push(Event::Html(CowStr::from(BLOCK_SENTINEL)));
        }
        combined.extend(events.iter().cloned());
    }
    let mut html = String::new();
    html::push_html(&mut html, combined.into_iter());
    let parts = html.split(BLOCK_SENTINEL).map(str::trim).collect::<Vec<_>>();

    if parts.len() == groups.len() {
        return parts
            .into_iter()
            .zip(ranges)
            .map(|(html, range)| Block {
                source: markdown[range.clone()].to_string(),
                html: html.to_string(),
                range,
            })
            .collect();
    }
    // The sentinel came back cut or doubled. Rendering each block on its own
    // gets the footnote numbers wrong, which is a smaller lie than a document
    // sliced in the wrong places.
    groups
        .into_iter()
        .zip(ranges)
        .map(|(events, range)| {
            let mut html = String::new();
            html::push_html(&mut html, events.into_iter());
            Block {
                source: markdown[range.clone()].to_string(),
                html: html.trim().to_string(),
                range,
            }
        })
        .collect()
}

/// Group a document's events by top-level block. A `Start` event's range spans
/// the whole block it opens; a block that is a single event -- a thematic break,
/// an HTML block -- carries its own.
fn group_events(markdown: &str) -> Vec<(std::ops::Range<usize>, Vec<Event<'_>>)> {
    let mut groups = Vec::new();
    let mut current: Option<(std::ops::Range<usize>, Vec<Event>)> = None;
    let mut depth = 0usize;
    for (event, range) in Parser::new_ext(markdown, markdown_options()).into_offset_iter() {
        if matches!(event, Event::Start(_)) {
            if depth == 0 {
                current = Some((range, Vec::new()));
            }
            depth += 1;
        } else if matches!(event, Event::End(_)) {
            depth = depth.saturating_sub(1);
        } else if depth == 0 {
            groups.push((range, vec![event]));
            continue;
        }
        if let Some((_, events)) = current.as_mut() {
            events.push(event);
        }
        if depth == 0 {
            if let Some(group) = current.take() {
                groups.push(group);
            }
        }
    }
    groups
}

/// Collapse each fenced/indented code block into a single pre-rendered
/// `Event::Html` and convert math events to KaTeX-ready markup. Done during
/// the event stream rather than by post-processing the HTML string, because
/// regex over generated HTML mangles code contents.
fn transform_events<'a>(
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
            Event::InlineMath(tex) => {
                out.push(Event::Html(CowStr::from(math_event_html(&tex, false))));
            }
            Event::DisplayMath(tex) => {
                out.push(Event::Html(CowStr::from(math_event_html(&tex, true))));
            }
            other => out.push(other),
        }
    }

    out
}

/// The document's headings, in document order, as plain text.
///
/// Used for the decorative labels in a review file. Headings written as raw
/// HTML are not `Tag::Heading` events, so they render as elements but do not
/// appear here; the label is cosmetic, and the anchor a comment actually uses
/// is its quote, so a shifted label costs nothing.
pub fn headings(markdown: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut depth = 0usize;
    for event in Parser::new_ext(crate::frontmatter::strip(markdown), markdown_options()) {
        match event {
            Event::Start(Tag::Heading { .. }) => {
                depth += 1;
                out.push(String::new());
            }
            Event::End(TagEnd::Heading(_)) => depth = depth.saturating_sub(1),
            Event::Text(text) | Event::Code(text) if depth > 0 => {
                if let Some(last) = out.last_mut() {
                    last.push_str(&text);
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod heading_tests {
    use super::headings;

    /// The labels in a review file. Inline markup inside a heading is part of
    /// its text rather than a separate event, so it has to be collected: a
    /// heading of "The `retry` loop" coming back as "The loop" would label the
    /// wrong section.
    #[test]
    fn headings_are_collected_in_document_order_with_their_inline_text() {
        let markdown = "# One\n\ntext\n\n## The `retry` loop\n\n### **Bold** and _italic_\n";
        assert_eq!(headings(markdown), vec!["One", "The retry loop", "Bold and italic"]);
    }

    #[test]
    fn a_document_with_no_headings_has_no_labels() {
        assert!(headings("just a paragraph\n").is_empty());
    }

    /// A review's labels come from this list, and frontmatter parses as a
    /// setext heading -- so without the strip, every comment on an Obsidian
    /// note would be filed under a section called `title: My Note`.
    #[test]
    fn frontmatter_is_not_a_heading() {
        let markdown = "---\ntitle: My Note\n---\n\n# One\n\n## Two\n";
        assert_eq!(headings(markdown), vec!["One", "Two"]);
    }
}

#[cfg(test)]
mod frontmatter_tests {
    use super::render_body;

    /// The bug this exists for. `---`, metadata, `---` is a thematic break
    /// followed by a paragraph that the closing fence promotes to an `<h2>`,
    /// which then outranks the document's own `<h1>` in the outline.
    #[test]
    fn frontmatter_renders_as_nothing_at_all() {
        let html = render_body("---\ntitle: My Note\ntags: [a, b]\n---\n\n# Hello\n\nBody.\n");
        assert!(!html.contains("<hr"), "the opening fence is still a rule: {html}");
        assert!(!html.contains("My Note"), "the metadata is still in the body: {html}");
        assert!(html.starts_with("<h1>Hello</h1>"), "got: {html}");
    }

    /// The other half of the contract: a document is allowed to open on a
    /// rule, and nothing about that looks like metadata once you require a
    /// closing fence.
    #[test]
    fn a_leading_thematic_break_still_renders() {
        let html = render_body("---\n\n# Hello\n");
        assert!(html.contains("<hr"), "got: {html}");
    }
}

#[cfg(test)]
mod block_tests {
    use super::{render_blocks, Block};
    use crate::highlight::Highlighter;

    fn blocks(markdown: &str) -> Vec<Block> {
        render_blocks(markdown, &Highlighter::new(), None)
    }

    /// A block's source is what the diff pairs on, so it has to be the whole
    /// construct. A fence whose source stopped at the opening line would pair
    /// two different code blocks as the same one.
    #[test]
    fn a_block_spans_its_whole_source_including_a_fenced_code_block() {
        let blocks = blocks("# Title\n\ntext\n\n```rust\nfn main() {}\n```\n\nafter\n");
        assert_eq!(blocks.len(), 4, "got: {blocks:#?}");
        assert_eq!(blocks[0].source.trim(), "# Title");
        assert_eq!(blocks[2].source.trim(), "```rust\nfn main() {}\n```");
        assert!(blocks[2].html.starts_with("<pre"), "got: {}", blocks[2].html);
    }

    /// Neither is a `Start`/`End` pair, so a grouper that only closed on `End`
    /// would swallow both into whatever block came next.
    #[test]
    fn a_raw_html_block_and_a_thematic_break_are_each_their_own_block() {
        let blocks = blocks("<div class=\"x\">raw</div>\n\n---\n\npara\n");
        assert_eq!(blocks.len(), 3, "got: {blocks:#?}");
        assert!(blocks[0].html.contains("<div class=\"x\">"), "got: {}", blocks[0].html);
        assert!(blocks[1].html.contains("<hr"), "got: {}", blocks[1].html);
    }

    /// Definitions render as nothing, and the paragraph that uses one renders
    /// as a link only because the whole document was parsed together.
    #[test]
    fn a_link_reference_definition_produces_no_block_of_its_own() {
        let blocks = blocks("See [spec].\n\n[spec]: https://example.com\n");
        assert_eq!(blocks.len(), 1, "got: {blocks:#?}");
        assert!(
            blocks[0].html.contains("href=\"https://example.com\""),
            "the definition has to reach the paragraph: {}",
            blocks[0].html
        );
    }

    /// The reason the blocks are pushed in one pass with a sentinel between
    /// them rather than one `push_html` each: the writer's footnote numbering
    /// restarts on every call, so both of these would come back `1`.
    #[test]
    fn footnotes_are_numbered_across_the_document_not_within_each_block() {
        let blocks = blocks("One[^a].\n\nTwo[^b].\n\n[^a]: first\n\n[^b]: second\n");
        assert!(blocks[0].html.contains(">1</a>"), "got: {}", blocks[0].html);
        assert!(blocks[1].html.contains(">2</a>"), "got: {}", blocks[1].html);
    }

    #[test]
    fn an_empty_document_has_no_blocks() {
        assert!(blocks("").is_empty());
    }
}
