# Contextual Diff Toolbar Design

## Goal

Reduce right-side control clutter while keeping the most relevant document actions immediately available.

## Interaction model

- The sidebar remains a separate hamburger button.
- Rendered Markdown mode shows one compact `Diff` button.
- Diff mode replaces that action with `Markdown` and exposes `Unified` / `Split` layout controls.
- An overflow (`⋯`) menu contains `Full Width`, labeled `Full Width` or `Exit Full Width` based on state.
- Diff layout controls are hidden whenever the document is rendered as Markdown.
- Existing menu commands and keyboard shortcuts remain available.

## Visual and accessibility rules

- Keep the control row compact and aligned at the top-right of the document.
- Prefer concise labels or familiar icons where space is limited, while retaining accessible labels and tooltips.
- Show an active state for the selected diff layout and full-width mode.
- Disable the Diff action for unavailable Git files, but allow Markdown to close a stale diff view.

## State and persistence

- View mode remains window-local and resets to rendered Markdown when opening another file.
- Unified/Split preference remains global and persisted.
- Full-width preference remains global and persisted.
- Reload and live reload preserve the active view mode, layout, and scroll position.

## Scope

This design changes only the document toolbar presentation. Git diff parsing, rendering, menu commands, and Markdown output remain unchanged except where needed to synchronize the contextual controls.
