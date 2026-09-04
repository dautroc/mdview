# MDView

A read-only Markdown viewer for macOS. Rust and AppKit, with WebKit used only
as a paint surface.

![MDView showing a document with its outline in the sidebar and its minimap down the right edge](docs/screenshot.png)

**[Take the tour](docs/tour.md)** — what each of these looks like in use,
grouped by what you are doing at the time.

Point it at a file and read. There is no editor, no preview pane to keep in
sync, and no server: every asset is embedded in the binary, so it renders the
same with the network off.

## What it renders

CommonMark plus GFM tables and task lists, syntax-highlighted code, images,
LaTeX math, and Mermaid diagrams.

A YAML or TOML frontmatter block at the top of a file is addressed to whatever
tool wrote it, not to you, so MDView takes it off before rendering. A `---`
anywhere else in the document is still a rule, and so is one at the top of a
file that never closes it.

## What it does

- **No buttons.** The window is the document. Everything is a key, and the
  macOS menu bar carries the same commands for a mouse. Press **?** for the
  list; the app offers it once, on first launch.
- **Outline** built from the document's headings, in a sidebar you can toggle
  and drag to resize; the width is remembered across launches. The heading you
  are reading is lit as you scroll, and the list keeps it in view, so the
  outline says where you are and not just what there is.
- **Minimap.** `g m` puts the shape of the whole document down the right edge:
  headings as bars, prose as lines, code and tables as blocks, pictures as
  frames, with your comments and find matches marked on it. The band shows the
  part you are reading; click it to jump, drag it to scrub. It floats over the
  margin rather than taking width from the text, so turning it on does not
  re-wrap what you are reading.
- **Bookmarks** for documents you come back to, kept across launches.
- **Comments.** Select a phrase — with the mouse, or with `v` and a motion —
  press `c`, and type a note. The note becomes a
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
- **Diff against HEAD.** `g d` on a file Git tracks shows what has changed
  since the last commit, and `g l` lays it out four ways: the source or the
  document, in one column or two. The source layouts go line by line, which is
  the view for checking a table's pipes. Rendered shows the document — the
  prose, as it renders — with a bar beside every block that changed and the
  version that was there before folded away under it, one click from being
  read; the outline and the minimap keep working there, because it is still the
  document. Rendered split puts the two documents side by side instead, a row
  to a block, which is the one to reach for when a page has been rewritten
  rather than edited.
- **Recent files.** `g r` opens the last fifty documents in a palette you can
  type into — a few letters of the name, or of the folder it is in, narrows the
  list. The document you are reading is not in it, so `g r` and enter is
  "back to the one before this". File > Open Recent carries the same list for a
  mouse.
- **Commands by name.** `:` opens a palette of what this app does, with the
  key for each one printed beside it — type a few letters of what you want,
  enter runs it, and you leave knowing the key. The vim alphabet is not in it:
  `j`, `w`, `/` and `y` are keys you already have, and listing them would bury
  the commands that are MDView's own. `?` still prints the lot.
- **Themes** — three light, five dark, or follow the system. `g t` opens a
  palette you can type into; arrowing through it previews each theme on the
  document, and only enter keeps one. The document takes the palette's own
  colours, not just the code inside it: headings, links, inline code, emphasis
  and table headings all come from what the theme says about Markdown, so a
  page reads the way it does in the editor the theme came from.
- **Find in page** with `/` or ⌘F: every match is highlighted as you type,
  `n` and `N` step through them, and esc clears the highlights.
- **A cursor, in the vim idiom.** `h`/`j`/`k`/`l` move it, `w`/`e`/`b` by the
  word, `^`/`$` to the ends of the line, `]`/`[` between headings, `/` to
  search, and `s` to jump anywhere on screen: type what you are looking at and
  every occurrence lights up, the nearest ones labelled — keep typing to narrow
  it down, or type a label to go there. A label is never a letter that could
  continue what you are typing, so it is never ambiguous which you meant, and
  the last match standing carries no label at all: enter takes it. The view follows the cursor, and
  the cursor follows the view: `⌃e`/`⌃y` leave it where it is while it is
  still on screen, and a scroll that would carry it off — a wheel, a half
  page, a drag on the minimap — brings it to the edge you scrolled towards,
  so `j` carries on from what you are reading. It holds its place across a
  save, anchored to its section, so editing elsewhere in the document does not
  move it. `?` lists the lot.
- **Click to zoom** an image or a Mermaid diagram to fill the window, or press
  `z` for whichever one you are looking at. Once it is zoomed, the arrows and
  `h`/`j`/`k`/`l` pan it.
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

Vim-flavoured, since nothing in a read-only viewer is expecting your typing.
Moving around is a single key; the commands that are not motion sit behind `g`,
which keeps the vim alphabet free for the document itself. Press **?** for the
full list, which the app also offers once on first launch, or **:** to search
the commands that are this app's own — the ones a vim alphabet does not
already cover — and run what you find.

| Key | |
| --- | --- |
| h / j / k / l | Move the cursor left / down / up / right |
| w / b | Forward / back a word (W / B by whitespace alone) |
| e | To the end of the word (E by whitespace alone) |
| s | Jump to anything on screen — type what you see, then its label |
| ^ / $ | Start / end of the line |
| v / V | Select from the cursor / whole blocks |
| o | Swap which end of the selection you are moving |
| y | Copy the selection |
| g g / G | Top / bottom of the document |
| d / u | Half a page down / up (⌃d / ⌃u too) |
| ⌃f / ⌃b | A page down / up |
| ⌃e / ⌃y | A line down / up, leaving the cursor where it is |
| ] / [ | Next / previous heading |
| } / { | Next / previous top-level heading |
| / | Find in this document |
| enter | Search, and hand the keyboard back to the document |
| n / N | Next / previous match (enter / ⇧enter too) |
| g s | Toggle the sidebar |
| g m | Toggle the minimap |
| g o / g b | Outline / bookmarks in the sidebar |
| m | Bookmark this document |
| c | Comment on the selection, or show the comments |
| ) / ( | Next / previous comment |
| g c / x | Edit / delete the comment you are looking at |
| C | Copy the review prompt for Claude |
| g t | Themes |
| g r | Recent files |
| g d | Diff, and back to Markdown (needs a tracked file) |
| g l | Diff layout: source or rendered, one column or two |
| z | Zoom the nearest image or diagram |
| g w | Toggle fullwidth view |
| r | Reload |
| + / − / 0 | Zoom in / out / actual size |
| : | Run a command by name |
| ? | The list of all of them |

## Working on it

```sh
make test      # cargo test
make bundle    # MDView.app
make dist      # a universal DMG
```

Two tools render the interface outside the app, because most of it is HTML and
looking at it beats inferring it from the stylesheet:

```sh
make shot FILE=notes.md THEME=mocha SIDEBAR=1   # one page, one PNG
make reels                                       # the GIFs in docs/tour.md
make reel REEL=finding                           # just one of them
```

`make reels` drives the same generated page through a list of steps in
`reels/*.json` and assembles the frames with ffmpeg, which is a developer's
dependency and nothing else — it is not in the binary and not in the bundle.
The demos are therefore rebuilt when the interface changes rather than
re-recorded by hand, and the state they show is invented in the spec files
rather than read off anybody's disk.
