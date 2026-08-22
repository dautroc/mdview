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

- **Outline** built from the document's headings, in a sidebar you can toggle.
- **Bookmarks** for documents you come back to, kept across launches.
- **Themes** — three light, three dark, or follow the system. Hovering a name
  in the picker previews it on the document; only a click keeps it.
- **Click to zoom** an image or a Mermaid diagram to fill the window.
- **Live reload.** Save in your editor and the view updates, holding your
  scroll position.
- **One window.** Opening another document reuses the current window rather
  than scattering windows across the desktop.

## Install

Download `MDView-<version>.dmg` from the releases page and drag the app to
Applications. The build is universal, so one download covers Apple silicon and
Intel.

macOS will refuse to open it the first time. The app is signed ad-hoc rather
than with an Apple Developer ID, so it is not notarized, and Gatekeeper treats
anything downloaded without notarization as untrusted. Open **System Settings →
Privacy & Security**, find MDView near the bottom, and choose **Open Anyway**.
Control-clicking the app no longer works for this; macOS Sequoia removed that
shortcut. The equivalent from a terminal is:

```sh
xattr -d -r com.apple.quarantine /Applications/MDView.app
```

For the `mdview` command, link the shim the bundle carries:

```sh
ln -sf /Applications/MDView.app/Contents/Resources/mdview /usr/local/bin/mdview
```

There is no Homebrew cask. Homebrew dropped `--no-quarantine` in 4.7 and stops
supporting casks that fail Gatekeeper on 1 September 2026, so a cask for an
unnotarized app would break almost immediately. Notarizing needs a paid Apple
Developer account; if that changes, a cask becomes worthwhile.

## Use

```sh
mdview notes.md               # open a file
mdview --print-html notes.md  # render to stdout
```

Or double-click a `.md` file in Finder, drop one on the window or the Dock
icon, or press ⌘O.

| Key | |
| --- | --- |
| ⌘O | Open a file |
| ⌘R | Reload |
| ⌥⌘S | Toggle the sidebar |
| ⌘T | Next theme |
| ⌘D | Bookmark this document |
| ⌘= ⌘- ⌘0 | Zoom in, out, actual size |
| ⌘W | Close the window |

## Build

```sh
make test
make install       # /Applications/MDView.app
make install-cli   # /usr/local/bin/mdview
```

Requires a Rust toolchain and the macOS command line tools. Building the app
needs no Swift toolchain; the icon is committed.

## Layout

- `crates/mdcore` — Markdown to self-contained HTML. Pure safe Rust, no
  AppKit, and where most of the tests live.
- `crates/mdapp` — the AppKit shell: windows, menus, file handling. The only
  crate with `unsafe`.
- `tools/shot.swift` — renders a generated page in a WKWebView and writes a
  PNG.
- `tools/icon.swift` — draws the app icon. `bundle/MDView.icns` is committed,
  so building needs no Swift toolchain; run `make icon` after editing it.

Vendored assets are refreshed with `python3 scripts/vendor-assets.py`.

## Releasing

`make dist` runs the tests, builds both architectures into one universal
binary, packages the bundle and writes `dist/MDView-<version>.dmg` with its
SHA-256. Pushing a `v*` tag runs the same thing in CI and attaches the DMG to a
GitHub release.

The version lives in `Cargo.toml`; `bundle/Info.plist` carries its own copy and
a test fails if the two drift, since nothing at runtime would catch it.

## Looking at the UI

The interface is HTML in a web view, so its layout bugs tend to be invisible
in the markup and obvious on screen: a button painted under a sticky banner, a
selected tab whose fill matches its background, a menu clipped by an ancestor's
`overflow`. `make shot` renders a page through the same WebKit the app embeds
and writes a PNG:

```sh
make shot FILE=README.md
make shot FILE=notes.md THEME=mocha SIDEBAR=1 WIDTH=520 HEIGHT=420
make shot FILE=notes.md JS='document.getElementById("mdview-theme").open=true'
```

`SIDEBAR=1` opens the panel, which a freshly generated page keeps hidden
because the app normally opens it over the message bridge. `JS` runs after the
load and before the snapshot, which is how you reach states the page does not
start in — opening the theme menu, dispatching a hover.

What it does not cover: the bridge itself. Live reload, persistence, the native
menu and window behaviour still have to be checked in the running app.
