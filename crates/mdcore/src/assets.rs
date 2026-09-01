//! Compile-time embedded assets. Nothing here is read from disk at runtime,
//! which is what lets the app work with no network and no resource lookups.

pub const PAGE_CSS: &str = include_str!("../assets/page.css");
pub const INIT_JS: &str = include_str!("../assets/init.js");
pub const KATEX_CSS: &str = include_str!("../assets/katex.css");
pub const KATEX_JS: &str = include_str!("../assets/katex.js");
pub const MERMAID_JS: &str = include_str!("../assets/mermaid.js");

/// Monokai Pro's palette is not one of syntect's defaults, so it ships as a
/// tmTheme embedded like every other asset rather than read from disk.
pub const MONOKAI_PRO_THEME: &str = include_str!("../assets/monokai-pro.tmTheme");

#[cfg(test)]
mod tests {
    /// v0.9.0 shipped a call to `showNote` with no such function anywhere in
    /// the file: every bookmark toggle threw, which took the page's own star
    /// state and the bookmarks list down with it. Nothing failed loudly, so
    /// the pairing is asserted here.
    #[test]
    fn the_note_helper_is_defined_and_styled() {
        assert!(super::INIT_JS.contains("function showNote("));
        assert!(super::INIT_JS.contains("showNote(next ? "));
        assert!(super::PAGE_CSS.contains("#mdview-note {"));
        assert!(super::PAGE_CSS.contains("#mdview-note.is-visible"));
        // The host says some of the same things, and reaches this by name.
        assert!(super::INIT_JS.contains("window.mdviewNote = showNote;"));
    }

    /// `D` refusing in silence made it read as a broken key -- it does nothing
    /// on any file outside a Git repository, and the greyed-out View menu item
    /// is no help to someone using the keyboard. The reason comes from the
    /// host, so the two hooks have to keep taking it.
    #[test]
    fn the_diff_key_says_why_it_will_not_open_the_diff() {
        let js = super::INIT_JS;
        assert!(js.contains("showNote(diffUnavailableReason"), "D refuses in silence");
        // Both hooks carry it: one runs on open, the other on live reload.
        assert!(js.contains("mdviewSetDiffAvailability = function (available, reason)"));
        assert!(js.contains("mdviewSetViewState = function (view, layout, fullWidth, available, reason)"));
        // A page the host has not spoken to yet still says something.
        assert!(js.contains("|| \"There is no Git diff for this file.\""));
    }

    /// Same hazard, same guard: the comment layer is three hooks the host
    /// calls by name and two classes only the stylesheet defines. A rename on
    /// one side alone throws, or paints nothing, in silence.
    #[test]
    fn the_comment_helpers_are_defined_and_styled() {
        for hook in [
            "window.mdviewSetComments",
            "function applyCommentAnchors(",
            "function clearCommentAnchors(",
            "function refreshHighlights(",
            "function attachCommentListeners(",
        ] {
            assert!(super::INIT_JS.contains(hook), "init.js is missing {hook}");
        }
        assert!(super::INIT_JS.contains("attachCommentListeners();"), "never attached");
        // A selection c cannot anchor is refused with a reason; only the
        // absence of a selection means "show me the comments". Sharing one
        // return value for both answered a complaint by opening a panel.
        assert!(super::INIT_JS.contains("if (capture === false) return;"));
        assert!(super::PAGE_CSS.contains(".mdview-comment-anchor {"));
        assert!(super::PAGE_CSS.contains("#mdview-comment-input {"));
    }

    /// The rail is built entirely in JS, so nothing in the emitted markup
    /// would notice if it stopped being wired up.
    #[test]
    fn the_comment_rail_is_defined_and_styled() {
        for hook in ["function layoutCommentRail(", "function focusComment(", "function railGeometry("] {
            assert!(super::INIT_JS.contains(hook), "init.js is missing {hook}");
        }
        assert!(super::PAGE_CSS.contains("#mdview-comment-rail {"));
        assert!(super::PAGE_CSS.contains(".mdview-comment-card {"));
        // The draft opens in the same column, which takes the id to out-
        // specify the bar's own fixed positioning.
        assert!(super::PAGE_CSS.contains("#mdview-comment.is-railed {"));
    }

    /// The card's buttons and the e and x keys must reach the same two
    /// functions. A second copy of either would be a comment that behaves one
    /// way from the keyboard and another from the mouse.
    #[test]
    fn the_card_buttons_share_the_key_paths() {
        let js = super::INIT_JS;
        assert!(js.contains("function commentCardButton("));
        for shared in ["function beginEditComment(", "function removeComment("] {
            assert!(js.contains(shared), "init.js is missing {shared}");
        }
        assert!(js.matches("openCommentBar(comment.quote").count() == 1, "edit has two paths");
        assert!(js.matches("postToHost(\"deleteComment:\"").count() == 1, "delete has two paths");
        assert!(super::PAGE_CSS.contains(".mdview-comment-card-btn {"));
        // Reserved by a float rather than overlaid, or hovering a card would
        // cover the end of its first line.
        assert!(super::PAGE_CSS.contains("float: right;"));
    }
}
