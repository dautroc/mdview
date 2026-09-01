# Changelog

Release notes for MDView, organized around user-visible features, fixes, and
the commits that introduced them. The newest release is listed first.

## [v0.11.0](https://github.com/dautroc/mdview/releases/tag/v0.11.0) — 2026-09-01

### What's new

- **The keys moved to make room for a cursor.** `w`, `e` and `b` are how vim
  moves by word and `s` is how it jumps, and all four were sitting on MDView
  commands. So the commands moved instead, behind `g`: `g s` toggles the
  sidebar, `g o` and `g b` are its panels, `g t` the themes, `g w` full width,
  `g d` the diff and `g l` its layout, `g c` edits the comment you are looking
  at. That is the answer vim itself gives for everything that does not fit on
  one key, and it leaves the alphabet free for the document rather than the
  chrome. `gg` stopped being a special case in the dispatcher at the same time
  and became an ordinary row in the table with everything else.
- **Paging is `⌃f` and `⌃b`, half a page `⌃d` and `⌃u`.** Vim's own bindings.
  This is the first time the page reads a modifier at all, so the find field
  keeps ⌃d and ⌃u as the text-editing keys macOS makes them: the page only sees
  them when the keyboard is not in a field. Space, `d` and `u` are now unbound
  and stay that way — space is held back as a leader key, and `d` and `u` are
  vim's delete and undo, which a viewer with no editing has no use for.
- **A pressed `g` waits, and swallows whatever follows it.** With eight
  commands behind the prefix, a mistyped one must not fall through and fire an
  unrelated command — `g` then `x` used to delete a comment. It waits a beat
  for the key that completes it, and esc cancels it outright.
- **There is a cursor.** `h`, `j`, `k` and `l` move it, `w`, `e` and `b` move
  it by the word — `W`, `E` and `B` by whitespace alone — and `^` and `$` go to
  the ends of the line. The view follows it rather than the other way round, so
  reading is now driving a position through the document rather than pushing a
  page around; `⌃e` and `⌃y` scroll without taking the cursor with them when
  you want to look ahead and come back.
- **The cursor is a place in the document, not in the page.** It is held as an
  offset into the document's text and re-found after every render, because
  highlighting a comment or a find match splits the page's text nodes apart and
  merges them back again — a cursor that pointed at one of those nodes would be
  pointing at nothing the moment you searched. It is also remembered against
  the section it sits in, the way a comment is, so saving an edit somewhere
  else in the document leaves it where you left it.
- **Words end where blocks do.** The text a motion walks is the document's
  text run together, and nothing separates the cells of a table row: `| one |
  two |` reads as `onetwo`. Motions now know where each block begins, so `w`
  stops between two cells rather than stepping over the join as though it were
  a single word.
- **`v` selects, and `c` comments on what it selected.** Commenting was the
  one thing in MDView that a keyboard could not do: `c` was a key whose
  precondition was a mouse gesture, because the only way to make a selection
  was to drag one. `v` starts a selection at the cursor and every motion
  extends it, `V` takes whole blocks, `o` swaps which end you are moving, `y`
  copies, and esc leaves. `V` then `c` is how you comment on a paragraph.
- **The selection is the real one.** It is the same selection a drag makes, so
  `c` did not have to learn anything new — it reads what it has always read.
  That also means nothing new is drawn on the document: the two highlight
  layers already there, comments outside and find matches inside, are ordered
  against each other so that closing find cannot strip a comment, and a third
  layer would have had nowhere to sit.
- **Opening find or the theme palette leaves the selection deliberately.**
  Focusing a text field collapses the document selection, so rather than watch
  it come apart, the commands that take focus stand the mode down first. `/`
  after `v` opens an empty find box, the way it does in vim, while `/` after a
  double-click still seeds from the words you picked.
- **`s` jumps anywhere you can see.** Press `s` and type what you are looking
  at. Every occurrence on screen lights up as you type and the nearest ones
  take a label: keep typing to narrow the field, or type a label to go there.
  Backspace takes a character back, enter takes the nearest match, esc gives
  up. In visual mode the jump extends the selection rather than moving it, so
  selecting an awkward phrase is `v` and one jump.
- **A label is never a letter that could continue the search.** That is the
  whole reason a single keystroke can mean either "narrow this down" or "go
  there" without a mode to switch between them: the letters that follow the
  current matches are struck out of the label alphabet before any are handed
  out, so there is nothing left to guess. Searching for `th` in a paragraph
  full of *the*, *that* and *thing* hands out `s d f g h j k l` and never `a`,
  `e` or `i`.
- **Narrowing to one match does not jump on its own.** Typing is how you got
  there, so typing keeps working: the last match standing carries no label to
  read instead, and enter takes it. Going the moment a query happened to be
  unique ended the jump mid-word, and every remaining letter of the word you
  were still typing ran as a command — which reads exactly like the search
  resetting itself.

### Fixes

- **Commenting on more than one line said the selection crossed a paragraph
  break, from well inside one paragraph.** A paragraph written across two
  source lines — which is most of them — keeps that line break inside the
  document as a real newline, while the selection reports the text as it is
  drawn, with a space in its place. The words being anchored were therefore a
  string the document did not contain, and the search for them failed. The
  quote is now read out of the document's own text rather than out of the
  selection, so it is findable by construction. A triple-clicked paragraph goes
  the same way, matching whitespace to whitespace.
- **A comment may now span a paragraph break.** It was never the intent that it
  could not — the question a selection has to answer is whether its words can
  be found again, and with the quote taken from the document that answer is now
  yes. The refusal is kept for words that genuinely cannot be located.

## [v0.10.0](https://github.com/dautroc/mdview/releases/tag/v0.10.0) — 2026-09-01

### What's new

- **Comments.** Select a phrase, press `c`, type a note. The passage is
  highlighted where it sits, the note joins a list in the sidebar beside the
  outline and bookmarks, and both come back next launch. `e` edits the comment
  you are looking at and `x` deletes it — refusing when the nearest one is more
  than a screen from centre, since there is no undo and you should not be able
  to delete something you never saw. `)` and `(` step between them.
- **Comments sit in the margin.** Each note is a card in the document's right
  margin, level with the passage it is about and pushed down only where two
  would overlap. Clicking a highlighted passage brings its card forward and
  clicking a card jumps to the passage. The rail lives inside the document
  column, which is as tall as the document, so the cards scroll with the text
  rather than being fed a scroll position. It stands down when the margin is
  too narrow — reserving space would re-wrap the text and move your place in
  the document every time a comment was added — and the sidebar panel is then
  the way to read them, as it always is for comments whose text was edited away.
- **Anything you can select, you can comment on.** A comment is anchored by its
  words, so the only question asked of a selection is whether those words can
  be found again: a double-clicked word, a triple-clicked paragraph, a
  multi-line code block, or a phrase running through bold, a link or inline
  code all anchor. Only a selection crossing a paragraph break is refused, and
  it says so. The words are trimmed first, so the whitespace a double-click
  takes with them is not part of the anchor.
- **Comments survive editing.** An anchor is re-found after every live reload
  by its quoted words, scoped to the section it was made in, so a save
  elsewhere in the document does not move it. A comment whose words were edited
  away keeps its place in the list, struck through, rather than vanishing.
- **`C` hands the whole review to Claude.** It copies a one-line prompt naming
  the review file, and asks for each comment to be deleted from that file once
  it has been addressed. That last part matters: addressing a comment usually
  means rewriting the exact passage it quotes, which would otherwise leave every
  comment behind, struck through, to be cleared one `x` at a time. MDView
  watches the review file, so a comment leaves the margin as soon as its record
  goes.
- **A review MDView cannot wholly read is never written over.** Writing renders
  the comment list and nothing else, so a record the parser had to skip would be
  erased by the next save. Every write is gated on the file reading cleanly
  instead, and a banner names the line, the reason and the consequence, clearing
  itself as soon as the file parses again. Three things count as unreadable: a
  `mdview-quote` line missing its numbers, a `mdview-note` block with no comment
  to attach to, and a record whose closing fence is gone — which is invisible
  from the outside, because the *next* record's fence closes it and the two
  silently become one. A file cut short mid-write is still readable, and still
  written to.
- **Reviews are stored where MDView owns.** Under Application Support, keyed by
  the document's path, not beside the document — MDView is pointed at files on
  read-only volumes and inside app bundles, and a sibling file would land in
  `git status` for every document anyone commented on.

### Fixes

- **`D` says why it will not open the diff.** It refused in silence whenever
  there was no Git diff to show, which is every file outside a repository —
  so on those files the key read as simply broken. In an app where `x`, `c` and
  `C` all explain themselves, silence was the wrong answer, and the greyed-out
  View menu item is no help to someone using the keyboard. It now names which
  of the three it is: not in a repository, not tracked, or no commits yet.
  Leaving the diff is still always allowed.

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
