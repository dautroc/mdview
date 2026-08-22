# Click-to-zoom overlay for diagrams and images

## What was built

Notion-style click-to-zoom for Mermaid diagrams and images, implemented entirely
in the two asset files that are `include_str!`'d into `mdcore` at compile time,
plus one new test:

- `crates/mdcore/assets/init.js`
  - `renderDiagrams()` now always returns a Promise (resolved immediately when
    `mermaid` is undefined, or when `mermaid.run()` throws synchronously or its
    promise rejects). `mdviewRenderAll` chains `renderDiagrams().then(enhanceZoomables)`.
  - `enhanceZoomables()` walks `#mdview-content` for `pre.mermaid` (using its
    `svg` child if `mermaid.run` has already produced one, else the `pre.mermaid`
    element itself) and for `<img>` elements, wrapping each in a
    `.mdview-zoomable` `<span>` carrying a `.mdview-zoom-btn` button.
  - Idempotency guard: `wrapZoomable()` marks the wrapped node itself with
    `data-mdview-zoom="1"` and returns immediately if that attribute is already
    present, so re-running `mdviewRenderAll` (which happens after every
    live-reload save) is a no-op for anything already enhanced. No nesting,
    no duplicate buttons.
  - `#mdview-lightbox` is created lazily on first zoom click and appended to
    `document.body`, outside `#mdview-content`, so a live-reload
    `innerHTML` swap of the content div cannot destroy it mid-use.
  - Opening the overlay clones the target node (`cloneNode(true)`) into the
    overlay's stage; the original stays in place in the document.
  - Zoom state `{scale, x, y}` is applied as `transform: translate(x,y) scale(s)`
    on the stage's inner element (`transform-origin: 0 0`), so wheel/pinch zoom
    about the cursor point, and the +/- buttons zoom about the stage center,
    share one `zoomAbout(cx, cy, factor)` function.
  - Wheel handling: a single `wheel` listener (registered `{ passive: false }`)
    handles both ordinary scroll-wheel and macOS trackpad pinch (which arrives
    as a `wheel` event with `ctrlKey: true`) through the same code path, and
    calls `preventDefault()` so the page behind doesn't scroll and the app
    doesn't page-zoom.
  - Panning: `mousedown` on the stage starts a drag; `mousemove`/`mouseup` are
    attached to `document` for the duration of the drag so panning still
    tracks the cursor outside the stage's bounds.
  - Scale is clamped to `[0.25, 8]`.
  - Dismiss on Esc (`keydown` listener added while open, removed on close),
    the close button, or a click whose `event.target === overlay` (i.e. lands
    on the backdrop itself, not on the stage or its content/controls, since
    those are separate elements the click would target instead).
  - Body scroll is locked (`document.body.style.overflow = "hidden"`) while
    open and the previous `overflow` value plus `window.scrollY` are restored
    exactly on close, so opening/closing the overlay does not move the page.
  - Every handler is wired with `addEventListener` — no inline `onclick=`/
    `onerror=` attributes anywhere, required because the page's CSP is
    `script-src 'nonce-…'`.
- `crates/mdcore/assets/page.css`
  - `.mdview-zoomable` / `.mdview-zoomable-inline` wrapper styles, a
    `.mdview-zoom-btn` hidden until `:hover`/`:focus-visible` on the wrapper,
    and `#mdview-lightbox` / `.mdview-lightbox-*` overlay styles.
  - All colors reuse the existing custom properties (`--bg`, `--fg`,
    `--border`, `--code-bg`); no new color literals were introduced except the
    backdrop's `rgba(0,0,0,.6)` scrim, which has no existing token and is
    theme-neutral (a dark backdrop reads correctly in both light and dark
    mode).
  - Controls use plain text glyphs (`−`, `+`, `↺`, `⤢`, `×`) — no external
    icons or fonts.
- `crates/mdcore/src/page.rs`
  - Added `zoom_affordances_are_embedded_in_the_page`, exactly as specified,
    asserting the CSS/JS markers reach `build_page`'s output and that no
    external `<link>`/`src="http` leaked in.

## Handling the two required failure modes

1. **`mermaid.run()` is async.** `renderDiagrams()` is now Promise-returning
   in every branch (no-mermaid, synchronous throw, and the real async path,
   whose rejection is also caught so the returned promise never rejects).
   `mdviewRenderAll` calls `enhanceZoomables` only in `.then(...)`, after
   diagrams exist (or after mermaid has definitively failed/is absent), so
   diagrams get their zoom buttons and a mermaid failure never costs images
   theirs — `enhanceZoomables` scans for both `pre.mermaid`/`svg` and `<img>`
   in the same pass regardless of how mermaid fared.
2. **Idempotency.** Every node `enhanceZoomables` touches (an `<img>`, or the
   `svg`/`pre.mermaid` it picks for a diagram) is marked with
   `data-mdview-zoom="1"` before being wrapped, and `wrapZoomable` bails out
   immediately if that attribute is already set. Running `mdviewRenderAll`
   twice back to back — the real shape of every live-reload save — wraps
   nothing a second time: no nested wrappers, no duplicate buttons.

## RED (test written, assets not yet updated)

```
test page::tests::zoom_affordances_are_embedded_in_the_page ... FAILED

failures:

---- page::tests::zoom_affordances_are_embedded_in_the_page stdout ----

thread 'page::tests::zoom_affordances_are_embedded_in_the_page' (8294216) panicked at crates/mdcore/src/page.rs:248:9:
zoom CSS missing
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    page::tests::zoom_affordances_are_embedded_in_the_page

test result: FAILED. 29 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s

error: test failed, to rerun pass `-p mdcore --lib`
```

## GREEN (final tree, `cargo test`, mdcore lib target)

```
test page::tests::a_script_tag_in_the_document_cannot_execute_under_the_emitted_csp ... ok
test highlight::tests::theme_css_returns_two_distinct_stylesheets ... ok
test page::tests::assets_are_inlined_not_linked ... ok
test page::tests::banner_container_exists_outside_the_swappable_body ... ok
test page::tests::both_highlight_themes_are_emitted_under_a_media_query ... ok
test page::tests::style_src_still_allows_inline_for_katex ... ok
test page::tests::page_is_a_complete_html_document ... ok
test page::tests::title_is_the_file_name ... ok
test page::tests::unsafe_inline_is_not_permitted_in_script_src ... ok
test page::tests::script_src_carries_a_nonce_matching_the_bundled_scripts ... ok
test page::tests::two_pages_get_different_nonces ... ok
test page::tests::zoom_affordances_are_embedded_in_the_page ... ok

test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s

     Running tests/render_snapshots.rs (target/debug/deps/render_snapshots-7ebdb7040b9ec47b)

running 6 tests
test empty_file_renders_an_empty_body ... ok
test basic_commonmark ... ok
test gfm_tables_tasks_strikethrough_footnotes ... ok
test inline_and_display_math ... ok
test relative_and_remote_image_paths_are_preserved ... ok
test code_blocks_and_mermaid ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s

   Doc-tests mdcore

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

`cargo test --workspace` on the final tree: 17 + 4 + 30 + 6 = 57 tests, all passing,
`cargo build --workspace` produced no warnings (56 pre-existing + 1 new test = 57).

## Real-binary verification

```
$ printf '# Zoom check\n\n![local](./nope.png)\n\n```mermaid\ngraph TD;\n  A[Start] --> B{Choice};\n  B --> C[End];\n```\n' > /tmp/zoom.md
$ cargo run -q -p mdapp -- --print-html /tmp/zoom.md > /tmp/zoom.html
mdview-zoomable        5
mdview-zoom-btn        5
mdview-lightbox        28
enhanceZoomables       3
data-mdview-zoom       4
addEventListener       19
inline handlers (must be 0):
0
leakage (both 0):
0
0
```

`make install` rebuilt and reinstalled `/Applications/MDView.app` on the final tree.

## What is NOT verified (no screen available)

I did not, and cannot, visually confirm any of the interactive behavior. The
following need a human, in the running app, with a document containing both a
Mermaid diagram and an image:

- Hovering a diagram or image reveals the zoom button in its corner.
- Clicking the button opens the overlay showing a larger copy.
- Scroll-wheel zooms in/out about the cursor position.
- Trackpad pinch (ctrlKey wheel event) zooms the same way.
- Dragging pans the zoomed content.
- The `−` / `+` / reset buttons work as expected, and scale cannot be driven
  past the 0.25–8 clamp.
- Esc dismisses the overlay.
- Clicking the close button dismisses the overlay.
- Clicking the dimmed backdrop (not the stage/image) dismisses the overlay.
- The document's scroll position is unchanged after closing the overlay.
- Saving the source file (triggering live reload) does not duplicate zoom
  buttons or nest wrappers on already-enhanced diagrams/images.
- Both light and dark mode look correct (overlay backdrop, button borders/
  background, control glyphs).
