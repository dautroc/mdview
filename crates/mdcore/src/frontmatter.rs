//! The metadata block some tools put above a Markdown document.
//!
//! Obsidian, Jekyll, Hugo and every static-site generator after them open a
//! file with a fenced block of YAML or TOML. It is addressed to the tool, not
//! to the reader, so a viewer's job is to take it off before rendering.
//!
//! Left on, it does not merely show: `---` opens as a thematic break, the
//! lines under it read as a paragraph, and the closing `---` turns that
//! paragraph into a setext heading. So a note beginning
//!
//! ```text
//! ---
//! title: My Note
//! ---
//! ```
//!
//! renders a rule and an `<h2>` reading `title: My Note`, and that `<h2>` then
//! takes the first row of the outline sidebar. The metadata does not just
//! clutter the document; it outranks the document's own title.

/// The fences we recognise, opening and closing. `---` is YAML, which is what
/// Obsidian and Jekyll write; `+++` is TOML, which is Hugo's.
const FENCES: [&str; 2] = ["---", "+++"];

/// Split a leading frontmatter block off the Markdown that follows.
///
/// Returns the block's contents without its fences, and the Markdown. When
/// there is no frontmatter the whole of `source` comes back as the Markdown,
/// borrowed, not copied.
///
/// The rules are the ones the writing tools themselves apply: the opening
/// fence has to be the file's very first line, the closing fence is the same
/// three characters alone on a line, and nothing in between is inspected. That
/// last part is deliberate. Validating the block as YAML would mean deciding
/// what to do with a block that does not parse, and the answer — show the
/// reader a page of raw metadata — is worse than the alternative in every case
/// we could construct.
///
/// It also means a document that genuinely opens on a thematic break and draws
/// another one further down has its opening rule eaten. That shape is rare
/// enough, and identical enough to real frontmatter, that no tool in this
/// family distinguishes them either.
pub fn split(source: &str) -> (Option<&str>, &str) {
    let open_end = line_end(source, 0);
    let Some(fence) = FENCES.iter().find(|fence| source[..open_end].trim_end() == **fence) else {
        return (None, source);
    };

    let mut cursor = open_end;
    while cursor < source.len() {
        let end = line_end(source, cursor);
        if source[cursor..end].trim_end() == *fence {
            return (Some(&source[open_end..cursor]), &source[end..]);
        }
        cursor = end;
    }

    // No closing fence anywhere in the file. Then the first line was never a
    // fence at all -- it was a thematic break, and the document is entitled to
    // start with one.
    (None, source)
}

/// `source` with any leading frontmatter removed. What the renderer wants.
pub fn strip(source: &str) -> &str {
    split(source).1
}

/// The offset just past the newline ending the line that starts at `from`, or
/// the end of `source` for a final line with no newline. Trailing `\r` stays
/// inside the line and is dealt with by the caller's `trim_end`, so a file
/// with CRLF endings fences the same as one without.
fn line_end(source: &str, from: usize) -> usize {
    match source[from..].find('\n') {
        Some(index) => from + index + 1,
        None => source.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::{split, strip};

    #[test]
    fn yaml_frontmatter_is_split_from_the_markdown() {
        let (block, body) = split("---\ntitle: My Note\ntags: [a, b]\n---\n\n# Hello\n");
        assert_eq!(block, Some("title: My Note\ntags: [a, b]\n"));
        assert_eq!(body, "\n# Hello\n");
    }

    /// Hugo's fence. Same shape, different characters.
    #[test]
    fn toml_frontmatter_is_split_the_same_way() {
        let (block, body) = split("+++\ntitle = \"My Note\"\n+++\n# Hello\n");
        assert_eq!(block, Some("title = \"My Note\"\n"));
        assert_eq!(body, "# Hello\n");
    }

    /// The one thing this must not break: a document whose author meant to
    /// draw a rule. Without a closing fence there is no block, and the `---`
    /// goes back to being ordinary Markdown.
    #[test]
    fn an_opening_fence_with_no_closing_one_is_a_thematic_break() {
        let source = "---\n\nA document that opens on a rule.\n";
        assert_eq!(split(source), (None, source));
    }

    #[test]
    fn a_bare_fence_line_is_left_alone() {
        assert_eq!(strip("---"), "---");
        assert_eq!(strip("---\n"), "---\n");
    }

    /// Exactly three characters. `----` is a setext underline or a thematic
    /// break depending on what precedes it, and neither is ours to eat.
    #[test]
    fn a_longer_rule_is_not_a_fence() {
        let source = "----\ntitle: x\n----\n# H\n";
        assert_eq!(split(source), (None, source));
    }

    /// Frontmatter is the first line or it is nothing. A `---` after a
    /// paragraph is that paragraph's setext underline, which is a heading the
    /// author asked for.
    #[test]
    fn a_block_below_the_first_line_is_not_frontmatter() {
        let source = "# Hello\n\n---\ntitle: x\n---\n";
        assert_eq!(split(source), (None, source));
    }

    #[test]
    fn crlf_line_endings_fence_the_same_as_lf() {
        let (block, body) = split("---\r\ntitle: x\r\n---\r\n# Hello\r\n");
        assert_eq!(block, Some("title: x\r\n"));
        assert_eq!(body, "# Hello\r\n");
    }

    /// Trailing spaces after a fence are invisible in an editor, so they must
    /// not decide whether the block is recognised.
    #[test]
    fn trailing_whitespace_on_a_fence_does_not_hide_it() {
        let (block, body) = split("---  \ntitle: x\n---\t\n# Hello\n");
        assert_eq!(block, Some("title: x\n"));
        assert_eq!(body, "# Hello\n");
    }

    #[test]
    fn an_empty_block_is_still_a_block() {
        let (block, body) = split("---\n---\n# Hello\n");
        assert_eq!(block, Some(""));
        assert_eq!(body, "# Hello\n");
    }

    /// The two fences are separate languages; a document does not get to open
    /// in one and close in the other.
    #[test]
    fn mismatched_fences_do_not_pair() {
        let source = "---\ntitle: x\n+++\n# Hello\n";
        assert_eq!(split(source), (None, source));
    }

    #[test]
    fn a_document_with_no_frontmatter_is_returned_whole() {
        let source = "# Hello\n\nBody.\n";
        assert_eq!(split(source), (None, source));
        assert_eq!(split(""), (None, ""));
    }

    /// A `---` inside the block closes it, because the fence is the closing
    /// rule and nothing looks past it. Documented so the behaviour is chosen
    /// rather than discovered.
    #[test]
    fn the_first_closing_fence_wins() {
        let (block, body) = split("---\na: 1\n---\nb: 2\n---\n# Hello\n");
        assert_eq!(block, Some("a: 1\n"));
        assert_eq!(body, "b: 2\n---\n# Hello\n");
    }
}
