# Changelog

Release notes for MDView, organized around user-visible features, fixes, and
the commits that introduced them. The newest release is listed first.

## Unreleased

### Fixes

- **`D` says why it will not open the diff.** It refused in silence whenever
  there was no Git diff to show, which is every file outside a repository —
  so on those files the key read as simply broken. In an app where `x`, `c` and
  `C` all explain themselves, silence was the wrong answer, and the greyed-out
  View menu item is no help to someone using the keyboard. It now names which
  of the three it is: not in a repository, not tracked, or no commits yet.
  Leaving the diff is still always allowed.

### What's new

- **Comments.** Select a phrase, press `c`, type a note. The passage is
  highlighted where it sits, the note joins a list in the sidebar beside the
  outline and bookmarks, and both come back next launch. `e` edits the comment
  you are looking at and `x` deletes it — refusing when the nearest one is off
  screen, since there is no undo. `C` copies a one-line prompt naming the
  review file, to paste into Claude.
- **Comments sit in the margin.** Each note is a card in the document's right
  margin, level with the passage it is about and pushed down only where two
  would overlap. Clicking a highlighted passage brings its card forward and
  clicking a card jumps to the passage. The rail lives inside the document
  column, which is as tall as the document, so the cards scroll with the text
  rather than being fed a scroll position. It stands down when the margin is
  too narrow — reserving space would re-wrap the text and move your place in
  the document every time a comment was added — and the sidebar panel is then
  the way to read them, as it always is for comments whose text was edited away.
- **Any selection you can re-find can hold a comment.** `c` used to decide
  whether a selection was anchorable by walking up the DOM for a paragraph-ish
  tag and refusing when the two ends disagreed, which turned down ordinary
  double-clicks — a word selection often starts at the end of the text node
  before the one you clicked in. It now asks the question that actually
  matters, and the one `applyCommentAnchors` will ask on every render: can
  these words be found again in the document text? Whole paragraphs and
  multi-line code now anchor too, and a selection that really does cross a
  paragraph break says so. The words are trimmed first, so the whitespace a
  double-click takes with them is not part of the anchor.
- **`C` says "copied" the way every other key does.** It first reported through
  a banner, which is the app's way of raising a condition someone has to deal
  with: it sits in the corner until it is clicked, so pressing the shortcut
  twice left two of them stacked up. Copying is news that expires, so it now
  uses the same short notice in the top-right corner that bookmarking and the
  other keys use, and it takes itself away. A review that could not be *written*
  is still a banner, because that one does want attention.
- **A review Claude has acted on empties itself.** Addressing a comment usually
  means rewriting the exact passage it quotes, so the natural end state of `C`
  working was a document full of comments whose text no longer existed — struck
  through, one `x` each to clear. The prompt now asks for each addressed record
  to be deleted from the review file, spelling out which fenced blocks make one
  up so a half-deleted record cannot end up unparseable. MDView watches the
  review file, so a comment leaves the margin as soon as its record goes. It is
  the only thing that would notice: comments were re-read on open and after
  MDView's own writes, and an edit by anyone else sat unseen until the document
  was reloaded for some unrelated reason.
- **A review MDView cannot wholly read is never written over.** Writing renders
  the comment list and nothing else, so a record the parser had to skip was
  erased by the next save — silently, since skipping was the documented way to
  survive a bad file. That was survivable while nothing else edited the file;
  asking Claude to delete records makes a half-deleted one the likely failure,
  and a half-deleted record is exactly what the parser skips. Every write is now
  gated on the file reading cleanly, and a banner names the line, the reason and
  the consequence. It clears itself as soon as the file parses again. Three
  things count as unreadable: a `mdview-quote` line missing its numbers, a
  `mdview-note` block with no comment to attach to, and a record whose closing
  fence is gone — which is invisible from the outside, because the *next*
  record's fence closes it and the two silently become one.
- **Comments survive editing.** An anchor is re-found after every live reload
  by its quoted words, scoped to the section it was made in, so a save
  elsewhere in the document does not move it. A comment whose words were edited
  away keeps its place in the list, struck through, rather than vanishing.
- **Reviews are stored where MDView owns.** Under Application Support, keyed by
  the document's path, not beside the document — MDView is pointed at files on
  read-only volumes and inside app bundles, and a sibling file would land in
  `git status` for every document anyone commented on.

## [v0.9.1](https://github.com/dautroc/mdview/releases/tag/v0.9.1) — 2026-09-01

### Fixes

- **Bookmarking works from the keyboard again.** `m` toggled the bookmark in
  storage but the page never heard about it: the callback the app runs after a
  toggle called a `showNote` that did not exist, so it threw before the star
  state or the bookmarks list could update. With the sidebar shut the key
  looked dead, and with it open the list did not move.
- **Bookmarking now says so.** Toggling raises a short notice in the top-right
  corner — “Bookmarked” or “Bookmark removed” — which steps aside when the
  sidebar is open so it never covers the list it is reporting on.

## [v0.9.0](https://github.com/dautroc/mdview/releases/tag/v0.9.0) — 2026-08-31

### What’s new

- **Vim-flavoured keys for everything.** Nothing in a read-only viewer is
  expecting your typing, so every command has a single key: `j`/`k` by the
  line, `d`/`u` by the half page, space and ⇧space by the page, `gg`/`G` to the
  ends, `]`/`[` between headings and `}`/`{` between top-level ones, `/` and
  `n`/`N` to search, `s` `o` `b` `m` `t` `D` `l` `z` `w` `r` and `+`/`−`/`0`
  for the rest. Press **?** for the list, which the app offers once on first
  launch. Motion is instant and jumps are smooth, so a held `j` does not queue
  animations that fight each other.
- **The page draws no controls at all.** The toolbar, the find bar's buttons,
  the sidebar's tabs and star, the sidebar toggle and the zoom badge are gone —
  every one of them duplicated a key, and between them they cost a fixed
  toolbar that re-offset itself by the sidebar width and a seven-level z-index
  ladder. The menu bar is the mouse's route to all of it.
- **A theme palette on `t`.** Type to filter, arrow to move, and every move
  previews the theme on the document — the thing a native menu cannot do, and
  the reason the palette exists alongside the new View ▸ Theme submenu rather
  than deferring to it. The submenu is checkmarked at draw time, so a theme
  picked from the page cannot leave the menu stale.

### Fixes

- **The diff layout and the lightbox keep a route in.** `l` cycles unified and
  split, which had lived only on two buttons, and `z` opens the nearest image
  or diagram, since dropping the zoom badge would otherwise have left the
  lightbox mouse-only. A zoom-in cursor keeps the affordance without drawing a
  control.

### Documentation

- The screenshot was retaken from the running app: the old one still showed the
  toolbar, sidebar tabs, star and hamburger, none of which exist.
- Only ⌘O, ⌘F and ⌘R keep a key equivalent. Every other menu command now has a
  single key in the page, so its shortcut was a second binding to keep in sync
  with the first — and the pair had already drifted once, when ⌘D bookmarked
  while ⌥⌘D toggled the diff. The macOS standards (⌘C, ⌘A, ⌘Q, ⌘W, ⌘M, ⌘H)
  stay; they duplicate nothing, and ⌘C is the only way to copy out of a
  WKWebView at all.

## [v0.8.0](https://github.com/dautroc/mdview/releases/tag/v0.8.0) — 2026-08-31

### What’s new

- **Find in page:** ⌘F opens a find bar over the document, matching as you
  type. Every match is highlighted, the current one is picked out in the
  theme's accent colour, and ⌘G / ⇧⌘G (or return / shift-return in the field)
  step through them with wraparound. Esc closes the bar and clears the
  highlights. Search covers prose, tables, and code — including the diff view —
  but not the interior of a rendered Mermaid diagram.

## [v0.7.0](https://github.com/dautroc/mdview/releases/tag/v0.7.0) — 2026-08-31

### What’s new

- **Resizable sidebar:** drag the divider between the document and the sidebar
  to set its width, or focus the divider and nudge it with the arrow keys. The
  width is clamped to 160–600px, remembered across launches, and re-applied on
  navigation and live reload. The toolbar keeps clear of the sidebar at any
  width.

## [v0.6.0](https://github.com/dautroc/mdview/releases/tag/v0.6.0) — 2026-08-24

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
