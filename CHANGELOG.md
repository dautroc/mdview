# Changelog

Release notes for MDView, organized around user-visible features, fixes, and
the commits that introduced them. The newest release is listed first.

## Unreleased

### What’s new

- **Monokai Pro theme:** a fourth dark appearance, sitting alongside the
  existing seven in the theme picker. It previews on hover, persists across
  launches like the others, and is selectable headlessly with
  `--theme monokai-pro`. Its palette ships as a tmTheme embedded in the
  binary rather than a syntect built-in, so the page background, window
  chrome, and diff colours are still derived from the one palette.

## [v0.5.0](https://github.com/dautroc/mdview/releases/tag/v0.5.0) — 2026-08-22

### Fixes

- The contextual toolbar now stays visible while scrolling and avoids
  overlapping the sidebar controls in either sidebar state.

## [v0.4.0](https://github.com/dautroc/mdview/releases/tag/v0.4.0) — 2026-08-22

### What’s new

- **Git diff view:** inspect the current open Markdown file against `HEAD` in
  unified or split layout, with syntax highlighting, source line numbers, and
  theme-aware additions, deletions, and hunks.
- **Contextual view toolbar:** switch between Markdown and diff views with
  compact controls that stay inside the document area, while Full Width remains
  available from the overflow menu.
- **Diff layout preference:** Unified/Split selection is remembered globally;
  diff mode remains local to each document window.

### Fixes

- Diff availability now stays synchronized during live reloads, including when
  a tracked file becomes unavailable or a stale diff view needs to be closed.

## [v0.3.0](https://github.com/dautroc/mdview/releases/tag/v0.3.0) — 2026-08-22

### What’s new

- **Fullwidth view:** expand the document beyond the centered reading column
  with **⌥⌘F** or **View → Full Width**. The preference is remembered across
  launches. ([5cda77c](https://github.com/dautroc/mdview/commit/5cda77c),
  [12f9258](https://github.com/dautroc/mdview/commit/12f9258),
  [56f9c41](https://github.com/dautroc/mdview/commit/56f9c41))

### Fixes

- Fullwidth state now survives reloads and navigation, including rapid toggles
  while a page is still loading. ([92e04b7](https://github.com/dautroc/mdview/commit/92e04b7),
  [01cc9b8](https://github.com/dautroc/mdview/commit/01cc9b8),
  [2fbb6aa](https://github.com/dautroc/mdview/commit/2fbb6aa),
  [d15eb7c](https://github.com/dautroc/mdview/commit/d15eb7c))

### Documentation

- Added the fullwidth shortcut to the keyboard reference. ([e9860fc](https://github.com/dautroc/mdview/commit/e9860fc))

## [v0.2.0](https://github.com/dautroc/mdview/releases/tag/v0.2.0) — 2026-08-22

### Build & release

- Releases are now cut automatically when the application version changes on
  `main`, with the Cargo, bundle, and lockfile versions kept together.
  ([87ed57d](https://github.com/dautroc/mdview/commit/87ed57d),
  [da273c3](https://github.com/dautroc/mdview/commit/da273c3))

## [v0.1.0](https://github.com/dautroc/mdview/releases/tag/v0.1.0) — 2026-08-22

The first public release: a read-only Markdown viewer for macOS that renders
CommonMark and GFM tables/task lists, syntax-highlighted code, images, LaTeX,
and Mermaid diagrams locally with all rendering assets embedded in the app.

### What’s new

- **Reading workflow:** open files from Finder, the Dock, drag-and-drop, or
  **⌘O**; reuse one window; keep a persisted recent-file history; and print
  rendered HTML from the command line. ([a605743](https://github.com/dautroc/mdview/commit/a605743),
  [7ef8199](https://github.com/dautroc/mdview/commit/7ef8199),
  [32d29ed](https://github.com/dautroc/mdview/commit/32d29ed),
  [39489c9](https://github.com/dautroc/mdview/commit/39489c9),
  [dd9da96](https://github.com/dautroc/mdview/commit/dd9da96))
- **Outline and bookmarks:** navigate headings in a collapsible sidebar and
  save documents for quick return. ([f045a9e](https://github.com/dautroc/mdview/commit/f045a9e),
  [c82aa13](https://github.com/dautroc/mdview/commit/c82aa13))
- **Themes:** choose named light and dark themes, preview them on hover, and
  keep the selected theme across launches. ([3dec054](https://github.com/dautroc/mdview/commit/3dec054),
  [3683f5a](https://github.com/dautroc/mdview/commit/3683f5a),
  [71921cf](https://github.com/dautroc/mdview/commit/71921cf))
- **Rich content:** click images and Mermaid diagrams to zoom them to the
  window, with KaTeX math and syntax-highlighted code rendered offline.
  ([d78c456](https://github.com/dautroc/mdview/commit/d78c456),
  [4196f20](https://github.com/dautroc/mdview/commit/4196f20),
  [acd710a](https://github.com/dautroc/mdview/commit/acd710a))
- **Live reload:** update the document after save while preserving the scroll
  position. ([eaf5472](https://github.com/dautroc/mdview/commit/eaf5472))

### Fixes

- Render a document’s own relative images correctly. ([a729738](https://github.com/dautroc/mdview/commit/a729738))
- Harden document loading, URL decoding, navigation policy, multi-file startup,
  theme rendering, and embedded-page security across the initial release.
  ([e4fa919](https://github.com/dautroc/mdview/commit/e4fa919),
  [eaf5472](https://github.com/dautroc/mdview/commit/eaf5472),
  [65175db](https://github.com/dautroc/mdview/commit/65175db),
  [2fc90fd](https://github.com/dautroc/mdview/commit/2fc90fd))

### Build & release

- Universal Apple silicon and Intel DMG, ad-hoc signed with first-launch
  Gatekeeper instructions. ([e600558](https://github.com/dautroc/mdview/commit/e600558))
