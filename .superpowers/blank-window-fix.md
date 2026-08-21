# Blank window fix

## Root cause (restated)

`crates/mdapp/src/navigation.rs`'s `decide_policy` only allowed a navigation
through when `scheme == "about"` and the navigation type was `Other`, on the
claim that `loadHTMLString` "arrives here as an about:blank navigation."
That claim was false for the document path: `window.rs`'s `reload()` calls
`webview.loadHTMLString_baseURL(html, Some(base))` where `base` is a
`file://` URL for the document's parent directory, so WebKit reports the
navigation with scheme `file`, not `about`. The `about` branch never
matched, `classify` got a directory path with no `.md` extension and
returned `None`, and control fell through to `Cancel` — the delegate
cancelled the app's own document load, leaving a blank white window on
every open. (`show_error`'s `loadHTMLString_baseURL(html, None)` resolves
to `about:blank` and so was never affected — it worked by accident of a
different code path.)

## What changed

### `crates/mdapp/src/navigation.rs`

- Added `Decision` enum (`Allow` / `Cancel` / `CancelAndHandle(NavigationRequest)`).
- Added pure `decide(absolute, scheme, file_path, is_other, expecting_own_load) -> Decision`,
  extracted from the objc delegate body. `classify` is untouched.
- Added `expecting_own_load: Rc<Cell<bool>>` to `NavigationState`.
- Rewrote `decide_policy` to a thin shim: extract `absolute`/`scheme`/`path`/
  `navigationType` as before, read-and-clear the one-shot token
  (`self.ivars().expecting_own_load.replace(false)`), call `decide`, then
  act on the `Decision` — calling the decision handler exactly once on
  every path (`Allow`/`Cancel` call it directly; `CancelAndHandle` invokes
  `self.ivars().handler` then calls the handler with `Cancel`).
- `NavigationDelegate::new` now takes a third parameter,
  `expecting_own_load: Rc<Cell<bool>>`.
- Added `use std::cell::Cell;`.

### `crates/mdapp/src/window.rs`

- `DocumentWindow` gained `expecting_own_load: Rc<Cell<bool>>`, following the
  same sharing pattern as `closed: Rc<Cell<bool>>` with `WindowCloseDelegate`.
- `DocumentWindow::open` creates the `Rc<Cell<bool>>`, passes a clone into
  `NavigationDelegate::new`, and stores the original in the struct.
- `DocumentWindow::reload()` sets `self.expecting_own_load.set(true)`
  immediately before its `loadHTMLString_baseURL(&doc.html, Some(&base))` call.
- `DocumentWindow::show_error()` sets `self.expecting_own_load.set(true)`
  immediately before its `loadHTMLString_baseURL(&html, None)` call.

The one-shot token (not URL matching) is what gates `Allow`: a document can
embed raw HTML such as `<meta http-equiv="refresh" content="0;url=https://evil">`,
which WebKit also reports as navigation type `Other`. Gating on the token
means only the load the app itself just initiated is ever allowed through;
that meta-refresh is still cancelled (and, since its scheme is `https`,
handed off as `CancelAndHandle(OpenExternal(...))`) because the token is
consumed by the app's own load and is `false` by the time the meta-refresh
fires.

## TDD: RED then GREEN

Tests were added to `navigation.rs`'s test module first, verbatim as specified,
then `cargo test` was run to confirm the module did not compile (no `decide`/
`Decision` yet) — genuine RED, not just failing assertions:

```
   Compiling mdapp v0.1.0 (/Users/loi.nd/workspace/mdview/crates/mdapp)
error[E0425]: cannot find function `decide` in this scope
   --> crates/mdapp/src/navigation.rs:122:13
    |
122 |             decide(
    |             ^^^^^^ not found in this scope

error[E0433]: cannot find type `Decision` in this scope
   --> crates/mdapp/src/navigation.rs:129:13
    |
129 |             Decision::Allow
    |             ^^^^^^^^ use of undeclared type `Decision`

[... repeated E0425/E0433 pairs for each of the 7 new tests ...]

error[E0425]: cannot find function `decide` in this scope
   --> crates/mdapp/src/navigation.rs:193:20
    |
193 |         assert_eq!(decide(None, None, None, false, false), Decision::Cancel);
    |                    ^^^^^^ not found in this scope

error[E0433]: cannot find type `Decision` in this scope
   --> crates/mdapp/src/navigation.rs:193:60
    |
193 |         assert_eq!(decide(None, None, None, false, false), Decision::Cancel);
    |                                                            ^^^^^^^^ use of undeclared type `Decision`

Some errors have detailed explanations: E0425, E0433.
For more information about an error, try `rustc --explain E0425`.
error: could not compile `mdapp` (bin "mdview" test) due to 14 previous errors
warning: build failed, waiting for other jobs to finish...
```

Full text captured at `/tmp/mdview-fix/blank-red.txt` (this session's scratch
location; not part of the repo).

After implementing `Decision`/`decide`, rewiring `decide_policy`, and wiring
the token through `window.rs`, GREEN on the final tree (temporary diagnostic
`eprintln!` already removed before this capture — see below):

```
running 17 tests
test navigation::tests::a_link_click_to_the_base_directory_is_cancelled ... ok
test navigation::tests::a_local_markdown_link_opens_a_document ... ok
test navigation::tests::a_missing_url_is_cancelled ... ok
test navigation::tests::an_external_link_is_handed_off_not_followed ... ok
test navigation::tests::a_meta_refresh_after_our_load_is_cancelled ... ok
test navigation::tests::http_links_open_externally ... ok
test navigation::tests::markdown_extension_match_is_case_insensitive ... ok
test navigation::tests::markdown_file_with_spaces_opens_as_document ... ok
test navigation::tests::markdown_files_open_as_documents ... ok
test navigation::tests::non_markdown_files_are_ignored ... ok
test navigation::tests::our_own_document_load_is_allowed ... ok
test navigation::tests::our_own_error_page_load_is_allowed ... ok
test navigation::tests::unknown_schemes_are_ignored ... ok
...
test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

(mdapp unit tests: 17, up from 10 — the 7 new `decide`/`Decision` tests, all
passing. Combined with mdapp's other 4 integration tests, mdcore's 29 unit
tests, and mdcore's 6 snapshot tests: 56 total passing, 0 failed — up from
the baseline 49, matching the 7 tests added. `cargo build --workspace` is
warning-free.) Full text captured at `/tmp/mdview-fix/blank-green.txt`.

## Real-GUI verification

A temporary `eprintln!("[DIAG] decide_policy: absolute={absolute:?}
scheme={scheme:?} decision={decision:?}");` was added right after computing
`decision` in `decide_policy`, the app rebuilt, and launched against this
repo's own `README.md`:

```
( ./target/debug/mdview /Users/loi.nd/workspace/mdview/README.md > /tmp/g.out 2>/tmp/g.err & echo $! > /tmp/g.pid ) ; sleep 6; kill "$(cat /tmp/g.pid)" 2>/dev/null
grep DIAG /tmp/g.err
```

Output:

```
[DIAG] decide_policy: absolute=Some("file:///Users/loi.nd/workspace/mdview/") scheme=Some("file") decision=Allow
```

This is the exact navigation from the original bug report (`file://` base
directory, no trailing markdown path) and it now decides `Allow` instead of
`Cancel`. The delegate allowed the app's own document load through.

The diagnostic was then removed, the app rebuilt, and `cargo test` re-run to
capture the final GREEN quoted above on the tree with no diagnostic code left
in it. `git status` shows only `crates/mdapp/src/navigation.rs` and
`crates/mdapp/src/window.rs` modified, both matching the described change —
no eprintln left behind.

## What remains unverified

- No screen was available in this environment: it cannot be confirmed that
  the window actually renders visible text, only that the navigation
  delegate now allows WebKit to proceed with the load instead of cancelling
  it (which is what produced the blank window). "The delegate allowed the
  load" is the honest claim; "the document is visibly rendering" is not one
  this session can make.
- Live-reload and link-click paths (`a_link_click_to_the_base_directory_is_cancelled`,
  `an_external_link_is_handed_off_not_followed`, `a_local_markdown_link_opens_a_document`)
  are covered by the pure `decide` unit tests but were not driven through the
  real GUI's file watcher or web view — no automated or manual GUI test
  harness exists for that in this environment.
- The meta-refresh XSS-style scenario (`a_meta_refresh_after_our_load_is_cancelled`)
  is verified only at the `decide` unit-test level, not by actually loading a
  document containing a `<meta http-equiv="refresh">` tag in the real app.
