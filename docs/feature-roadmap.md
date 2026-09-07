# MDView feature roadmap

This roadmap implements five product expansions in dependency-aware order without changing MDView's core identity: a read-only, offline, keyboard-first macOS Markdown viewer.

**Audience:** MDView contributors familiar with Rust, AppKit, WebKit, and the existing `mdcore`/`mdapp` split.

## Recommended delivery order

| Phase | Feature | Outcome | Relative effort |
| --- | --- | --- | --- |
| 1 | Git Time Machine | Compare a document with any Git revision and browse its history | Medium |
| 2 | Project Workspace | Navigate and search a folder or repository as one collection | High |
| 3 | Intelligent Link Navigation | Preview links, move back and forward, find backlinks, and detect broken references | Medium |
| 4 | Review Inbox | Manage comments across documents and exchange portable review sessions | Medium |
| 5 | Publish and Share | Print, export PDF, export sections, and expose self-contained HTML | Low–medium |

Ship each phase independently. Do not hold completed phases for a single large release: the later features depend on data and UI seams established by the earlier ones, but each phase has its own useful exit state.

## Product and architecture constraints

The implementation should preserve these invariants:

- **Read-only documents.** MDView may write preferences, indexes, review records, and exports, but never modifies the Markdown source.
- **Offline by default.** Workspace indexing, Git history, reviews, and exports operate without a service or account.
- **One rendering implementation.** App pages, diffs, previews, and exports reuse `mdcore`; they must not introduce a second Markdown renderer.
- **WebKit remains a paint and interaction surface.** Filesystem, Git, persistence, printing, and save panels remain native responsibilities.
- **Page messages retain tab identity.** New bridge callbacks must follow the source-tab routing used by `AppDelegate::handle_message_from` in `crates/mdapp/src/app.rs`.
- **Global and per-document state stay distinct.** `AppState` coordinates workspaces and shared indexes; `DocumentWindow` owns tab-specific navigation, revision, scroll, and preview state.
- **Generated pages stay self-contained and safe.** Changes to `crates/mdcore/src/page.rs` must retain the nonce-based content security policy and the no-network execution model.
- **New UI remains discoverable by keyboard and mouse.** Every MDView command belongs in the in-page command catalogue and the appropriate native menu, following the existing contract tests.
- **macOS 11 remains the deployment floor** unless a separate product decision changes `LSMinimumSystemVersion` in `bundle/Info.plist`.

## Shared implementation strategy

### Separate data from AppKit

Pure models, parsers, indexers, and query logic should remain testable without a running application. Add them to `mdcore` when they are renderer- or repository-oriented, or to pure modules in `mdapp` when they describe application persistence and review state. Keep Objective-C bindings in `app.rs`, `window.rs`, `menu.rs`, and small native adapters.

### Introduce explicit view context

The existing `ViewMode` only distinguishes rendered Markdown from a HEAD diff. Phase 1 should replace the implicit HEAD assumption with an explicit per-tab context, conceptually:

```rust
enum DocumentView {
    WorkingTree,
    Comparison(ComparisonSpec),
}

struct ComparisonSpec {
    base: Revision,
    target: ComparisonTarget,
    layout: DiffLayout,
}

enum ComparisonTarget {
    WorkingTree,
    Revision(Revision),
}
```

The exact names may change during implementation. The invariant matters: render and reload code must receive the selected comparison rather than recover it from global preferences or page markup. Keep the diff layout as a global preference unless user testing shows layouts should differ per tab.

### Avoid blocking the main thread

Git history queries and workspace indexing can exceed the cost of the existing single-file render. Run them away from the AppKit main thread and post immutable results back to the delegate. Every asynchronous result must carry a request generation and workspace or tab identity so a late result cannot update a closed tab or superseded query.

Use the standard library first. Add a dependency only when it materially reduces correctness risk; document the reason in the change that introduces it.

### Add test fixtures, not machine-dependent assumptions

Git tests should create isolated temporary repositories, as `crates/mdcore/src/diff.rs` already does. Workspace and link tests should build temporary directory trees. Tests must not depend on the contributor's Git configuration, home directory contents, repository branches, or network.

## Phase 1 — Git Time Machine

### Goal

Let a reader browse commits that touched the open document and compare the working tree or two committed versions using all four existing diff layouts.

This phase comes first because MDView's rendered block diff is already a differentiator, and `crates/mdcore/src/rdiff.rs` needs only two source strings. The main work is generalizing Git acquisition and giving each tab an explicit comparison state.

### User experience

Deliver the smallest complete workflow first:

1. `g h` opens a searchable history palette for the current document.
2. Each row shows the short commit id, subject, author, and date.
3. Selecting a commit compares that revision with the working tree.
4. `g l` continues to cycle source/rendered and unified/split layouts.
5. Escape or the existing diff toggle returns to the working document.
6. The View menu exposes **Document History** and identifies the selected comparison.
7. Live reload updates only comparisons whose target is the working tree.

After that workflow is stable, add revision-to-revision comparison and branch/tag selection. Do not put branch, tag, commit, and two-sided comparison into the first UI iteration.

### Core changes

Refactor `crates/mdcore/src/diff.rs` around a reusable Git repository context:

- Add validated `Revision` and `RepoPath` value types. Never interpolate user input into a shell; continue invoking `git` with argument arrays.
- Split repository discovery and tracked-path resolution out of `availability` and `load_diff`.
- Replace the HEAD-specific loader with an API that accepts a base and target, while preserving `load_diff(path)` as a compatibility wrapper during migration.
- Add `history_for_path` using `git log --follow` with a delimiter-safe machine format and a result cap.
- Add source loading for `revision:path` and patch loading for revision-to-working-tree and revision-to-revision comparisons.
- Carry human-readable revision labels in the returned comparison so empty and error states no longer say only “against HEAD.”
- Keep `parse_patch`, `split_rows`, and `rdiff::render_body` independent of Git command execution.

Update `crates/mdcore/src/lib.rs` so `render_diff_document_with` and `render_diff_body_of` accept an explicit comparison. Update `crates/mdcore/src/page.rs` and `crates/mdcore/assets/init.js` to display the base and target labels without inferring them from body HTML.

### Native application changes

- Store comparison state on `DocumentWindow` in `crates/mdapp/src/window.rs`.
- Extend `Message` and `parse_message` in `crates/mdapp/src/state.rs` for opening history, selecting a revision, and leaving history.
- Add a history catalogue hook and palette in `crates/mdcore/assets/init.js`, reusing the existing palette behavior for themes, recent files, and commands.
- Route history requests and results through `AppDelegate` in `crates/mdapp/src/app.rs` with the originating tab id.
- Add native menu actions and validation in `crates/mdapp/src/menu.rs` and `AppDelegate`.
- Add optional CLI seams to make the feature testable without GUI automation, for example `--diff-base REV` and later `--diff-target REV`. Preserve `--diff` as HEAD-to-working-tree shorthand.

### Tests

- Revision validation rejects empty, malformed, and option-like values where appropriate.
- History parsing handles Unicode, delimiters in subjects, renamed files, deleted files, and a configured result cap.
- Comparison loading covers HEAD-to-working-tree, commit-to-working-tree, and commit-to-commit.
- Missing revisions, shallow history gaps, untracked files, and repositories without commits produce distinct errors.
- Existing patch parsing and rendered block pairing tests remain unchanged where possible.
- CLI integration tests prove revision flags select the requested source.
- Page contract tests prove the history catalogue and comparison labels exist.
- A reel demonstrates selecting a historical revision and cycling all four layouts.

### Exit criteria

- A tracked Markdown file can be compared with any commit returned by its history palette.
- All four layouts render the selected comparison correctly.
- Switching revisions never changes the working file.
- A late Git result cannot update another or closed tab.
- Existing `g d`, `g l`, live reload, comments, outline, and minimap behavior continues to work.
- `cargo test`, `make shot` for each layout, and the new history reel pass.

### Main risks

**Renames:** `git log --follow` helps history discovery but does not directly provide one stable path for every revision. Resolve the path for a selected commit before loading its source, and cover rename chains with tests.

**Large histories:** cap initial results and load additional pages on demand. Do not collect an unbounded repository history on the main thread.

**Meaning of “current”:** an uncommitted working tree and a committed target differ from two immutable revisions. Model the target explicitly so live reload and labels remain correct.

## Phase 2 — Project Workspace and cross-document search

### Goal

Let a user open a folder or Git repository, navigate its Markdown files, search their contents, and restore the reading session.

Workspace mode should remain optional. Opening one file must stay as fast and simple as it is now.

### User experience

1. **File → Open Folder…** selects a workspace root.
2. The sidebar gains a **Files** panel with a lazy, keyboard-navigable Markdown tree.
3. `g f` opens a fuzzy file palette.
4. `g /` opens workspace search and shows highlighted snippets plus the nearest heading.
5. Selecting a result opens or focuses the document and lands on the match.
6. Relaunching MDView restores the workspace's open tabs, selected tab, and per-document reading position.
7. File changes update the tree and search index without requiring a restart.

### Workspace model

Add a pure workspace module, preferably `crates/mdcore/src/workspace.rs`, with models such as:

- `WorkspaceRoot`
- `WorkspaceFile` with canonical path, root-relative display path, and stable identity
- `WorkspaceSnapshot` with ordered files and a generation number
- `SearchQuery`, `SearchHit`, and `SearchSnippet`
- `IgnoreRules` or a narrowly scoped walker policy

Start with `.md`, `.markdown`, and `.mdown`, matching `crates/mdapp/src/navigation.rs`. Skip `.git`, hidden build directories, symlink cycles, and files above a documented size cap. Respecting `.gitignore` is desirable but should not block the first complete release; if omitted initially, make that limitation visible in the roadmap issue and UI help.

### Indexing approach

For the first release, build an in-memory index from normalized text plus heading ranges. This avoids a database migration and keeps the offline model simple.

- Scan in a worker thread and send batches so the file tree can appear before indexing completes.
- Store lowercase text for filtering, original text offsets for snippets, and heading boundaries for context.
- Incrementally replace entries after watcher events.
- Debounce event bursts and perform a bounded reconciliation scan because filesystem watchers can coalesce or omit individual changes.
- Cancel or supersede old generations when the workspace changes.

Do not introduce SQLite until measurement shows memory or startup time is unacceptable for representative documentation repositories.

### Native application changes

- Add workspace identity, index status, and worker channels to `AppState` in `crates/mdapp/src/app.rs`.
- Generalize `watcher.rs` from one file to a bounded recursive workspace watcher or add a separate `WorkspaceWatcher`.
- Extend `defaults.rs` with versioned workspace-session storage. Arrays and primitive defaults are sufficient only if the format remains explicit and migratable; otherwise use one app-owned file under Application Support.
- Add **Files** and **Search** sidebar modes in `crates/mdcore/src/page.rs` and `crates/mdcore/assets/init.js`.
- Extend `Message` with workspace open, file selection, search query, search result selection, and session updates.
- Add directory selection and menu actions in `app.rs` and `menu.rs`.
- Restore tabs only after validating that each path remains inside the saved workspace and still exists. Missing files are skipped and reported once, not treated as fatal.

### Tests

- Discovery is deterministic and excludes unsupported files, `.git`, oversized files, and symlink cycles.
- Search is case-insensitive by default, returns stable ordering, clips snippets safely on Unicode boundaries, and identifies heading context.
- Incremental add/change/delete operations produce the same index as a full rescan.
- Generation tests prove stale scans and stale queries are ignored.
- Session parsing tolerates missing fields and unknown newer fields without losing valid paths.
- Integration tests open multiple search results and prove existing-tab deduplication still applies.
- A reel demonstrates file navigation and cross-document search.
- Benchmark representative small, medium, and large workspaces before deciding whether persistence or a more complex index is needed.

### Exit criteria

- A folder can be opened and all supported Markdown files are navigable.
- Search results arrive progressively and identify both file and section.
- Filesystem updates converge without restarting the app.
- Restoring a session cannot open paths outside the recorded workspace.
- Single-file launch behavior and startup performance remain unaffected when no workspace is open.

### Main risks

**UI density:** the existing sidebar was designed for one document. Keep Files, Search, Outline, Bookmarks, and Comments as explicit modes rather than mixing them into one long panel.

**Scale:** establish file-count, file-size, total-byte, and result caps. Show partial-index status instead of freezing or silently dropping content.

**Session privacy:** store local paths only in the existing local preferences/Application Support model. Do not add telemetry or cloud synchronization as part of workspace mode.

## Phase 3 — Intelligent Link Navigation

### Goal

Turn local Markdown links into fast, explainable navigation: preview before opening, move back and forward, list backlinks, and reveal broken targets.

This phase follows Workspace because backlinks and repository-wide link validation require a known document set. Single-file users still receive history and previews for resolvable local links.

### User experience

- Hovering or invoking a keyboard command on a local Markdown link opens a lightweight preview with the target heading and surrounding content.
- Enter opens the target; an alternate action opens it in a new native tab.
- Back and forward restore document, heading or text anchor, and scroll position.
- The sidebar gains a **Links** view containing backlinks and broken outgoing links.
- Broken links are visibly marked but do not prevent reading.
- External HTTP, HTTPS, and mail links continue to open through `NSWorkspace`; they are never fetched for preview.

### Link model and resolver

Add `crates/mdcore/src/links.rs` with pure types and functions:

- `DocumentLink` with source range, raw destination, resolved target, and optional fragment
- `ResolvedLink::{Document, Heading, External, Asset, Missing, OutsideWorkspace}`
- heading slug generation shared with the page instead of independently reimplementing `slugify` in Rust and JavaScript
- an outbound-link extractor based on the same `pulldown-cmark` event stream used by rendering
- a workspace link graph with outgoing and incoming edges

Resolve percent encoding, relative paths, fragments, duplicate heading slugs, extensionless Markdown conventions if deliberately supported, and case sensitivity according to the filesystem. Do not silently reinterpret ambiguous links; expose them as unresolved.

### Preview rendering

Use `mdcore` to render the target document, then select a bounded section or block range for the preview. The preview must retain the page CSP and must not execute document-provided scripts. Add a compact page mode rather than assembling ad hoc HTML in JavaScript.

### Navigation state

Add a per-tab stack to `DocumentWindow` containing canonical document identity plus a stable reading anchor. Prefer heading identity and intra-section text context over raw `scrollY`, with `scrollY` as a fallback. Record navigation only for committed moves, not hover previews or reloads.

When a link opens another document in a new native tab, the new tab receives its own history. When it opens in the same tab, either permit `DocumentWindow` to change its document safely or replace the tab while transferring history. The existing invariant that a document tab is never repointed should not be broken casually; prototype both approaches and preserve watcher/bridge identity correctness.

### Native and page changes

- Extend `navigation.rs` to carry fragments and explicit open disposition rather than only a path.
- Add preview and navigation messages to `state.rs`.
- Add preview presentation, link markers, back/forward hooks, and Links sidebar rendering to `init.js` and `page.css`.
- Add menu actions and validation for Back, Forward, Open Link, and Open Link in New Tab.
- Feed the workspace graph from Phase 2 into each page by bounded summaries; do not inject the entire graph into every tab.

### Tests

- Link extraction covers inline links, reference links, encoded paths, fragments, images, and raw external URLs.
- Resolution covers duplicate headings, missing headings, renamed/missing files, parent-relative paths, and paths outside the workspace.
- Backlinks update after source changes.
- Preview rendering is bounded and preserves CSP behavior.
- History restoration survives live reload and skips documents that disappeared.
- Navigation delegate tests continue to reject custom schemes, script URLs, and unintended WebKit navigation.
- A reel demonstrates preview, open, back, forward, backlink, and broken-link states.

### Exit criteria

- Every local Markdown link has one explainable resolution state.
- Preview and full navigation use the same resolver.
- Back and forward restore a useful reading position across documents.
- Workspace backlinks and broken-link status update after edits.
- External links preserve the existing safe handoff behavior.

### Main risks

**Slug disagreement:** heading ids are assigned by JavaScript today. Move slug rules behind shared fixtures or emit stable ids from Rust so link resolution and page anchors cannot disagree.

**Tab semantics:** native tabs currently map one-to-one to immutable document paths. Decide and test same-tab link behavior before implementation; transferring history into a replacement tab is safer than silently mutating a window with path-bound watchers.

**Large graphs:** update edges only for changed documents and send bounded page data.

## Phase 4 — Review Inbox and portable review sessions

### Goal

Make reviews manageable across a workspace while preserving the existing human-readable files and safe-write behavior.

### User experience

- A **Review Inbox** lists unresolved comments across the workspace, grouped or filtered by document.
- Comments have `open`, `resolved`, and `stale` states. A stale comment is one whose quote no longer anchors.
- Selecting an inbox item opens the document and jumps to its passage or stale card.
- A reviewer can export a review session and another user can import it without receiving arbitrary filesystem paths.
- `C` can copy a prompt for the current document or the filtered inbox.
- A review summary can be exported after all comments are resolved.

### Review format evolution

Version the grammar in `crates/mdapp/src/review.rs` before adding fields. Preserve the current rule: a partially parsed review is displayed but never destructively rewritten.

Add fields only when their behavior is implemented:

- document identity relative to a workspace root
- comment status
- optional reviewer display name
- created and resolved timestamps if they support sorting or summaries
- optional reply records only after the status workflow is stable

Do not infer “resolved” solely from a deleted record. Preserve deletion for permanent removal and add an explicit status so review history can be summarized.

### Storage and portability

Keep live review files under Application Support, as `crates/mdapp/src/store.rs` requires. A portable bundle should be an explicit export containing:

- schema version
- workspace-relative document identities
- content fingerprints for matching
- comments and statuses
- optional human-readable manifest

Use a deterministic archive or directory format that can be tested without AppKit. Never import absolute paths from another machine. Match by relative path first, then require user confirmation for fingerprint-based relocation.

### Inbox aggregation

Add a pure `ReviewIndex` that loads only review records belonging to the active workspace. Update it from the existing review watchers and workspace file events. Do not scan every file under Application Support on each timer tick.

Move storage and grammar into a dedicated library module or `mdcore` only if doing so does not pull AppKit concerns across the crate boundary. The important split is pure format/index logic versus filesystem and UI adapters.

### Native and page changes

- Add review index state and import/export orchestration to `AppState`.
- Extend `store.rs` with enumeration, versioned reads, portable export, and collision-safe import.
- Extend `Message` for filtering, resolving/reopening, multi-document prompt generation, and navigation.
- Add an Inbox sidebar mode or palette in `init.js`; keep per-document comment cards unchanged.
- Add File or Review menu actions for Import Review, Export Review, and Export Review Summary.
- Use `NSSavePanel`/`NSOpenPanel` for user-selected destinations; never write beside source documents implicitly.

### Tests

- Old review files parse with `open` status and round-trip through the new serializer.
- New fields preserve awkward payloads and malformed-record damage reporting.
- Resolving, reopening, and deleting are distinct operations.
- Inbox aggregation filters by canonical workspace membership and updates incrementally.
- Export is deterministic; import rejects traversal paths, unsupported versions, and unsafe absolute identities.
- Conflict behavior is explicit and tested: merge by stable comment id and document identity, never silently overwrite newer local work.
- Prompt generation includes only the selected filter and names every review file it expects an agent to update.
- A reel demonstrates the inbox and status transitions.

### Exit criteria

- All comments in an active workspace are visible from one inbox.
- Status changes survive relaunch and retain review history.
- Portable sessions round-trip between two workspace copies without absolute path leakage.
- Damaged input remains non-destructive.
- Existing Claude handoff continues to update the visible cards through file watchers.

### Main risks

**Migration safety:** write backups before the first format upgrade and test older fixtures. Never use a successful partial parse as permission to rewrite.

**Identity across clones:** paths alone are insufficient; fingerprints alone can collide or become stale. Use both and ask the user when relocation is ambiguous.

**Scope creep into collaboration:** keep this release file-based and offline. Real-time multi-user synchronization, accounts, permissions, and hosted comments are separate products.

## Phase 5 — Publish and Share

### Goal

Expose MDView's existing deterministic rendering through native print, PDF, section export, and explicit HTML export.

`mdcore` already generates complete HTML with embedded styles, scripts, math/Mermaid runtimes, and supported local images. This phase productizes that capability instead of creating a second export renderer.

### User experience

- **File → Print…** prints the active rendered document or comparison.
- **File → Export PDF…** saves a PDF after diagrams and math finish rendering.
- **File → Export HTML…** writes the same self-contained page produced by `--print-html`.
- **Export Current Section…** exports the current heading section as HTML or PDF.
- **Copy as Rich Text** copies the selection or current section for pasting into rich-text applications.
- Export options choose document theme versus print-optimized light theme and optionally include a generated table of contents.

### Rendering changes

Add an explicit render purpose to `crates/mdcore/src/page.rs`, conceptually `Interactive`, `Print`, or `SectionExport`.

- Interactive pages retain palettes, comments, minimap, keyboard handlers, and the host bridge.
- Print/export pages omit interactive chrome and include print-specific CSS.
- Section export uses the same parser/render pipeline and preserves definitions needed by the selected section, including referenced links and footnotes.
- Exported comparisons retain clear base/target labels.
- HTML export reports local images that exceeded `images::MAX_INLINE_BYTES` or used unsupported formats rather than claiming the result is portable.

Do not strip the CSP from HTML exports. If an exported page requires scripts for Mermaid or KaTeX, retain nonce stamping and embedded assets.

### Native application changes

- Add print and export actions to `menu.rs` and `AppDelegate`.
- Add a page-readiness contract stronger than navigation completion: PDF capture must wait until `mdviewRenderAll` has completed asynchronous Mermaid rendering.
- Use native save panels and atomic output writes.
- Confirm the macOS 11 availability and `objc2-web-kit` binding surface for WebKit PDF generation in a short implementation spike. If direct PDF generation is unavailable on the deployment floor, use the WebKit print operation rather than a new HTML/PDF dependency.
- Add a page bridge request for the active section and rich-text payload.
- Extend CLI support with explicit output paths or formats only if semantics remain script-friendly; never mix binary PDF bytes with diagnostics on stdout.

### Tests

- Interactive and export page modes contain only their intended chrome.
- HTML export remains self-contained for supported local images and contains no external stylesheets or scripts.
- Oversized, missing, and unsupported images produce an actionable export warning.
- Section extraction handles introductory content, nested headings, footnotes, reference links, math, Mermaid, and no-heading documents.
- Print CSS snapshots cover tables, page breaks, code overflow, rendered diffs, and light/dark choices.
- App-level tests cover cancellation, atomic writes, invalid destinations, and page readiness.
- Manually verify generated PDFs on the minimum supported macOS version and one current macOS version.

### Exit criteria

- The active document and every diff layout print without interactive chrome.
- PDF capture waits for math and diagrams and produces selectable text where WebKit supports it.
- Exported HTML opens offline with supported local images intact.
- Current-section exports contain all definitions needed to render correctly.
- Export never modifies the source document or writes without an explicit destination.

### Main risks

**Asynchronous rendering:** navigation completion does not mean Mermaid is finished. Add an explicit JavaScript completion signal and timeout with a visible error.

**Pagination:** wide tables, split diffs, and large diagrams need print-specific fallback layouts. Prefer readable single-column print output when a split view cannot fit.

**Remote images:** HTML is self-contained only for supported local images. Remote sources are intentionally preserved by the renderer and may not work offline; make that limitation explicit at export time.

## Cross-cutting release gates

Apply these gates to every phase:

### Correctness

- Pure logic has unit tests.
- Crate-boundary strings and JavaScript hook names have contract tests.
- Temporary repositories and workspaces are isolated and deterministic.
- Error states explain what failed without exposing raw command construction or silently falling back.

### Security and privacy

- Git commands receive argument arrays, disable prompts and pagers, and never invoke a shell.
- Imported paths are canonicalized and checked against the chosen workspace.
- Raw Markdown cannot execute scripts in generated or preview pages.
- No feature sends document content, indexes, reviews, or paths over the network.

### Performance

Record baseline and post-change measurements for:

- first document open
- live reload of a medium document
- history palette first result
- workspace discovery and index completion
- cross-document query latency
- memory for a representative large workspace

Set caps from measurements and expose partial results rather than blocking the UI.

### Accessibility and macOS behavior

- New lists expose appropriate roles, labels, selection, and keyboard focus.
- Every command is available through the native menu.
- Reduced-motion, light/dark appearance, VoiceOver navigation, and full keyboard access receive manual checks.
- Features work on the minimum supported macOS version or are runtime-guarded with a documented fallback.

### Validation commands

Run the narrowest relevant checks while developing, then the complete release checks:

```sh
cargo test -p mdcore
cargo test -p mdapp
make test
make bundle
```

For visual changes, add or update focused snapshots, `make shot` scenarios, and one generated reel per feature. Run `make dist` only at a release boundary because it builds both architectures and packages the DMG.

## Issue breakdown and tracking

Create one tracking issue per phase and one implementation issue per milestone below. Finish milestones in order within a phase; parallelize only when their file ownership does not overlap.

### Phase 1 milestones

- [x] Generalize repository discovery, revision validation, and Git source loading.
- [x] Add path history and rename-aware revision resolution.
- [x] Render explicit comparisons in all four layouts.
- [x] Add per-tab comparison state and asynchronous request generations.
- [x] Add the history palette, commands, and native menus.
- [x] Add CLI coverage, fixtures, snapshots, and a history reel.

### Phase 2 milestones

- [x] Define workspace discovery rules, caps, and pure models.
- [x] Build progressive background scanning and the in-memory index.
- [x] Add Files and workspace Search UI.
- [x] Add incremental watcher reconciliation.
- [x] Add versioned session restore for workspace tabs and reading positions.
- [x] Measure scale, add fixtures, and generate a workspace reel.

### Phase 3 milestones

- [x] Extract links and unify heading slug behavior.
- [x] Resolve links and build the incremental workspace graph.
- [x] Add compact target previews.
- [x] Add back/forward state and settle same-tab semantics.
- [x] Add backlinks and broken-link UI.
- [x] Add security tests, snapshots, and a links reel.

### Phase 4 milestones

- [ ] Version the review grammar and add explicit statuses.
- [ ] Build the incremental workspace review index.
- [ ] Add Review Inbox filtering and navigation.
- [ ] Add portable, path-safe export/import with conflict handling.
- [ ] Add multi-document prompts and review summaries.
- [ ] Add migration fixtures and a review-inbox reel.

### Phase 5 milestones

- [ ] Add render-purpose and current-section extraction APIs.
- [ ] Add print CSS and export-ready comparison labels.
- [ ] Complete the macOS 11 PDF/print API spike.
- [ ] Add native Print, PDF, and HTML export flows.
- [ ] Add Copy as Rich Text and export warnings.
- [ ] Validate PDFs, snapshots, CLI behavior, and an export reel.

## First implementation step

Start Phase 1 with a refactor-only change in `crates/mdcore/src/diff.rs`: introduce repository/path/revision types and make the existing HEAD-to-working-tree behavior call the generalized loader. Keep output byte-for-byte equivalent and run the existing diff tests before adding history UI. This creates a reviewable foundation and prevents Git acquisition, tab state, and palette work from landing as one inseparable change.
