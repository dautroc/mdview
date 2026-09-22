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
    let grouped = group_document(markdown, highlighter, base_dir);
    if grouped.events.is_empty() {
        return String::new();
    }
    // Pushed in one pass and split back into blocks, so that each block can be
    // told which line of the file it came from: that attribute is what
    // `mdview notes.md:42` aims at. The split is the one `render_blocks`
    // makes, and when the sentinel does not survive it, the body is pushed
    // whole instead -- the line numbers are what a stray U+0001 costs, never
    // the document.
    match push_grouped_html(&grouped.events) {
        Some(parts) => parts
            .iter()
            .zip(&grouped.lines)
            .map(|(html, line)| with_source_line(html, *line))
            .collect::<Vec<_>>()
            .join("\n"),
        None => {
            let mut out = String::new();
            html::push_html(&mut out, grouped.events.into_iter().flatten());
            out
        }
    }
}

pub fn image_warnings(markdown: &str, base_dir: &std::path::Path) -> Vec<crate::images::ImageWarning> {
    let markdown = crate::frontmatter::strip(markdown);
    let mut seen = std::collections::HashSet::new();
    Parser::new_ext(markdown, markdown_options()).filter_map(|event| match event {
        Event::Start(Tag::Image { dest_url, .. }) => {
            let destination = dest_url.to_string();
            if !seen.insert(destination.clone()) { return None; }
            crate::images::warning(&destination, base_dir)
        }
        _ => None,
    }).collect()
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
///
/// `line` is the other direction, and counts what the parser never saw: the
/// 1-based line of the *file* the block starts on, metadata block included,
/// because a line number that has to be corrected for frontmatter is not the
/// number the editor, the compiler or `grep -n` that sent the reader here
/// printed.
///
/// The HTML is deliberately free of it. The rendered diff decides that a block
/// is unchanged by comparing two blocks' HTML, and a line number in the markup
/// would report every paragraph below an inserted line as rewritten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub html: String,
    pub source: String,
    pub range: std::ops::Range<usize>,
    pub line: u32,
}

/// `html` with the line of the file it came from on its outermost element.
///
/// Inserted after the tag name rather than before the `>`: an attribute value
/// is allowed to contain one, and finding the end of an opening tag properly
/// means parsing the HTML this crate just generated. A block that opens with
/// no tag at all -- an HTML comment, a reference definition that rendered to
/// nothing -- is handed back untouched, and the reveal aims at the block above
/// it instead.
pub fn with_source_line(html: &str, line: u32) -> String {
    let Some(rest) = html.strip_prefix('<') else {
        return html.to_string();
    };
    let name = rest
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(rest.len());
    if name == 0 {
        return html.to_string();
    }
    let at = 1 + name;
    format!("{} data-line=\"{line}\"{}", &html[..at], &html[at..])
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
    let grouped = group_document(markdown, highlighter, base_dir);
    if grouped.events.is_empty() {
        return Vec::new();
    }
    let htmls = push_grouped_html(&grouped.events).unwrap_or_else(|| {
        // The sentinel came back cut or doubled. Rendering each block on its
        // own gets the footnote numbers wrong, which is a smaller lie than a
        // document sliced in the wrong places.
        grouped
            .events
            .iter()
            .map(|events| {
                let mut html = String::new();
                html::push_html(&mut html, events.iter().cloned());
                html.trim().to_string()
            })
            .collect()
    });
    htmls
        .into_iter()
        .zip(grouped.ranges)
        .zip(grouped.lines)
        .map(|((html, range), line)| Block {
            source: grouped.markdown[range.clone()].to_string(),
            html,
            range,
            line,
        })
        .collect()
}

/// A document parsed once and cut into its top-level blocks: the events of
/// each block, the byte range it occupies in the Markdown the parser saw, and
/// the line of the file it begins on. Both renderings above are built from
/// this, which is what keeps them the same document.
struct Grouped<'a> {
    events: Vec<Vec<Event<'a>>>,
    ranges: Vec<std::ops::Range<usize>>,
    lines: Vec<u32>,
    /// `source` with its frontmatter removed: what `ranges` index into.
    markdown: &'a str,
}

fn group_document<'a>(
    source: &'a str,
    highlighter: &Highlighter,
    base_dir: Option<&std::path::Path>,
) -> Grouped<'a> {
    // Frontmatter comes off before the parser sees it. Doing it here rather
    // than in `Document` keeps it out of the diff view, which shows the file
    // as it is on disk and would be wrong to hide a metadata edit from.
    let markdown = crate::frontmatter::strip(source);
    let slugs = crate::links::headings(markdown)
        .into_iter()
        .map(|heading| heading.slug)
        .collect::<Vec<_>>();
    let mut heading_index = 0;
    let mut events = Vec::new();
    let mut ranges = Vec::new();
    for (range, group) in group_events(markdown) {
        let group = assign_heading_ids_from(group, &slugs, &mut heading_index);
        events.push(inline_images(
            transform_events(group.into_iter(), highlighter),
            base_dir,
        ));
        ranges.push(range);
    }
    let lines = source_lines(source, markdown, &ranges);
    Grouped {
        events,
        ranges,
        lines,
        markdown,
    }
}

/// Push every block's HTML in a single pass and hand back one string per
/// block, or `None` when the sentinel came back cut or doubled and the pieces
/// therefore do not line up with the blocks.
fn push_grouped_html(groups: &[Vec<Event<'_>>]) -> Option<Vec<String>> {
    let mut combined: Vec<Event> = Vec::new();
    for (index, events) in groups.iter().enumerate() {
        if index > 0 {
            combined.push(Event::Html(CowStr::from(BLOCK_SENTINEL)));
        }
        combined.extend(events.iter().cloned());
    }
    let mut html = String::new();
    html::push_html(&mut html, combined.into_iter());
    let parts = html
        .split(BLOCK_SENTINEL)
        .map(|part| part.trim().to_string())
        .collect::<Vec<_>>();
    (parts.len() == groups.len()).then_some(parts)
}

/// The 1-based line of the file each block starts on. `markdown` is `source`
/// with its frontmatter taken off, so the lines that block occupied are added
/// back on here -- the parser's offsets are into the text it was given, and
/// nobody counts lines from below the metadata.
fn source_lines(source: &str, markdown: &str, ranges: &[std::ops::Range<usize>]) -> Vec<u32> {
    let first = 1 + newlines(&source[..source.len() - markdown.len()]);
    let mut line = first;
    let mut scanned = 0;
    let mut lines = Vec::with_capacity(ranges.len());
    for range in ranges {
        let start = range.start.min(markdown.len());
        // Blocks arrive in document order, so the scan walks the text once.
        // A range that went backwards would still be counted correctly.
        if start < scanned {
            line = first;
            scanned = 0;
        }
        line += newlines(&markdown[scanned..start]);
        scanned = start;
        lines.push(line);
    }
    lines
}

fn newlines(text: &str) -> u32 {
    text.bytes().filter(|byte| *byte == b'\n').count() as u32
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

fn assign_heading_ids_from<'a>(
    events: impl IntoIterator<Item = Event<'a>>,
    slugs: &[String],
    index: &mut usize,
) -> Vec<Event<'a>> {
    events
        .into_iter()
        .map(|event| match event {
            Event::Start(Tag::Heading {
                level,
                id: _,
                classes,
                attrs,
            }) => {
                let id = slugs
                    .get(*index)
                    .cloned()
                    .unwrap_or_else(|| "section".to_string());
                *index += 1;
                Event::Start(Tag::Heading {
                    level,
                    id: Some(CowStr::from(id)),
                    classes,
                    attrs,
                })
            }
            other => other,
        })
        .collect()
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
        assert_eq!(
            headings(markdown),
            vec!["One", "The retry loop", "Bold and italic"]
        );
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

    #[test]
    fn rendered_heading_ids_use_the_link_resolvers_duplicate_slugs() {
        let html = super::render_body("# Repeat\n\n# Repeat\n\n# Café & tea\n");
        assert!(html.contains("id=\"repeat\">Repeat</h1>"));
        assert!(html.contains("id=\"repeat-1\">Repeat</h1>"));
        assert!(html.contains("id=\"café--tea\">Café &amp; tea</h1>"));
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
        assert!(
            !html.contains("<hr"),
            "the opening fence is still a rule: {html}"
        );
        assert!(
            !html.contains("My Note"),
            "the metadata is still in the body: {html}"
        );
        assert!(
            html.starts_with("<h1 data-line=\"6\" id=\"hello\">Hello</h1>"),
            "got: {html}"
        );
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
        assert!(
            blocks[2].html.starts_with("<pre"),
            "got: {}",
            blocks[2].html
        );
    }

    /// Neither is a `Start`/`End` pair, so a grouper that only closed on `End`
    /// would swallow both into whatever block came next.
    #[test]
    fn a_raw_html_block_and_a_thematic_break_are_each_their_own_block() {
        let blocks = blocks("<div class=\"x\">raw</div>\n\n---\n\npara\n");
        assert_eq!(blocks.len(), 3, "got: {blocks:#?}");
        assert!(
            blocks[0].html.contains("<div class=\"x\">"),
            "got: {}",
            blocks[0].html
        );
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

#[cfg(test)]
mod source_line_tests {
    use super::{render_blocks, render_body, with_source_line};
    use crate::highlight::Highlighter;

    fn lines(markdown: &str) -> Vec<u32> {
        render_blocks(markdown, &Highlighter::new(), None)
            .into_iter()
            .map(|block| block.line)
            .collect()
    }

    /// The line a block *starts* on, not the line it ends on: a request for a
    /// line inside a list or a fence is answered by the block containing it,
    /// which is the block whose own line is the last one at or before it.
    #[test]
    fn a_block_carries_the_line_of_the_file_it_starts_on() {
        let markdown = "# Title\n\nA paragraph.\n\n- one\n- two\n- three\n\n```rust\nfn main() {}\n```\n";
        assert_eq!(lines(markdown), vec![1, 3, 5, 9]);
    }

    /// Frontmatter never reaches the parser, so its offsets are into a
    /// document that starts three lines late. `grep -n` counts the metadata,
    /// and so does the number printed beside a line in any editor, so the
    /// stripped lines are added back.
    #[test]
    fn the_stripped_frontmatter_is_counted_back_in() {
        let markdown = "---\ntitle: My Note\n---\n\n# Title\n\nBody.\n";
        assert_eq!(lines(markdown), vec![5, 7]);
    }

    /// What the reveal actually reads. The attribute is on the outermost
    /// element of each block and nowhere else -- an inline element inside a
    /// paragraph has no line of its own to give.
    #[test]
    fn the_body_puts_the_line_on_each_top_level_element() {
        let html = render_body("# Title\n\nA *paragraph*.\n");
        assert!(html.contains("<h1 data-line=\"1\""), "got: {html}");
        assert!(html.contains("<p data-line=\"3\">"), "got: {html}");
        assert_eq!(html.matches("data-line=").count(), 2, "got: {html}");
    }

    /// The attribute goes after the tag name, so everything the writer emitted
    /// is still there, in its original order, behind it.
    #[test]
    fn annotating_keeps_the_attributes_the_writer_wrote() {
        assert_eq!(
            with_source_line("<h1 id=\"title\">Title</h1>", 4),
            "<h1 data-line=\"4\" id=\"title\">Title</h1>"
        );
    }

    /// A block with no tag to hang it on is handed back whole rather than
    /// having an attribute forced into its text.
    #[test]
    fn a_block_that_opens_with_no_tag_is_left_alone() {
        for html in ["", "<!-- a note -->", "plain text"] {
            assert_eq!(with_source_line(html, 7), html);
        }
    }
}
