# Fullwidth View Design

## Goal

Add an opt-in fullwidth reading mode that lets wide Markdown content use the available MDView window width without replacing the existing readable-width default.

## User Experience

- The existing centered 46rem reading column remains the default for users who have not enabled fullwidth view.
- The View menu gains a checkable **Full Width** item with the **Option-Command-F** shortcut.
- Selecting the item toggles fullwidth view immediately and updates its checkmark.
- Fullwidth view removes the document content's 46rem maximum width while retaining the existing 1.5rem horizontal padding and vertical padding.
- When the sidebar is open, the document fills the remaining main-content area rather than extending beneath or behind the sidebar.
- The preference is global to the app and persists across launches, document changes, reloads, and live reloads.

## Architecture

The feature follows the app's existing native-menu and saved-preference patterns:

1. Add a `MDViewFullWidth` boolean preference in the defaults adapter. An absent value means `false`, preserving current behavior.
2. Add a checkable Full Width item to the native View menu. Its initial state comes from the saved preference, and its action is handled by the application delegate.
3. The application delegate toggles and saves the preference, synchronizes the menu checkmark, and asks the current document window to apply the new layout state.
4. The document window applies the state by setting a `data-fullwidth` attribute on the page root. It also queues the same operation after page loads so document changes, reloads, and live reloads restore the preference.
5. Page CSS removes `max-width: 46rem` from `#mdview-content` only while the root carries the enabled state. Existing margins and padding continue to control spacing.

This approach updates layout without rebuilding the page, so scroll position, page zoom, sidebar state, and rendered content remain intact.

## State and Data Flow

- On application startup, the View menu reads the saved boolean and shows the corresponding checkmark.
- When the user invokes Full Width, the application delegate inverts the current saved value, persists it, updates the menu item, and evaluates the layout-state script in the current window.
- When a page is created or reloaded, the window reads the saved value and queues a script that reapplies the root attribute after WebKit finishes loading.
- MDView intentionally remains centered when the preference has never been stored.

## Failure Handling

- If no document window is open, the menu action still saves the preference and updates its checkmark; the next opened document receives the chosen state.
- If the preference is absent, it resolves to disabled.
- Applying the page state is idempotent: setting the same attribute state repeatedly produces no additional effects.
- No page-to-native bridge message or document reload is required, avoiding malformed-message and scroll-restoration failure modes.

## Testing

- A page/CSS test verifies that the fullwidth selector removes the width cap while the base rule retains the existing 46rem default and padding.
- Menu tests or source-level menu contract tests verify the Full Width label, Option-Command-F shortcut, checkable state, and action selector.
- Preference/state tests verify that an absent preference means centered view and stored values restore enabled or disabled state.
- Window/app tests verify that toggling persists the value and that load/reload paths queue restoration.
- The full test suite must remain green.

## Out of Scope

- Edge-to-edge content without padding.
- Automatic fullwidth behavior based on window size or document content.
- A toolbar or sidebar button.
- Per-document width preferences.
- Multiple width presets or a user-configurable maximum width.
