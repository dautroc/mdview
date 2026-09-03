//! The rendered diff: the document as it renders now, with the blocks that
//! changed against an older version of it marked, and that older version kept
//! beside each one.
//!
//! The source diff in `diff.rs` answers "which lines moved". This answers
//! "which of these paragraphs is not the one I wrote", which is a different
//! question and wants a different unit: the block, not the line. So this module
//! ignores Git's hunks entirely and pairs the two documents' top-level blocks
//! itself. All it needs is the two sources, which is also what makes it
//! testable without a repository.

use crate::escape::escape_html;
use crate::highlight::Highlighter;
use crate::render::{render_blocks, Block};

/// What happened to one block between the two versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Same,
    Changed,
    Added,
    Removed,
}

/// One row of the alignment: which block of each side it speaks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockOp {
    pub change: Change,
    pub old: Option<usize>,
    pub new: Option<usize>,
}

/// Above this many cells the alignment table is not worth building. A document
/// that big is a generated one, and pairing its middle positionally costs a
/// worse diff on a file nobody is reading paragraph by paragraph -- where
/// spending a second and a gigabyte would cost the whole view.
const LCS_CELL_CAP: usize = 1_000_000;

/// One column or two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Columns {
    /// The document, with the blocks that changed marked and each one's older
    /// version folded away under it.
    Single,
    /// The two documents beside each other, a row to a block.
    Split,
}

/// Render `new_source` as itself, against `old_source`.
pub fn render_body(
    old_source: &str,
    new_source: &str,
    highlighter: &Highlighter,
    base_dir: Option<&std::path::Path>,
    columns: Columns,
) -> String {
    let old = render_blocks(old_source, highlighter, base_dir);
    let new = render_blocks(new_source, highlighter, base_dir);
    let ops = pair_blocks(&old, &new);

    let mut html = format!(
        "<div class=\"mdview-rdiff mdview-rdiff-{}\">",
        match columns {
            Columns::Single => "single",
            Columns::Split => "split",
        }
    );
    html.push_str(&note(old_source, new_source, &ops));
    for op in &ops {
        match columns {
            Columns::Single => push_single(&mut html, op, &old, &new),
            Columns::Split => push_row(&mut html, op, &old, &new),
        }
    }
    html.push_str("</div>");
    html
}

/// One block of the document, marked if it changed, with the version that was
/// there before folded away under it.
fn push_single(html: &mut String, op: &BlockOp, old: &[Block], new: &[Block]) {
    match op.change {
        Change::Same => {
            if let Some(index) = op.new {
                html.push_str(&new[index].html);
            }
        }
        Change::Changed => {
            if let (Some(new_index), Some(old_index)) = (op.new, op.old) {
                html.push_str(&marked(&new[new_index].html, "changed"));
                html.push_str(&before(&old[old_index].html, "changed", "Before"));
            }
        }
        Change::Added => {
            if let Some(index) = op.new {
                html.push_str(&marked(&new[index].html, "added"));
            }
        }
        Change::Removed => {
            if let Some(index) = op.old {
                html.push_str(&before(&old[index].html, "removed", "Removed"));
            }
        }
    }
}

/// One row of the two-column view: what the block was, beside what it is.
///
/// The pairing is what makes this layout possible at all. Two rendered
/// documents laid side by side drift apart within a screen, because a
/// paragraph and the paragraph that replaced it are not the same height; a row
/// per pair is as tall as its taller half, so the two columns stay level for
/// the length of the document without measuring anything.
///
/// The mark goes on the row rather than on the blocks. In one column a mark is
/// a bar in the margin beside the text; here the row already has two margins of
/// its own, and a block that carried its own bar would sit a few pixels out of
/// line with the block it is being compared to.
fn push_row(html: &mut String, op: &BlockOp, old: &[Block], new: &[Block]) {
    let change = match op.change {
        Change::Same => "same",
        Change::Changed => "changed",
        Change::Added => "added",
        Change::Removed => "removed",
    };
    html.push_str(&format!(
        "<div class=\"mdview-rdiff-row\" data-mdview-change=\"{change}\">"
    ));
    push_side(html, op.old.map(|index| old[index].html.as_str()), "old");
    push_side(html, op.new.map(|index| new[index].html.as_str()), "new");
    html.push_str("</div>");
}

/// One half of a row. An absent half is still a cell: the row is a grid, and a
/// missing column would let the other half spread across both.
///
/// The older half's side class is `mdview-rdiff-old`, which is also the class
/// the folded-away version wears in one column, and that is deliberate rather
/// than a collision: it is the one thing the page keys on to keep a version
/// that is not the document out of the outline, out of the heading keys and out
/// of the find index. Here it matters more than in the folded layout, not less,
/// because this version is on screen from the moment the view opens.
fn push_side(html: &mut String, block: Option<&str>, side: &str) {
    let gap = if block.is_none() { " mdview-rdiff-gap" } else { "" };
    html.push_str(&format!(
        "<div class=\"mdview-rdiff-side mdview-rdiff-{side}{gap}\">{}</div>",
        block.unwrap_or("")
    ));
}

/// What to say when the file changed somewhere this layout cannot show.
///
/// Two edits are invisible to a renderer by design: the frontmatter, which
/// comes off before the parser sees it, and a link reference definition, which
/// parses to no event at all. Either can be the whole of a commit. A layout
/// that showed an unmarked document for one of them would be claiming nothing
/// changed, which is the one thing a diff must never do.
fn note(old_source: &str, new_source: &str, ops: &[BlockOp]) -> String {
    let old_meta = crate::frontmatter::split(old_source).0.unwrap_or("");
    let new_meta = crate::frontmatter::split(new_source).0.unwrap_or("");
    if old_meta != new_meta {
        return format!(
            "<details class=\"mdview-rdiff-note\"><summary>The frontmatter changed</summary>\
             <pre class=\"mdview-rdiff-meta mdview-rdiff-meta-old\">{}</pre>\
             <pre class=\"mdview-rdiff-meta mdview-rdiff-meta-new\">{}</pre></details>",
            escape_html(old_meta.trim_matches('\n')),
            escape_html(new_meta.trim_matches('\n')),
        );
    }
    if ops.iter().all(|op| op.change == Change::Same) {
        return "<p class=\"mdview-rdiff-note\">The file changed, but nothing in the rendered \
                document did — a link definition, or whitespace. <code>g l</code> reaches the \
                source diff.</p>"
            .to_string();
    }
    String::new()
}

/// The older version of a block, kept out of the document proper.
///
/// It lives in a `<template>`, not merely inside a closed `<details>`: nothing
/// walks into template content, so this version cannot reach the outline, the
/// find index or the offsets the comment anchors are stored against, and KaTeX
/// and Mermaid cannot render it at zero height -- which for Mermaid is
/// permanent, since it marks what it has drawn and never draws it again. The
/// page hydrates the template the first time a reader opens one.
fn before(html: &str, change: &str, summary: &str) -> String {
    format!(
        "<details class=\"mdview-rdiff-old\" data-mdview-change=\"{change}\">\
         <summary>{summary}</summary><div class=\"mdview-rdiff-old-body\"></div>\
         <template>{html}</template></details>"
    )
}

/// Stamp the change onto the block's own outermost element.
///
/// Wrapping it in a div instead would cost the minimap its classification --
/// it reads `content.children` by tag name -- and would put a box around every
/// changed paragraph in a view whose whole point is that the document still
/// looks like itself.
fn marked(html: &str, change: &str) -> String {
    match mark_first_tag(html, change) {
        Some(marked) => marked,
        // Nothing to stamp: an HTML comment, or bare text from a raw HTML
        // block. A box of its own is the honest fallback; the minimap then
        // reads it as prose rather than as whatever it really is.
        None => format!("<div class=\"mdview-rdiff-block\" data-mdview-change=\"{change}\">{html}</div>"),
    }
}

/// Insert the attribute after the first tag's *name*. Scanning for the first
/// `>` instead would cut inside an attribute value: `<img alt="a > b">`.
fn mark_first_tag(html: &str, change: &str) -> Option<String> {
    let rest = html.strip_prefix('<')?;
    if !rest.starts_with(|ch: char| ch.is_ascii_alphabetic()) {
        return None;
    }
    let at = 1 + rest.find(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-')?;
    Some(format!(
        "{} data-mdview-change=\"{change}\"{}",
        &html[..at],
        &html[at..]
    ))
}

/// Align the two documents' blocks.
///
/// Keyed on block source rather than on rendered HTML: source is what decides
/// where a block *moved to*, and it is free. Rendered equality then gets the
/// last word, so that rewriting `_em_` as `*em*` -- a change to the file that
/// is not a change to the document -- comes back as Same.
///
/// Unmatched runs pair the way `diff::split_rows` pairs unmatched line runs:
/// the first `min(removed, added)` of them are one block rewritten, and the
/// remainder stand alone. It is the same guess about the same kind of edit, and
/// the two views should not disagree about it.
pub fn pair_blocks(old: &[Block], new: &[Block]) -> Vec<BlockOp> {
    let mut ops = Vec::new();
    let mut prefix = 0;
    while prefix < old.len() && prefix < new.len() && keyed(&old[prefix]) == keyed(&new[prefix]) {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < old.len() - prefix
        && suffix < new.len() - prefix
        && keyed(&old[old.len() - 1 - suffix]) == keyed(&new[new.len() - 1 - suffix])
    {
        suffix += 1;
    }

    for index in 0..prefix {
        ops.push(pair(old, new, index, index));
    }

    let old_mid = &old[prefix..old.len() - suffix];
    let new_mid = &new[prefix..new.len() - suffix];
    let matched = if old_mid.len().saturating_mul(new_mid.len()) > LCS_CELL_CAP {
        Vec::new()
    } else {
        longest_common_subsequence(old_mid, new_mid)
    };

    let (mut from_old, mut from_new) = (0, 0);
    let end = (old_mid.len(), new_mid.len());
    for (at_old, at_new) in matched.into_iter().chain(std::iter::once(end)) {
        gap(
            &mut ops,
            (prefix + from_old, prefix + at_old),
            (prefix + from_new, prefix + at_new),
        );
        if at_old < old_mid.len() {
            ops.push(pair(old, new, prefix + at_old, prefix + at_new));
        }
        from_old = at_old + 1;
        from_new = at_new + 1;
    }

    for index in 0..suffix {
        ops.push(pair(
            old,
            new,
            old.len() - suffix + index,
            new.len() - suffix + index,
        ));
    }

    // The alignment is keyed on source, so a block can pair as rewritten and
    // still render exactly as it did -- `_em_` written as `*em*`. The reader is
    // being shown the document, not the file, so the rendering has the last
    // word.
    for op in &mut ops {
        if let (Change::Changed, Some(from), Some(to)) = (op.change, op.old, op.new) {
            if old[from].html == new[to].html {
                op.change = Change::Same;
            }
        }
    }
    ops
}

/// Two blocks that align. Same when they also render the same, which is what
/// the reader is being asked to look at.
fn pair(old: &[Block], new: &[Block], old_index: usize, new_index: usize) -> BlockOp {
    let change = if old[old_index].html == new[new_index].html {
        Change::Same
    } else {
        Change::Changed
    };
    BlockOp { change, old: Some(old_index), new: Some(new_index) }
}

/// A run of blocks matched on neither side.
fn gap(ops: &mut Vec<BlockOp>, old: (usize, usize), new: (usize, usize)) {
    let removed = old.1 - old.0;
    let added = new.1 - new.0;
    let rewritten = removed.min(added);
    for offset in 0..rewritten {
        ops.push(BlockOp {
            change: Change::Changed,
            old: Some(old.0 + offset),
            new: Some(new.0 + offset),
        });
    }
    for offset in rewritten..removed {
        ops.push(BlockOp { change: Change::Removed, old: Some(old.0 + offset), new: None });
    }
    for offset in rewritten..added {
        ops.push(BlockOp { change: Change::Added, old: None, new: Some(new.0 + offset) });
    }
}

/// What two blocks are the same block by. Trimmed, so that a paragraph that
/// only gained a blank line after it is not a new paragraph.
fn keyed(block: &Block) -> &str {
    block.source.trim()
}

/// Indices of one longest common subsequence, as `(old, new)` pairs.
fn longest_common_subsequence(old: &[Block], new: &[Block]) -> Vec<(usize, usize)> {
    let (rows, cols) = (old.len() + 1, new.len() + 1);
    let mut table = vec![0u32; rows * cols];
    for row in (0..old.len()).rev() {
        for col in (0..new.len()).rev() {
            table[row * cols + col] = if keyed(&old[row]) == keyed(&new[col]) {
                table[(row + 1) * cols + col + 1] + 1
            } else {
                table[(row + 1) * cols + col].max(table[row * cols + col + 1])
            };
        }
    }

    let mut out = Vec::new();
    let (mut row, mut col) = (0, 0);
    while row < old.len() && col < new.len() {
        if keyed(&old[row]) == keyed(&new[col]) {
            out.push((row, col));
            row += 1;
            col += 1;
        } else if table[(row + 1) * cols + col] >= table[row * cols + col + 1] {
            row += 1;
        } else {
            col += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(old: &str, new: &str) -> String {
        render_body(old, new, &Highlighter::new(), None, Columns::Single)
    }

    fn split(old: &str, new: &str) -> String {
        render_body(old, new, &Highlighter::new(), None, Columns::Split)
    }

    fn ops(old: &str, new: &str) -> Vec<BlockOp> {
        let highlighter = Highlighter::new();
        let old = render_blocks(old, &highlighter, None);
        let new = render_blocks(new, &highlighter, None);
        pair_blocks(&old, &new)
    }

    fn changes(old: &str, new: &str) -> Vec<Change> {
        ops(old, new).into_iter().map(|op| op.change).collect()
    }

    #[test]
    fn an_unchanged_document_marks_no_block() {
        let doc = "# Title\n\nOne.\n\nTwo.\n";
        let html = body(doc, doc);
        assert!(!html.contains("data-mdview-change"), "got: {html}");
        assert!(!html.contains("mdview-rdiff-old"), "got: {html}");
    }

    #[test]
    fn an_edited_paragraph_is_changed_and_carries_its_older_version_in_a_template() {
        let html = body("# Title\n\nOld words.\n", "# Title\n\nNew words.\n");
        assert!(html.contains("<h1>Title</h1>"), "the unchanged heading is untouched: {html}");
        assert!(
            html.contains("<p data-mdview-change=\"changed\">New words.</p>"),
            "got: {html}"
        );
        assert!(
            html.contains("<template><p>Old words.</p></template>"),
            "the older version has to be out of the document: {html}"
        );
    }

    /// The same guess `diff::split_rows` makes about lines: the first
    /// `min(removed, added)` of a run are one thing rewritten, and the rest
    /// stand alone. Two views of one file should not disagree about that.
    #[test]
    fn unmatched_runs_pair_the_way_split_rows_do_and_the_remainder_stands_alone() {
        let old = "Keep.\n\nA one.\n\nB two.\n\nC three.\n\nKeep too.\n";
        let new = "Keep.\n\nA changed.\n\nKeep too.\n";
        assert_eq!(
            changes(old, new),
            vec![Change::Same, Change::Changed, Change::Removed, Change::Removed, Change::Same]
        );
    }

    #[test]
    fn a_deleted_block_keeps_the_place_it_was_deleted_from() {
        let html = body("One.\n\nGone.\n\nTwo.\n", "One.\n\nTwo.\n");
        let removed = html.find("Removed").expect("a summary for the deletion");
        let after = html.find("<p>Two.</p>").expect("the block it was above");
        assert!(removed < after, "the deletion belongs where it was: {html}");
        assert!(html.contains("<template><p>Gone.</p></template>"), "got: {html}");
    }

    #[test]
    fn the_marker_goes_on_the_blocks_own_element_and_never_wraps_it() {
        let html = body("One.\n", "One.\n\n| a |\n| - |\n| b |\n");
        assert!(html.contains("<table data-mdview-change=\"added\">"), "got: {html}");
        assert!(!html.contains("mdview-rdiff-block"), "nothing here needs a box: {html}");
    }

    /// An HTML comment is a block with no element to stamp. It gets a box
    /// rather than a mangled first tag.
    #[test]
    fn a_block_that_does_not_begin_with_a_tag_is_wrapped_rather_than_mangled() {
        let html = body("One.\n", "One.\n\n<!-- a note -->\n");
        assert!(
            html.contains("<div class=\"mdview-rdiff-block\" data-mdview-change=\"added\">"),
            "got: {html}"
        );
    }

    /// The reason the attribute goes in after the tag *name*: an attribute
    /// value is allowed to contain the character a naive scan would stop at.
    #[test]
    fn an_attribute_value_containing_a_greater_than_sign_is_not_split() {
        let marked = mark_first_tag("<img src=\"x.png\" alt=\"a > b\">", "added").expect("a tag");
        assert_eq!(marked, "<img data-mdview-change=\"added\" src=\"x.png\" alt=\"a > b\">");
    }

    /// A change to the file that is not a change to the document. The
    /// alignment is keyed on source, so this pairs as Changed; rendering
    /// equally is what takes it back to Same.
    #[test]
    fn an_edit_that_renders_identically_is_not_a_change() {
        assert_eq!(changes("Some _em_ here.\n", "Some *em* here.\n"), vec![Change::Same]);
    }

    #[test]
    fn a_frontmatter_only_edit_says_so_rather_than_showing_nothing() {
        let html = body("---\ntitle: Old\n---\n\nBody.\n", "---\ntitle: New\n---\n\nBody.\n");
        assert!(html.contains("The frontmatter changed"), "got: {html}");
        assert!(html.contains("title: Old"), "got: {html}");
        assert!(html.contains("title: New"), "got: {html}");
        assert!(!html.contains("data-mdview-change"), "the body did not change: {html}");
    }

    #[test]
    fn a_change_the_renderer_cannot_see_is_named_rather_than_hidden() {
        let html = body(
            "See [spec].\n\n[spec]: https://example.com\n",
            "See [spec].\n\n[spec]: https://example.com\n\n[other]: https://example.com/other\n",
        );
        assert!(html.contains("mdview-rdiff-note"), "got: {html}");
        assert!(html.contains("source diff"), "got: {html}");
    }

    /// The pairing is what makes two columns possible: a row per pair, as tall
    /// as its taller half, so the two documents stay level without anything
    /// having to measure them.
    #[test]
    fn two_columns_put_each_block_beside_the_one_it_replaced() {
        let html = split("# Title\n\nOld words.\n", "# Title\n\nNew words.\n");
        assert_eq!(html.matches("mdview-rdiff-row").count(), 2, "a row per block: {html}");
        assert!(
            html.contains(
                "<div class=\"mdview-rdiff-row\" data-mdview-change=\"changed\">\
                 <div class=\"mdview-rdiff-side mdview-rdiff-old\"><p>Old words.</p></div>\
                 <div class=\"mdview-rdiff-side mdview-rdiff-new\"><p>New words.</p></div>"
            ),
            "got: {html}"
        );
    }

    /// On screen from the start rather than behind a fold, so the class that
    /// keeps it out of the outline and the find index matters more here, not
    /// less.
    #[test]
    fn the_older_column_says_it_is_not_the_document() {
        let html = split("Old.\n", "New.\n");
        assert!(html.contains("mdview-rdiff-side mdview-rdiff-old"), "got: {html}");
        assert!(!html.contains("<template>"), "two columns need no fold: {html}");
        assert!(!html.contains("<details"), "two columns need no fold: {html}");
    }

    /// The row carries the mark. A block carrying its own would sit a few
    /// pixels out of line with the block it is being compared to.
    #[test]
    fn in_two_columns_the_row_is_marked_and_the_blocks_are_not() {
        let html = split("Old.\n", "New.\n");
        assert!(html.contains("<p>New.</p>"), "the block is unmarked: {html}");
        assert!(html.contains("<p>Old.</p>"), "the block is unmarked: {html}");
        assert_eq!(html.matches("data-mdview-change").count(), 1, "got: {html}");
    }

    #[test]
    fn a_block_with_no_counterpart_leaves_the_other_half_empty() {
        let html = split("One.\n", "One.\n\nTwo.\n");
        assert!(
            html.contains("<div class=\"mdview-rdiff-side mdview-rdiff-old mdview-rdiff-gap\"></div>"),
            "an added block has nothing to sit beside: {html}"
        );
    }

    /// Above the cap the alignment table is not built at all, and the whole
    /// middle pairs positionally. The view still works; it just guesses.
    #[test]
    fn a_document_too_large_to_align_falls_back_to_positional_pairing() {
        let old = (0..1200).map(|n| format!("Line {n}.\n\n")).collect::<String>();
        let new = (0..1200).map(|n| format!("Line {}.\n\n", n + 1)).collect::<String>();
        let changes = changes(&old, &new);
        assert_eq!(changes.len(), 1200, "every block pairs with the one in its place");
        assert!(
            changes.iter().all(|change| *change == Change::Changed),
            "an alignment would have found 1199 of these unchanged"
        );
    }
}
