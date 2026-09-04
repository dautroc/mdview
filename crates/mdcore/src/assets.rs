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

/// Chiroptera Dark Hard, likewise absent from syntect's defaults. Translated
/// from the Neovim colourscheme's palette and its own syntax mapping.
pub const CHIROPTERA_DARK_HARD_THEME: &str =
    include_str!("../assets/chiroptera-dark-hard.tmTheme");

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

    /// The cursor and the view each follow the other, and the second half is
    /// three guards deep: a follow that fired in visual mode would rewrite the
    /// selection `c` is about to act on, one that fired for a reader who has
    /// never used the cursor would hand them a caret they did not ask for, and
    /// one that fired mid-`g g` would drop the cursor wherever the animation
    /// had reached. Each is a line that would be easy to lose and impossible
    /// to notice.
    #[test]
    fn the_cursor_follows_the_view_and_the_view_the_cursor() {
        let js = super::INIT_JS;
        assert!(js.contains("function scrollCursorIntoView("), "the view must follow the cursor");
        assert!(js.contains("function followScrollWithCursor("), "the cursor must follow the view");
        let start = js
            .find("function followScrollWithCursor(")
            .expect("follow function");
        let end = start + js[start..].find("\n  }").expect("follow must close");
        let body = &js[start..end];
        for guard in ["if (cursorAt === null) return;", "if (visual) return;", "cursorLedScrollUntil"] {
            assert!(body.contains(guard), "the follow has lost its guard: {guard}");
        }
        // Dragged to the margin it crossed, never recentred: a scroll of two
        // lines must not move the cursor at all.
        assert!(body.contains("CURSOR_MARGIN"), "the follow must fire only at the edges");
        assert!(
            js.contains("followScrollWithCursor();"),
            "nothing calls the follow on a scroll"
        );
    }

    /// Same hazard again. The minimap is a canvas, so nothing about it is
    /// styled by the rules it draws with: a renamed function paints nothing,
    /// and a renamed id paints somewhere nobody can see.
    #[test]
    fn the_minimap_is_defined_and_styled() {
        for hook in [
            "function paintMinimap(",
            "function placeMinimapWindow(",
            "function minimapReserve(",
            "function attachMinimapListeners(",
            "window.mdviewSetMinimap",
        ] {
            assert!(super::INIT_JS.contains(hook), "init.js is missing {hook}");
        }
        assert!(super::INIT_JS.contains("attachMinimapListeners();"), "never attached");
        // Painted pixels do not restyle themselves. Each of these is a surface
        // that changes the document's shape or its colours and fires no event
        // of its own, so each has to say so by hand.
        for repaint in [
            // Diagrams arrive with a height the first paint could not measure.
            "renderDiagrams().then(enhanceZoomables).then(scheduleMinimapPaint)",
            // The System theme stamps no attribute at all.
            "dark.addEventListener(\"change\", scheduleMinimapPaint)",
            // The marks it plots exist only once find and comments are rebuilt.
            "// Last of all: the comment and find marks the map plots exist only now.",
        ] {
            assert!(super::INIT_JS.contains(repaint), "init.js never repaints the map: {repaint}");
        }
        // Sliced rather than matched against its neighbours: a theme preview
        // is an attribute flip that fires nothing, and the repaint has to be
        // inside the function doing the flipping, wherever that function sits.
        let theme_start = super::INIT_JS.find("function applyTheme(").expect("applyTheme");
        let theme_end = theme_start
            + super::INIT_JS[theme_start..].find("\n  }").expect("applyTheme must close");
        assert!(
            super::INIT_JS[theme_start..theme_end].contains("scheduleMinimapPaint()"),
            "a theme preview leaves the map painted in the old theme's colours"
        );
        for selector in ["#mdview-minimap {", "#mdview-minimap-window {", "#mdview-minimap-canvas"] {
            assert!(super::PAGE_CSS.contains(selector), "page.css is missing {selector}");
        }
    }

    /// The outline is a map, and a map that does not mark where you are
    /// standing is half a map. The rule it uses is the one `]` and `[` use --
    /// asserted here, because two rules for "the heading you are on" would
    /// disagree the first time either moved.
    #[test]
    fn the_outline_follows_the_reader() {
        let js = super::INIT_JS;
        assert!(js.contains("function currentHeadingId("), "no current-heading rule");
        assert!(js.contains("function syncOutline("), "nothing applies it");
        assert!(
            js.contains("headings[i].getBoundingClientRect().top > HEADING_EPSILON"),
            "the outline must stand on the same line the heading keys do"
        );
        assert!(
            js.contains("schedulePositionSync, { passive: true }"),
            "the mark has to follow the scroll to be worth anything"
        );
        assert!(
            super::PAGE_CSS.contains("#mdview-sidebar-body a[data-outline-id].is-current"),
            "page.css never lights the row"
        );
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

    /// A quote is arbitrary document text of arbitrary length, and the length
    /// cap is no longer short enough to keep it out of the layout by accident.
    /// Every surface that shows one has to go through the one elider, or a
    /// section-length selection becomes a sidebar row of a thousand words and
    /// a tooltip nobody can dismiss.
    #[test]
    fn every_displayed_quote_goes_through_the_elider() {
        let js = super::INIT_JS;
        assert!(js.contains("function excerpt(text, max)"), "the elider is gone");
        // The three surfaces, by their assignments rather than by eye.
        for shown in [
            "a.textContent = excerpt(comment.note || comment.quote,",
            "a.title = excerpt(comment.quote,",
            "label.textContent = excerpt(quote,",
            "var shown = excerpt(comment.quote,",
        ] {
            assert!(js.contains(shown), "init.js shows a quote raw: {shown}");
        }
        // The refusal is a backstop now, not the thing holding the layout up.
        assert!(js.contains("var COMMENT_QUOTE_MAX = 4000;"));
    }

    /// Two comments cannot both highlight an overlapping span -- clearing a
    /// mark replaces it with flat text, taking anything nested inside it -- so
    /// one of the pair is orphaned. The enclosing one has to be the survivor:
    /// the other way round strikes through the comment about a whole passage
    /// on the strength of an aside three words wide inside it.
    ///
    /// That is a WIDEST-FIRST claim, and it is not the order the wrapping
    /// needs: wrapRuns splits the very text nodes the index was built from, so
    /// the marks still have to go on back to front. Two orderings, two passes.
    /// Collapsing them into one loop again is the regression this guards, and
    /// it fails in whichever direction it is collapsed -- it either restores
    /// the old winner or corrupts every anchor but the last.
    #[test]
    fn the_enclosing_comment_wins_an_overlap_and_wrapping_still_runs_backwards() {
        let js = super::INIT_JS;
        // Widest, then leftmost, then first to arrive: no tie is left to sort
        // stability, which is not guaranteed to be the same across engines.
        assert!(js.contains("order: i,"), "arrival order is not recorded");
        assert!(
            js.contains("return b.to - b.from - (a.to - a.from) || a.from - b.from || a.order - b.order;"),
            "the claim is no longer widest-first"
        );
        // And the winners wrapped back to front, in a pass of their own.
        assert!(
            js.contains("claimed.sort(function (a, b) {\n      return b.from - a.from;\n    });"),
            "wrapping no longer runs backwards"
        );
        assert!(js.contains("wrapRuns(runsFor(index, winner.from, winner.to)"), "wrapping is not a separate pass");
    }

    /// `g l` is the only way to the rendered layout, and the layout is only a
    /// layout because the stylesheet knows the class the body comes wrapped in.
    #[test]
    fn the_rendered_diff_layout_is_reachable_and_styled() {
        let js = super::INIT_JS;
        assert!(js.contains("function renderedDiff("), "nothing recognises the layout");
        assert!(
            js.contains("var layouts = [\"unified\", \"split\", \"rendered\", \"rendered-split\"];"),
            "g l no longer cycles all four"
        );
        assert!(
            super::PAGE_CSS.contains(".mdview-rdiff-split .mdview-rdiff-row {"),
            "the two-column layout has no rows"
        );
        assert!(
            super::PAGE_CSS.contains("details.mdview-rdiff-old {"),
            "the folded-away older version is unstyled"
        );
        assert!(
            super::PAGE_CSS
                .contains(".mdview-rdiff-single [data-mdview-change]:not(.mdview-rdiff-old)"),
            "a changed block has no mark in one column"
        );
    }

    /// The rendered layout is a diff *and* a document, and the two rules that
    /// hand it back what the source layouts give up are both a specificity
    /// accident away from doing something else. The `:not([hidden])` keeps the
    /// strip closed when the reader has closed it; rewriting the full-bleed
    /// rule in place rather than overriding it keeps `g w` working.
    #[test]
    fn the_rendered_diff_keeps_the_minimap_and_the_prose_column() {
        let css = super::PAGE_CSS;
        assert!(
            css.contains(":root[data-view=\"diff\"]:not([data-diff-layout=\"rendered\"]) #mdview-content"),
            "the rendered layout has lost its reading column"
        );
        assert!(
            css.contains(":root[data-diff-layout=\"rendered\"] #mdview-minimap:not([hidden])"),
            "the strip is either gone from the layout or forced open in it"
        );
        let start = super::INIT_JS.find("function minimapVisible(").expect("minimapVisible");
        let end = start + super::INIT_JS[start..].find("\n  }").expect("must close");
        assert!(
            super::INIT_JS[start..end].contains("renderedDiff()"),
            "the strip is styled back in but never painted"
        );
    }

    /// The older version of a block is not part of the document, and four
    /// things would treat it as if it were. It stays in a `<template>` until
    /// a reader opens it, and out of the outline and the find index after.
    #[test]
    fn the_older_version_of_a_block_cannot_reach_the_outline_or_the_find_index() {
        let js = super::INIT_JS;
        assert!(js.contains("function documentHeadings("), "no filtered heading list");
        assert_eq!(
            js.matches("querySelectorAll(\"h1, h2, h3, h4, h5, h6\")").count(),
            1,
            "every heading query has to go through documentHeadings"
        );
        assert!(
            js.contains("found[i].closest(\".mdview-rdiff-old\")"),
            "the filter no longer filters anything"
        );
        assert!(
            js.contains("el.classList.contains(\"mdview-rdiff-old\")"),
            "find would offer matches in text that is not in the document"
        );
        // Hydrated on open, so nothing renders it at zero height: mermaid
        // marks what it has drawn and would never draw it again.
        assert!(js.contains("body.appendChild(template.content);"), "never hydrated");
        assert!(js.contains("attachRenderedDiffListeners();"), "never attached");
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

    /// The card's buttons and the `g c` and `x` keys must reach the same two
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
