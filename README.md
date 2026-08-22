# MDView

A read-only macOS Markdown viewer. Rust and AppKit, with WebKit used only as
a paint surface.

Renders CommonMark plus GFM tables and task lists, syntax-highlighted code,
images, LaTeX math, and Mermaid diagrams. Everything is embedded in the
binary, so it works with no network access.

## Build

    make test
    make install       # /Applications/MDView.app
    make install-cli   # /usr/local/bin/mdview

## Use

    mdview notes.md              # open a file
    mdview --print-html notes.md # render to stdout

Or double-click a `.md` file in Finder, drop one on the window or the Dock
icon, or press ⌘O.

The open document reloads automatically when an external editor saves it,
keeping your scroll position.

## Layout

- `crates/mdcore` — Markdown to self-contained HTML. Pure safe Rust, no
  AppKit, and where all the tests live.
- `crates/mdapp` — the AppKit shell: windows, menus, file handling.
- `tools/shot.swift` — renders a generated page in a WKWebView and writes a
  PNG.

Vendored assets are refreshed with `python3 scripts/vendor-assets.py`.

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
