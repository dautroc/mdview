# MDView

A read-only Markdown viewer for macOS. Rust and AppKit, with WebKit used only
as a paint surface.

![MDView showing a document with its outline in the sidebar](docs/screenshot.png)

Point it at a file and read. There is no editor, no preview pane to keep in
sync, and no server: every asset is embedded in the binary, so it renders the
same with the network off.

## What it renders

CommonMark plus GFM tables and task lists, syntax-highlighted code, images,
LaTeX math, and Mermaid diagrams.

## What it does

- **No buttons.** The window is the document. Everything is a key, and the
  macOS menu bar carries the same commands for a mouse. Press **?** for the
  list; the app offers it once, on first launch.
- **Outline** built from the document's headings, in a sidebar you can toggle
  and drag to resize; the width is remembered across launches.
- **Bookmarks** for documents you come back to, kept across launches.
- **Comments.** Select a phrase, press `c`, type a note. The note becomes a
  card in the document's right margin, level with the passage it is about;
  clicking a highlighted passage brings its card forward, hovering a card lights
  up its passage and reveals its edit and delete buttons, and clicking one
  jumps to the passage. When the window is too narrow for the margin the cards
  live in the sidebar instead. `C` copies a prompt that points Claude at the
  review file holding them, and asks it to delete each comment it has addressed
  — MDView watches that file, so a comment leaves the margin as soon as it is
  dealt with. A comment whose passage was rewritten instead stays in the
  sidebar, struck through, rather than vanishing with the words it was about.
  If the review file ends up in a state MDView cannot wholly read, it says so
  and stops writing to it rather than saving over the part it did not
  understand.
- **Themes** — three light, four dark, or follow the system. `t` opens a
  palette you can type into; arrowing through it previews each theme on the
  document, and only enter keeps one.
- **Find in page** with `/` or ⌘F: every match is highlighted as you type,
  `n` and `N` step through them, and esc clears the highlights.
- **Keyboard navigation** in the vim idiom — `j`/`k` by the line, `d`/`u` by
  the half page, `]`/`[` between headings, `/` to search. `?` lists the lot.
- **Click to zoom** an image or a Mermaid diagram to fill the window, or press
  `z` for whichever one you are looking at.
- **Live reload.** Save in your editor and the view updates, holding your
  scroll position.
- **One window.** Opening another document reuses the current window rather
  than scattering windows across the desktop.

## Install

Grab `MDView-<version>.dmg` from the releases page and drag it to
Applications. Universal binary, so one download covers Apple silicon and
Intel.

It is ad-hoc signed, not notarized, so Gatekeeper blocks the first launch:

```sh
xattr -d -r com.apple.quarantine /Applications/MDView.app
```

Or **System Settings → Privacy & Security → Open Anyway**. Control-clicking no
longer works for this; Sequoia removed that shortcut. There is no Homebrew
cask, because Homebrew stops supporting casks that fail Gatekeeper on
1 September 2026 and notarizing needs a paid Apple Developer account.

For the `mdview` command:

```sh
ln -sf /Applications/MDView.app/Contents/Resources/mdview /usr/local/bin/mdview
```

Building from source instead:

```sh
make install       # /Applications/MDView.app
make install-cli   # /usr/local/bin/mdview
```

## Use

```sh
mdview notes.md               # open a file
mdview --print-html notes.md  # render to stdout
```

Or double-click a `.md` file in Finder, drop one on the window or the Dock
icon, or press ⌘O.

Everything is a single key, vim-flavoured, since nothing in a read-only viewer
is expecting your typing. Press **?** for the full list, which the app also
offers once on first launch.

| Key | |
| --- | --- |
| j / k | Down / up a line |
| d / u | Half a page down / up |
| space / ⇧space | A page down / up |
| gg / G | Top / bottom of the document |
| ] / [ | Next / previous heading |
| } / { | Next / previous top-level heading |
| / | Find in this document |
| enter | Search, and hand the keyboard back to the document |
| n / N | Next / previous match (enter / ⇧enter too) |
| s | Toggle the sidebar |
| o / b | Outline / bookmarks in the sidebar |
| m | Bookmark this document |
| c | Comment on the selection, or show the comments |
| ) / ( | Next / previous comment |
| click | Focus the comment on a highlighted passage |
| e / x | Edit / delete the comment you are looking at |
| C | Copy the review prompt for Claude |
| t | Themes |
| D | Diff, and back to Markdown |
| l | Diff layout, unified or split |
| z | Zoom the nearest image or diagram |
| w | Toggle fullwidth view |
| r | Reload |
| + / − / 0 | Zoom in / out / actual size |
| ? | The list of all of them |
