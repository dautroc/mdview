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

Vendored assets are refreshed with `python3 scripts/vendor-assets.py`.
