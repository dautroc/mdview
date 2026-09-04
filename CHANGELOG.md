# Changelog

Release notes for MDView, organized around user-visible features, fixes, and
the commits that introduced them. The newest release is listed first.

## [v0.18.0](https://github.com/dautroc/mdview/releases/tag/v0.18.0) — 2026-09-04

### What's new

- **A tour, and the demos in it are generated rather than recorded.**
  `docs/tour.md` shows the app working in seven sections, grouped by what a
  reader is doing at the time rather than by which key does it — reading
  something long, finding your way, reviewing a draft with Claude, seeing what
  changed, coming back to a document, what it renders, making it yours. The
  README kept one image from v0.9 and a list of features nobody could see; the
  minimap, both palettes, the rendered diff layouts and the theme-derived
  document colours had all shipped since without ever being shown.
- **`make reels`.** The whole interface is JavaScript in `init.js`, and
  `tools/shot.swift` already rendered a generated page headlessly and ran
  JavaScript against it. `tools/reel.swift` is that, stepped: a list of steps
  in `reels/*.json`, a snapshot after each, and the dwell time recorded beside
  it for ffmpeg. So a demo is rebuilt when the interface changes rather than
  re-shot by hand, every demo is pinned to one theme and one size, and the
  recents and bookmarks in them are literals in a spec rather than whatever
  was on the machine that pressed record.
- **The reel answers the bridge.** The page asks the app before it offers:
  `postToHost` returns false with no handler and `hasHost` gates `c` outright,
  so a bare web view answers `c` with "Comments need the app", drops `m`, and
  does nothing for `g d`. `reel.swift` registers the same `mdview` message
  handler `window.rs` does and answers from the spec — a page load, or one of
  the `window.mdview*` hooks, which is what the app answers with. Without it,
  four of the seven sections could not have been filmed at all.
- **`mdview --print-html --diff [--diff-layout LAYOUT]`.** The diff is a whole
  other page — `build_diff_page`, not a state the normal one can be put into —
  so the four layouts were the one part of the app that could not be rendered
  outside it, which is why `make shot` has never been able to show one. An
  unrecognised layout name is refused rather than defaulted: `--diff-layout
  renderd` quietly printing a unified diff is the kind of typo a demo bakes
  into a GIF.
- **Three tests keep the tour from rotting.** Nothing can check that a demo is
  current, but a demo referenced and never generated can be caught, and so can
  a retired shortcut still advertised in the one document that names keys
  outside the README's table.

### Notes

- Frames are drawn to an explicit pixel size. `takeSnapshot` hands back an
  image backed by whichever screen the offscreen window landed on, so the same
  reel came back 900×620 on one run and 1800×1240 on the next; a demo that
  changes size when it is rebuilt is not a demo that can be rebuilt.
- ffmpeg is a developer's dependency and nothing else. It is not in the binary,
  not in the bundle, and `all`, `test` and `dist` never reach it. Without it
  the frames are still written and `make reel` says where they are.
- The comment cards are shown in the sidebar rather than the margin, because
  that is where they now land: the reading column caps at 150 characters, and
  the rail wants 180px beyond it, so the margin only appears past about a
  2000px window. The behaviour is unchanged since v0.17 — the demo just makes
  it visible.
- `docs/screenshot.png` was retaken from the running app at v0.18.0, showing
  the outline and the minimap that the v0.9 image predated. It stays a hand
  capture rather than a generated one, because it is the only image that shows
  the real macOS window.

## [v0.17.0](https://github.com/dautroc/mdview/releases/tag/v0.17.0) — 2026-09-04

### What's new

- **The document takes the theme's colours, not just the code inside it.**
  Every named theme has derived its page chrome from its syntax palette for
  six versions, but only six of those colours ever reached the document:
  background, text, muted, border, code surface and link. Everything a
  Markdown file actually says — every heading, every table, every bold run,
  every inline code span — was painted in one colour. Headings, links, inline
  code, emphasis and table headings now come from what the theme itself says
  about Markdown, so a page reads the way it does in the editor the theme came
  from.
- **Links are the palette's, at last.** They were GitHub blue on all eight
  themes, because the link colour was assigned the chrome accent rather than
  anything the palette said. Chiroptera painted its links `#6cb6ff` while its
  own link colour sat unused at `#85c6c9`. The chrome keeps the accent, which
  is what the find bar and the jump labels want; the document gets the theme.
- **Read out of the palette, not guessed at.** The eight themes disagree about
  how a heading is even expressed — a plain `markup.heading` rule, or only the
  `#` marks coloured with the text left to `entity.name.section`, or a font
  weight and no colour at all. Asking for the scope name you expect finds one
  of those three. MDView hands syntect the scope stack its own Markdown syntax
  would push and lets the matcher answer, and reads silence as silence: a
  palette that says nothing about prose falls through to the code scopes every
  palette does define, so no theme is left without a colour and none is
  invented.
- **A colour that cannot be read is corrected, and one that can is left
  alone.** Each hue is measured against the surface it actually lands on —
  the code surface for inline code and table headings, the page for the rest —
  and lifted toward the text colour only as far as legibility needs. Solarized
  Light runs its own body text at 4.13:1, so an unbending 4.5 target would
  walk every hue to the foreground and hand back the monochrome page this
  change exists to remove; it settles for the 3:1 floor instead of giving up
  the colour. Headings take 3:1 as large text, which is what keeps Solarized
  Dark's yellow from turning grey over four hundredths of a ratio.
- **The minimap heads its sections in the same colour the page does.**
- **Chiroptera Dark Hard.** An eighth theme, translated from the Neovim
  colourscheme's own palette file and the mapping in its `core.vim` rather
  than eyeballed from a screenshot: String green, Number and Type blue,
  Function and Boolean bright magenta, Keyword yellow, Conditional red,
  Operator and PreProc magenta, Label and Constant cyan, Comment dim and
  italic. It ships as a tmTheme beside Monokai Pro, so its page chrome derives
  from it exactly as every other theme's does. Its comment colour sits at
  about 4:1 against the page, under WCAG AA — that is the palette's own value,
  and a theme that reads differently here than in the editor is not the theme.
- **The reading column is 150 characters wide.** It had been capped at 46rem
  since the first commit, about ninety characters — the width a page of
  running text wants, and narrower than a lot of what people keep in Markdown.
  A six-column table no longer has to scroll inside its own box to be read.
  `g w` still removes the cap entirely.

### Notes

- The System theme is unchanged. It stamps no theme attribute, so none of the
  new colours are defined for it, and every rule names a fallback: it renders
  the monochrome GitHub renders, as before.

## [v0.16.0](https://github.com/dautroc/mdview/releases/tag/v0.16.0) — 2026-09-03

### What's new

- **The diff can be the document.** `g d` has always opened a source diff: a
  row per line, highlighted as Markdown source. That is the view for checking
  a table's pipes, and the wrong one for the question people actually bring to
  a diff of prose — which of these paragraphs is not the one I wrote. `g l`
  now cycles two axes rather than one: the source or the document, in one
  column or two.
- **In one column, the document with its changes marked.** The working copy
  renders exactly as it always does, with a bar in the margin beside every
  block that changed against HEAD, and the version that was there before
  folded away under it, one click from being read. Nothing is tinted and
  nothing is struck through: a page of coloured bands stops reading as a page.
- **In two, the two documents side by side.** A row to a block, which is what
  makes side-by-side rendered prose possible at all — two documents laid out
  independently drift apart within a screen, because a paragraph and the
  paragraph that replaced it are not the same height. A row is as tall as its
  taller half, so the columns stay level the whole way down. Reach for this one
  when a page has been rewritten rather than edited.
- **The blocks are paired, not the lines.** The rendered layouts ignore Git's
  hunks and compare the two versions' top-level blocks instead, because the
  block is the unit a reader compares. An edit that changes the file without
  changing the document — `_em_` written as `*em*` — is not reported as a
  change, and a run of rewrites pairs the way the split source diff pairs a run
  of rewritten lines, so the two views cannot disagree about what happened.
- **The outline and the minimap keep working in it.** They stay out of the
  source diff, which has no headings and no shape to map, but the single-column
  rendered diff is still the document and gets both. The version that was there
  before is inert until you open it, so it cannot double the outline, cannot be
  landed on by `]` and `[`, and cannot be found by `/` — a search that offered
  matches in text that is not in the document would be worse than no search.
- **It says so when the change is somewhere it cannot show.** Frontmatter comes
  off before the renderer sees it and a link definition renders as nothing at
  all, so either can be the whole of an edit. Rather than show an unmarked
  document and imply nothing happened, the layout names what changed and points
  at the source diff — the one thing a diff must never do is claim there is
  nothing to see.
- **What you read is what the document is.** The blocks are parsed and rendered
  in one pass and split afterwards, so footnotes are numbered across the
  document rather than restarting in every block, and a link definition written
  at the foot of the file still reaches the paragraph that uses it. A test
  holds the pieces to exactly what the normal view renders.

### Fixes

- **A document about the diff view is no longer mistaken for one.** The page
  used to decide whether it was showing a diff by searching the body HTML for
  the diff's own class names, which was survivable while a diff body was only
  ever generated rows. It is not survivable now that the body can be your
  prose: a document mentioning `mdview-diff-split` in a code fence would have
  stamped itself a split diff and loaded the wrong half of the stylesheet. The
  layout is passed in now rather than guessed at.

## [v0.15.0](https://github.com/dautroc/mdview/releases/tag/v0.15.0) — 2026-09-02

### What's new

- **The outline says which section you are in.** It listed every heading and
  said nothing about which one you were under, which on anything longer than a
  screen is half a map. The row for the section you are reading is now lit as
  you scroll, and the list scrolls itself just far enough to keep that row
  visible. The rule for which heading you are "on" is the one `]` and `[`
  already use, so pressing `]` lights the row it took you to rather than the
  one after it, and the two cannot drift apart.
- **It does not fight you for the list.** The outline is only nudged when the
  current heading changes, so scrolling it by hand to look somewhere else
  stays where you put it, and the sidebar's own scroll is moved rather than
  the document's — a panel that scrolled the page out from under you would be
  worse than one that lost your place.
- **`g m` maps the whole document down the right edge.** The outline says what
  the headings are; what it cannot show is shape — how long a section runs,
  where the code and the pictures are, how much is still below you. A
  scrollbar knows all of that and shows none of it. Headings are bars, shorter
  as they nest; prose is the lines it is made of; code and tables are blocks;
  images and diagrams are frames. Click it to jump, drag it to scrub.
- **Your comments and your search are on it.** Comments down one edge and
  find's matches down the other, so a passage that is both does not hide one
  mark under the other, and a search you have just run reads as a shape rather
  than as a count.
- **It is structure, not a photograph.** A scaled clone of the page, the way an
  editor does it, would duplicate every diagram, equation and image, would be
  rebuilt on every save, and would show prose as a uniform grey — because
  prose has no shape at that scale. Code does, and headings do, and those are
  what you navigate by.
- **It costs the text no width.** The strip floats over the margin rather than
  taking a column, so turning it on does not re-wrap what you are reading, and
  the comment rail is told what it took rather than left to collide with it.
  It stays out of the diff, which has no shape to map.
- **The cursor follows the view, as the view already followed the cursor.** A
  wheel, a half page or a drag on the minimap used to leave the cursor on a
  paragraph you had scrolled past, so the next `j` carried on from there
  instead of from what you were reading. It is now dragged to the edge you
  scrolled towards — only at the edges, never recentred, so a scroll of two
  lines still leaves it alone. This is what `⌃e` and `⌃y` have always done in
  vim: leave the cursor be until it would scroll off the window.
- **Not while you are choosing something.** The cursor does not follow a
  scroll in visual mode, where it would extend the selection `c` and `y` are
  about to act on, and never appears for a reader who has not used it — a
  scroll is not the moment to hand somebody a caret they did not ask for.

## [v0.14.0](https://github.com/dautroc/mdview/releases/tag/v0.14.0) — 2026-09-02

### What's new

- **`:` opens the commands in a palette.** A single-key app has one thing it
  cannot do: be searched. `?` answers "what does this key do" and nothing
  answered "what is the key for this", so a command you had not memorised was
  findable only by reading the whole sheet. `:` — vim's own key for typing a
  command name — opens a palette on the shell the themes and the recent files
  already use: type a few letters of what you want, enter runs it, and the key
  is printed beside every row, so you leave knowing it.
- **Its rows are the cheat sheet's rows.** The palette walks the same table the
  `?` sheet prints, when it opens. A command is in it because it is documented,
  and the two cannot disagree about what exists.
- **The vim alphabet is not in it.** `j`, `w`, `/`, `y` and the rest are keys
  you already have; twenty-five rows of vim would bury the twenty-eight
  commands that are MDView's own. They are marked in the table and skipped
  there, and the `?` sheet still prints every one — it is the map of the whole
  keyboard, and the palette is the list of things you might not know exist.
  What stays in is what this app invented, even where the idea is vim-shaped:
  `d` and `u`, `s`, and the heading keys.
- **Typing narrows on the group as well as the label.** "scroll" finds the six
  scrolling commands, none of which have the word in their name. The heading is
  searched without being printed, because printing "Scrolling" on four adjacent
  rows would spend the width the labels need.
- **A command still acts on your selection.** A window has exactly one
  selection and the palette's search field destroys it by taking the focus, so
  the palette paints it back on the way out — `v`, then `:`, then "Copy the
  selection" copies what `y` would have.
- **`d` and `u` are half a page down and up.** They were held back on the
  argument that they are vim's delete and undo, which does not survive contact
  with a viewer that has no editing: neither key meant anything, and the
  half-paging they were kept clear for was sitting on `⌃d` and `⌃u` — a
  modifier, in an app whose premise is that a command is one key. `⌃d` and `⌃u`
  still work.

## [v0.13.0](https://github.com/dautroc/mdview/releases/tag/v0.13.0) — 2026-09-02

### What's new

- **`g r` opens the recent files in a palette.** File > Open Recent already
  held the last fifty documents, and a native menu is the wrong shape for fifty
  of anything: it shows them one at a time, to a mouse, when the thing you know
  about the file you want is a few letters of its name. The history now gets
  the same treatment the themes got — an overlay you type into, on the same
  shell, with the same keys. Type to narrow, arrow through it, enter opens,
  esc closes.
- **The document you are reading is not in the list.** It is the one row that
  could do nothing, and it would otherwise sit under the highlight the moment
  the palette opens, which is where enter lands. With it gone, `g r` and enter
  is "back to the one before this" without reading the list at all.
- **A row is the name over the folder it is in**, with your home directory
  written `~`, because two documents called `README.md` are only tellable apart
  by where they live. The folder is part of the row rather than only of the
  tooltip so that the filter reads it too: `other` finds
  `~/work/other-project/README.md`.
- **A file that has gone is hidden, not forgotten.** Entries whose file is
  missing are dropped from the display only — the rule Open Recent already
  follows — so an unmounted volume does not silently erase the history.

### Fixes

- **A long selection was refused rather than elided.** The cap on a comment's
  quote was 400 characters and carried no argument for it. It was not an
  anchoring constraint — the search does not care how long a quote is — so it
  was refusing selections the machinery would have anchored fine. What it was
  really holding up was the layout: three surfaces show a quote, and each
  decided for itself, or not at all, whether to cut it down. They now share one
  elider, which flattens whitespace before cutting so the budget is spent on
  words rather than on a code block's indentation, and the cap is 4000.
- **A note on three words no longer strikes through the comment about the
  passage containing them.** Two comments cannot both highlight an overlapping
  span, and the winner used to fall out of the order the highlights were
  wrapped in, which meant the later-starting one claimed first — exactly
  backwards for nesting. The claim is now its own pass, widest first, so
  enclosure wins by construction.
- **A key hint in the cheat sheet was painted over by its own label.** The keys
  column was a fixed width, and a hint wider than it does not wrap or clip: it
  overflows, and the label is drawn on top of it. The zoomed image's `↑ ↓ ← →
  h j k l` was over that width, so "Pan" was printed across the keys and the
  row read as a string of nonsense characters. The column is now a floor rather
  than a ceiling, and a wide hint pushes its label right instead.

### Documentation

- **The cheat sheet lists the keys you press, and nothing else.** It is read to
  find the one key you do not know, so `esc`, `enter`, the arrows and `hjkl`
  were spending rows for no information, and `click` is not a key at all. Those
  rows are gone, from the sheet and from the README's table, except where the
  line carries a surprise rather than a key: the theme palette previews as you
  arrow through it, and enter in the find field hands the keyboard back to the
  document. Nothing else said either. The sheet is about a screen shorter.

## [v0.12.0](https://github.com/dautroc/mdview/releases/tag/v0.12.0) — 2026-09-02

### What's new

- **A frontmatter block comes off before the document is rendered.** Every
  note written in Obsidian, Jekyll or Hugo opens with a fenced block of YAML
  or TOML addressed to the tool that wrote it rather than to the reader.
  MDView rendered it, and not merely as stray text: the opening `---` is a
  thematic break, the lines under it are a paragraph, and the closing `---`
  turns that paragraph into a setext heading. So a note beginning `title: My
  Note` drew a rule, printed its own metadata, and then put an `<h2>` reading
  `title: My Note` at the top of the outline sidebar, above the document's own
  title. The metadata did not just clutter the page; it outranked the
  document. `---` is YAML and `+++` is TOML, and both are recognised.
- **A rule is still a rule.** The rules are the ones the writing tools
  themselves apply: the opening fence has to be the file's very first line,
  the closing fence is the same three characters alone on a line, and a block
  with no closing fence is not a block at all — a document that genuinely
  opens on a thematic break keeps it. A `---` anywhere below the first line is
  left alone entirely, so a rule between two sections, and a setext heading
  underlined with dashes, both render as they always did.
- **Nothing between the fences is inspected.** Validating the block as YAML
  would mean deciding what to do with a block that does not parse, and the
  only answer — show the reader a page of raw metadata — is worse than the
  alternative in every case we could construct. It also means a nested list
  under `tags:` comes off with the rest, which the line-by-line version of
  this would have got wrong.
- **The diff still shows the file as it is on disk.** The strip happens on the
  way into the renderer rather than on the way out of the file, so `g d` keeps
  showing the frontmatter, and an edit to it shows up as a change. A diff that
  hid one would be lying about the file.

## [v0.11.1](https://github.com/dautroc/mdview/releases/tag/v0.11.1) — 2026-09-01

### What's new

- **`h` `j` `k` `l` pan a zoomed image or diagram.** The lightbox was the one
  place left where the vim keys stopped working: `z` opened a diagram and then
  only the arrows moved it, so the hand that had just pressed `z` had to travel
  for the rest. They pan exactly as the arrows do — the same nudge, the same
  direction — and `l` still cycles the diff layout everywhere the lightbox is
  not up, because the document already hands the keyboard over while one is
  open. `?` lists them alongside the arrows.

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
