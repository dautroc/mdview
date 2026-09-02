//! Pure application-state logic: history, bookmarks, and the page bridge's
//! wire format. No AppKit, no I/O — everything here is unit-tested.

use mdcore::{DiffLayout, Theme};

pub fn resolve_diff_layout(stored: Option<&str>) -> DiffLayout {
    match stored {
        Some("split") => DiffLayout::Split,
        _ => DiffLayout::Unified,
    }
}

pub fn diff_layout_wire(layout: DiffLayout) -> &'static str {
    match layout {
        DiffLayout::Unified => "unified",
        DiffLayout::Split => "split",
    }
}

/// The stored theme, or System when nothing is stored. `Theme::from_wire` maps
/// anything unrecognised to System, so this cannot fail; it exists so the menu
/// bar and the page cannot disagree about what "no stored value" means.
#[allow(dead_code)]
pub fn resolve_theme(stored: Option<&str>) -> Theme {
    Theme::from_wire(stored.unwrap_or(""))
}

/// The hint is shown once ever. Absent means never shown, which is the only
/// state a fresh install can be in.
#[allow(dead_code)]
pub fn should_show_shortcuts_hint(stored: Option<bool>) -> bool {
    !stored.unwrap_or(false)
}

#[allow(dead_code)]
pub fn resolve_full_width(stored: Option<bool>) -> bool {
    stored.unwrap_or(false)
}

#[allow(dead_code)]
pub fn next_full_width(stored: Option<bool>) -> bool {
    !resolve_full_width(stored)
}

#[allow(dead_code)]
pub fn full_width_script(enabled: bool) -> &'static str {
    if enabled {
        "document.documentElement.setAttribute('data-fullwidth','1');"
    } else {
        "document.documentElement.removeAttribute('data-fullwidth');"
    }
}

pub fn queue_full_width_script(scripts: &mut Vec<String>, enabled: bool) {
    scripts.retain(|script| {
        script != full_width_script(true) && script != full_width_script(false)
    });
    scripts.push(full_width_script(enabled).to_string());
}

pub const SIDEBAR_WIDTH_DEFAULT: u32 = 260;
pub const SIDEBAR_WIDTH_MIN: u32 = 160;
pub const SIDEBAR_WIDTH_MAX: u32 = 600;

#[allow(dead_code)]
pub fn clamp_sidebar_width(px: u32) -> u32 {
    px.clamp(SIDEBAR_WIDTH_MIN, SIDEBAR_WIDTH_MAX)
}

#[allow(dead_code)]
pub fn resolve_sidebar_width(stored: Option<i64>) -> u32 {
    match stored {
        Some(v) if v > 0 => clamp_sidebar_width(v as u32),
        _ => SIDEBAR_WIDTH_DEFAULT,
    }
}

#[allow(dead_code)]
pub fn sidebar_width_script(px: u32) -> String {
    format!(
        "window.mdviewSetSidebarWidth && window.mdviewSetSidebarWidth({});",
        clamp_sidebar_width(px)
    )
}

#[allow(dead_code)]
pub fn queue_sidebar_width_script(scripts: &mut Vec<String>, px: u32) {
    let needle = "window.mdviewSetSidebarWidth";
    scripts.retain(|s| !s.contains(needle));
    scripts.push(sidebar_width_script(px));
}

/// Scripts for the page's find bar. The page owns the search itself; the app
/// only relays the standard macOS Find shortcuts into it. Each guards on the
/// hook existing, because the error page has no init.js behind it.
#[allow(dead_code)]
pub fn open_find_script() -> &'static str {
    "window.mdviewOpenFind && window.mdviewOpenFind();"
}

#[allow(dead_code)]
pub fn find_step_script(forward: bool) -> &'static str {
    if forward {
        "window.mdviewFindNext && window.mdviewFindNext();"
    } else {
        "window.mdviewFindPrevious && window.mdviewFindPrevious();"
    }
}

/// The page owns the keyboard cheat sheet, the same way it owns the find bar;
/// the Help menu item only asks for it. Guarded for the error page, which has
/// no init.js behind it.
#[allow(dead_code)]
pub fn shortcuts_script() -> &'static str {
    "window.mdviewToggleShortcuts && window.mdviewToggleShortcuts();"
}

/// The one-time nudge that tells a first-time user the keys exist. With no
/// buttons on the page there is nothing else to notice, so this is the only
/// thing standing between a new user and an apparently inert window.
#[allow(dead_code)]
pub fn shortcuts_hint_script() -> &'static str {
    "window.mdviewShowShortcutsHint && window.mdviewShowShortcutsHint();"
}

/// Put `path` at the front of `list`, promoting an existing entry rather than
/// duplicating it, and truncate to `cap`.
#[allow(dead_code)]
pub fn push_history(list: &[String], path: &str, cap: usize) -> Vec<String> {
    let mut out = Vec::with_capacity(list.len() + 1);
    out.push(path.to_string());
    out.extend(list.iter().filter(|p| p.as_str() != path).cloned());
    out.truncate(cap);
    out
}

/// The name a recent-files row is labelled with. A path with no last
/// component -- only `/` has none -- falls back to the whole thing rather than
/// to an empty row.
#[allow(dead_code)]
pub fn recent_label(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// The directory shown under that name, with the user's home written `~`.
/// Two documents called `README.md` are only tellable apart by where they
/// live, and the palette is filtered on this text as well as on the name.
#[allow(dead_code)]
pub fn recent_dir(path: &str, home: Option<&str>) -> String {
    let Some(parent) = std::path::Path::new(path).parent() else {
        return String::new();
    };
    let parent = parent.to_string_lossy().into_owned();
    let Some(home) = home.map(|h| h.trim_end_matches('/')).filter(|h| !h.is_empty()) else {
        return parent;
    };
    if parent == home {
        return "~".to_string();
    }
    // Only a whole segment counts: with a home of `/Users/bo`, the trailing
    // slash is what keeps `/Users/bobby/notes` from being written `~bby/notes`.
    match parent.strip_prefix(&format!("{home}/")) {
        Some(rest) => format!("~/{rest}"),
        None => parent,
    }
}

/// Add `path` if absent, remove it if present.
#[allow(dead_code)]
pub fn toggle_bookmark(list: &[String], path: &str) -> Vec<String> {
    if is_bookmarked(list, path) {
        list.iter().filter(|p| p.as_str() != path).cloned().collect()
    } else {
        let mut out = Vec::with_capacity(list.len() + 1);
        out.push(path.to_string());
        out.extend(list.iter().cloned());
        out
    }
}

#[allow(dead_code)]
pub fn is_bookmarked(list: &[String], path: &str) -> bool {
    list.iter().any(|p| p.as_str() == path)
}

/// One message from the page. The wire format is a plain `kind:payload`
/// string rather than JSON, so no JSON dependency is needed to read it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Message {
    SetTheme(Theme, Option<u32>),
    ToggleBookmark,
    ToggleDiff,
    SetDiffLayout(DiffLayout),
    ToggleFullWidth,
    OpenPath(String),
    SetSidebar { open: bool, tab: String },
    SetSidebarWidth(u32),
    /// The page's single-key shortcuts for actions only the host can perform:
    /// reloading from disk, and page zoom, which lives on the WKWebView.
    ReloadDocument,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    /// A new comment on the current document. `heading` and `nth` are its
    /// anchor; see `crate::review::Comment`.
    AddComment { heading: usize, nth: usize, quote: String, note: String },
    EditComment { id: String, note: String },
    DeleteComment { id: String },
    CopyReview,
}

/// Parse a bridge message. Returns `None` for anything unrecognised or
/// incomplete — a malformed message from the page is ignored, never fatal.
#[allow(dead_code)]
pub fn parse_message(raw: &str) -> Option<Message> {
    if raw == "toggleBookmark" {
        return Some(Message::ToggleBookmark);
    }
    if raw == "toggleDiff" {
        return Some(Message::ToggleDiff);
    }
    if raw == "toggleFullWidth" {
        return Some(Message::ToggleFullWidth);
    }
    if raw == "reloadDocument" {
        return Some(Message::ReloadDocument);
    }
    if raw == "zoomIn" {
        return Some(Message::ZoomIn);
    }
    if raw == "zoomOut" {
        return Some(Message::ZoomOut);
    }
    if raw == "zoomReset" {
        return Some(Message::ZoomReset);
    }
    if raw == "copyReview" {
        return Some(Message::CopyReview);
    }
    let (kind, rest) = raw.split_once(':')?;
    match kind {
        "setDiffLayout" => match rest {
            "unified" => Some(Message::SetDiffLayout(DiffLayout::Unified)),
            "split" => Some(Message::SetDiffLayout(DiffLayout::Split)),
            _ => None,
        },
        "setTheme" => {
            // Format: setTheme:<wire> or setTheme:<wire>:<scrollY>
            let (wire, scroll_str) = match rest.split_once(':') {
                Some((w, s)) => (w, Some(s)),
                None => (rest, None),
            };
            let scroll = scroll_str.and_then(|s| s.parse::<u32>().ok());
            Some(Message::SetTheme(Theme::from_wire(wire), scroll))
        }
        "openPath" => {
            if rest.is_empty() {
                None
            } else {
                Some(Message::OpenPath(rest.to_string()))
            }
        }
        "setSidebar" => {
            let (open, tab) = rest.split_once(':')?;
            if tab.is_empty() {
                return None;
            }
            Some(Message::SetSidebar {
                open: open == "1",
                tab: tab.to_string(),
            })
        }
        "setSidebarWidth" => {
            let px = rest.parse::<u32>().ok()?;
            Some(Message::SetSidebarWidth(px))
        }
        // A comment carries two free-text fields, and `openPath` only gets
        // away with one because it is last. The page percent-encodes both, so
        // every field here is delimiter-free by construction.
        "addComment" => {
            let parts: Vec<&str> = rest.splitn(4, ':').collect();
            let [heading, nth, quote, note] = parts.as_slice() else {
                return None;
            };
            Some(Message::AddComment {
                heading: heading.parse().ok()?,
                nth: nth.parse().ok()?,
                quote: percent_decode(quote)?,
                note: percent_decode(note)?,
            })
        }
        "editComment" => {
            let (id, note) = rest.split_once(':')?;
            if id.is_empty() {
                return None;
            }
            Some(Message::EditComment { id: id.to_string(), note: percent_decode(note)? })
        }
        "deleteComment" => {
            if rest.is_empty() {
                None
            } else {
                Some(Message::DeleteComment { id: rest.to_string() })
            }
        }
        _ => None,
    }
}

/// Decode one percent-encoded field from the page.
///
/// Bytes are collected and validated as UTF-8 only at the end, never decoded a
/// `char` at a time: `encodeURIComponent` emits UTF-8 as percent bytes, so a
/// char-wise decoder turns every emoji and accented letter into mojibake.
/// `None` for a truncated or non-hex escape, so a malformed message is dropped
/// rather than half-read.
#[allow(dead_code)]
pub fn percent_decode(text: &str) -> Option<String> {
    fn hex(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let high = hex(*bytes.get(at + 1)?)?;
            let low = hex(*bytes.get(at + 2)?)?;
            out.push(high * 16 + low);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The file name a document's review is stored under: FNV-1a of its
/// canonicalized path, in hex.
///
/// Deliberately not `DefaultHasher`, which `page.rs` uses for the CSP nonce
/// precisely because `RandomState` seeds it per process. A file name built on
/// that would point somewhere new on every launch, losing every comment.
#[allow(dead_code)]
pub fn review_file_name(canonical_path: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in canonical_path.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}.md")
}

/// The one line `C` puts on the pasteboard. Absolute paths on purpose: it is
/// pasted into a Claude session whose working directory is unknown here.
///
/// It asks for the addressed records to be deleted, because otherwise the
/// natural end state of a successful edit is a document full of comments whose
/// quoted text no longer exists — addressing a comment usually means rewriting
/// exactly the passage it points at, which orphans it. The app watches the
/// review file, so a deleted record leaves the margin as soon as it is written.
///
/// The shape of a record is spelled out rather than left to be inferred: a
/// half-deleted fence makes its record unparseable, and an unparseable record
/// is skipped and then lost on the next write.
///
/// ONE LINE, always. Pasting into Claude Code submits on the first newline, so
/// a prompt with one in it sends half of itself.
#[allow(dead_code)]
pub fn review_prompt(review_path: &str, doc_path: &str) -> String {
    format!(
        "Please address the review comments in {review_path} — they are my notes on \
         {doc_path}. Each comment is a `~~~~ mdview-quote` fenced block holding the \
         passage I selected, optionally followed by a `~~~~ mdview-note` block with my \
         note. When you have addressed one, delete both of its fenced blocks from the \
         review file, opening and closing fences included, and leave the rest of the \
         file alone."
    )
}

/// Why `D` will not open the diff, or `None` when it will.
///
/// The key refused in silence, which in this app means it reads as broken:
/// every other refusal says why. The three reasons are worth telling apart —
/// a file outside a repository, a file the repository is not tracking, and a
/// repository with nothing to diff against are three different things to go
/// and fix.
#[allow(dead_code)]
pub fn diff_unavailable_note(availability: mdcore::DiffAvailability) -> Option<&'static str> {
    match availability {
        mdcore::DiffAvailability::Available => None,
        mdcore::DiffAvailability::GitUnavailable => Some("This file is not in a Git repository."),
        mdcore::DiffAvailability::Untracked => Some("Git is not tracking this file."),
        mdcore::DiffAvailability::NoHead => Some("This repository has no commits to diff against."),
    }
}

/// The banner for a review file MDView could not wholly read, or `None` when
/// it read cleanly.
///
/// Says the consequence, not just the fault: the file is about to stop being
/// written to, and nothing else on screen would show that. `C` asks Claude to
/// delete records, so a half-deleted one is the likely cause and the line
/// number is what makes it findable.
#[allow(dead_code)]
pub fn damaged_review_banner(
    damage: &[crate::review::Damage],
    review_path: &str,
) -> Option<String> {
    let [first, rest @ ..] = damage else {
        return None;
    };
    if rest.is_empty() {
        return Some(format!(
            "Line {} of the review file cannot be read: {}. \
             MDView will not write to the file until it is fixed — {review_path}",
            first.line, first.reason,
        ));
    }
    let lines: Vec<String> = damage.iter().map(|d| d.line.to_string()).collect();
    Some(format!(
        "{} records in the review file cannot be read, at lines {}. \
         The first: {}. \
         MDView will not write to the file until they are fixed — {review_path}",
        damage.len(),
        lines.join(", "),
        first.reason,
    ))
}

/// Hand the page its comment list. Built here rather than in `app.rs` because
/// it is a pure function of pure inputs, and the objc layer has no tests.
#[allow(dead_code)]
pub fn comments_script(comments: &[crate::review::Comment]) -> String {
    let items: Vec<String> = comments
        .iter()
        .map(|comment| {
            format!(
                "{{id:{},heading:{},nth:{},quote:{},note:{}}}",
                mdcore::escape::js_string_literal(&comment.id),
                comment.heading,
                comment.nth,
                mdcore::escape::js_string_literal(&comment.quote),
                mdcore::escape::js_string_literal(&comment.note),
            )
        })
        .collect();
    // Guarded like every other injected call: the error page has no init.js.
    format!("window.mdviewSetComments && window.mdviewSetComments([{}]);", items.join(","))
}

/// The recent-files list for one window: the history, with that window's own
/// document taken out, each entry carrying the name and folder the palette
/// shows and filters on.
///
/// The current document is dropped because the palette is a way to somewhere
/// else -- it is the one row that could do nothing, and it would sit under the
/// highlight the moment the palette opens, which is where enter lands.
#[allow(dead_code)]
pub fn recents_script(history: &[String], current: &str, home: Option<&str>) -> String {
    let items: Vec<String> = history
        .iter()
        .filter(|path| path.as_str() != current)
        .map(|path| {
            format!(
                "{{name:{},dir:{},path:{}}}",
                mdcore::escape::js_string_literal(&recent_label(path)),
                mdcore::escape::js_string_literal(&recent_dir(path, home)),
                mdcore::escape::js_string_literal(path),
            )
        })
        .collect();
    // Guarded like every other injected call: the error page has no init.js.
    format!("window.mdviewSetRecents && window.mdviewSetRecents([{}]);", items.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdcore::Theme;

    /// The two fields the wire could not carry unencoded. A quote lifted out
    /// of a document is full of colons, and `parse_message` splits on them.
    #[test]
    fn a_quote_containing_a_colon_and_a_newline_survives_the_wire() {
        let raw = "addComment:3:1:note%3A%20line%201%0Aand%20line%202:why%3F";
        assert_eq!(
            parse_message(raw),
            Some(Message::AddComment {
                heading: 3,
                nth: 1,
                quote: "note: line 1\nand line 2".to_string(),
                note: "why?".to_string(),
            })
        );
    }

    /// `encodeURIComponent` emits UTF-8 as percent bytes, so a decoder working
    /// a `char` at a time returns mojibake for anything outside ASCII.
    #[test]
    fn an_emoji_survives_the_wire() {
        assert_eq!(percent_decode("%F0%9F%8E%AF").as_deref(), Some("🎯"));
        assert_eq!(percent_decode("caf%C3%A9").as_deref(), Some("café"));
        assert_eq!(percent_decode("100%25%20done").as_deref(), Some("100% done"));
    }

    #[test]
    fn a_malformed_percent_escape_is_ignored_not_fatal() {
        for bad in ["%", "%4", "%ZZ", "%2", "a%", "%C3"] {
            assert_eq!(percent_decode(bad), None, "{bad:?} should not decode");
        }
        // And the message carrying one is dropped rather than half-read.
        assert_eq!(parse_message("addComment:1:0:%ZZ:note"), None);
    }

    #[test]
    fn a_comment_message_needs_all_four_of_its_fields() {
        assert_eq!(parse_message("addComment:1:0:quote"), None);
        assert_eq!(parse_message("addComment:x:0:q:n"), None);
        assert_eq!(parse_message("editComment:"), None);
        assert_eq!(parse_message("editComment::note"), None);
        assert_eq!(parse_message("deleteComment:"), None);
    }

    /// The name is a path that has to point at the same file next launch. The
    /// literal is the guard: a seeded hasher would satisfy every property you
    /// could state about this function and still lose every comment on quit.
    #[test]
    fn review_file_name_is_stable_across_processes() {
        assert_eq!(review_file_name(""), "cbf29ce484222325.md");
        assert_eq!(review_file_name("/tmp/notes.md").len(), "cbf29ce484222325.md".len());
        assert_eq!(review_file_name("/tmp/notes.md"), review_file_name("/tmp/notes.md"));
    }

    #[test]
    fn two_documents_do_not_share_a_review_file() {
        assert_ne!(review_file_name("/tmp/a.md"), review_file_name("/tmp/b.md"));
    }

    #[test]
    fn the_review_prompt_is_one_line_naming_both_absolute_paths() {
        let prompt = review_prompt("/Users/x/Library/r/9.md", "/Users/x/notes.md");
        assert!(prompt.contains("/Users/x/Library/r/9.md"));
        assert!(prompt.contains("/Users/x/notes.md"));
        // Claude Code submits on the first newline: a prompt containing one
        // would send half of itself and act on an instruction cut in two.
        assert!(!prompt.contains('\n'), "a pasted prompt has to be one line");
        // The line-continuations that keep this readable in the source must
        // not leave doubled spaces in what is actually pasted.
        assert!(!prompt.contains("  "), "the source wrapping leaked into the prompt");
    }

    /// Addressing a comment means rewriting the passage it quotes, which
    /// orphans it — so a review that is acted on and never pruned leaves every
    /// comment behind, struck through. Asking for the record to go is what
    /// makes "done" mean something, and naming the fences is what stops a
    /// half-deleted record from being silently dropped on the next write.
    #[test]
    fn the_review_prompt_asks_for_addressed_records_to_be_deleted() {
        let prompt = review_prompt("/Users/x/Library/r/9.md", "/Users/x/notes.md");
        assert!(prompt.contains("delete"), "nothing asks for the record to go");
        assert!(prompt.contains("mdview-quote"), "the record shape is left to guesswork");
        assert!(prompt.contains("mdview-note"));
        assert!(prompt.contains("fences included"), "a half-deleted record parses as nothing");
        assert!(prompt.contains("leave the rest of the file alone"));
    }

    /// `D` used to do nothing at all when there was no diff, which in an app
    /// whose every other refusal explains itself reads as a broken key.
    #[test]
    fn every_reason_the_diff_is_unavailable_has_something_to_say() {
        use mdcore::DiffAvailability::*;
        assert_eq!(diff_unavailable_note(Available), None, "an available diff must not nag");
        for unavailable in [GitUnavailable, Untracked, NoHead] {
            let note = diff_unavailable_note(unavailable).expect("{unavailable:?} says nothing");
            assert!(note.ends_with('.'), "{unavailable:?}: {note}");
            assert!(note.len() < 60, "too long for the note strip: {note}");
        }
        // Three different things to go and fix, so three different sentences.
        assert_ne!(diff_unavailable_note(GitUnavailable), diff_unavailable_note(Untracked));
        assert_ne!(diff_unavailable_note(Untracked), diff_unavailable_note(NoHead));
    }

    /// A clean file must not raise anything: the banner is a condition someone
    /// has to resolve, and one that appears for every document would be noise
    /// nobody reads by the time it means something.
    #[test]
    fn a_review_that_read_cleanly_raises_no_banner() {
        assert_eq!(damaged_review_banner(&[], "/Users/x/r/9.md"), None);
    }

    /// The banner has to say the CONSEQUENCE. Nothing else on screen would
    /// show that comments have quietly stopped being saved, and a user who
    /// reads this as cosmetic will keep typing notes into a file that is not
    /// being written.
    #[test]
    fn the_damaged_review_banner_names_the_line_the_reason_and_the_consequence() {
        let one = [crate::review::Damage { line: 12, reason: crate::review::ORPHAN_NOTE }];
        let text = damaged_review_banner(&one, "/Users/x/r/9.md").expect("a banner");
        assert!(text.contains("Line 12"), "no line number: {text}");
        assert!(text.contains(crate::review::ORPHAN_NOTE));
        assert!(text.contains("will not write"), "does not say what stopped: {text}");
        assert!(text.contains("/Users/x/r/9.md"), "nothing to open: {text}");
    }

    #[test]
    fn a_review_with_several_bad_records_counts_them_and_lists_the_lines() {
        let many = [
            crate::review::Damage { line: 12, reason: crate::review::ORPHAN_NOTE },
            crate::review::Damage { line: 31, reason: crate::review::BAD_INFO },
        ];
        let text = damaged_review_banner(&many, "/Users/x/r/9.md").expect("a banner");
        assert!(text.contains("2 records"), "no count: {text}");
        assert!(text.contains("lines 12, 31"), "no lines: {text}");
        // The first reason only: a banner listing every one would not be read.
        assert!(text.contains(crate::review::ORPHAN_NOTE));
        assert!(!text.contains(crate::review::BAD_INFO));
        assert!(text.contains("will not write"));
    }

    /// Same hazard as the bookmarks list: a quote is arbitrary document text,
    /// and an unescaped `</script>` or newline in one would break the page.
    #[test]
    fn the_comments_script_escapes_every_field_and_is_guarded() {
        let comments = [crate::review::Comment::new("1", 2, 0, "</script>\n\"q\"", "note")];
        let script = comments_script(&comments);
        assert!(script.contains("window.mdviewSetComments &&"));
        assert!(!script.contains("</script>"), "the literal has to be escaped");
        assert!(!script.contains('\n'), "a raw newline would break the literal");
        assert!(script.contains("heading:2"));
    }

    #[test]
    fn an_empty_comment_list_still_reaches_the_page() {
        // Otherwise deleting the last comment would leave its highlight up.
        assert_eq!(
            comments_script(&[]),
            "window.mdviewSetComments && window.mdviewSetComments([]);"
        );
    }

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// Every `postToHost("…")` in the page has to be a message this parser
    /// accepts. Nothing else catches a typo on either side: an unparsed
    /// message is dropped in silence, so the key would simply do nothing.
    #[test]
    fn every_message_the_page_sends_is_one_the_bridge_understands() {
        let js = mdcore::assets::INIT_JS;
        let mut seen = 0;
        let mut rest = js;
        while let Some(at) = rest.find("postToHost(\"") {
            rest = &rest[at + "postToHost(\"".len()..];
            let Some(end) = rest.find('"') else { break };
            let literal = &rest[..end];
            seen += 1;
            // The ones built by concatenation ("setTheme:" + id) carry their
            // payload at runtime; give the parser a representative one.
            let sample = match literal {
                "setTheme:" => "setTheme:mocha:0".to_string(),
                "setSidebar:" => "setSidebar:1:outline".to_string(),
                "setSidebarWidth:" => "setSidebarWidth:260".to_string(),
                "setDiffLayout:" => "setDiffLayout:split".to_string(),
                "openPath:" => "openPath:/tmp/x.md".to_string(),
                "addComment:" => "addComment:1:0:a%20quote:a%20note".to_string(),
                "editComment:" => "editComment:2f:a%20note".to_string(),
                "deleteComment:" => "deleteComment:2f".to_string(),
                other => other.to_string(),
            };
            assert!(
                parse_message(&sample).is_some(),
                "the page sends {literal:?}, which the bridge drops"
            );
        }
        assert!(seen >= 8, "expected to find the page's messages, saw {seen}");
    }

    #[test]
    fn the_cheat_sheet_script_calls_the_pages_own_hook_and_is_guarded() {
        assert!(shortcuts_script().contains("mdviewToggleShortcuts"));
        // Same hazard as the find scripts: the error page has no init.js.
        assert!(shortcuts_script().contains("&&"));
    }

    #[test]
    fn the_hint_script_calls_the_pages_own_hook_and_is_guarded() {
        assert!(shortcuts_hint_script().contains("mdviewShowShortcutsHint"));
        assert!(shortcuts_hint_script().contains("&&"));
    }

    #[test]
    fn an_unset_theme_resolves_to_system() {
        assert_eq!(resolve_theme(None), Theme::System);
        assert_eq!(resolve_theme(Some("")), Theme::System);
        // Anything unrecognised, including a theme that has since been removed.
        assert_eq!(resolve_theme(Some("no-such-theme")), Theme::System);
    }

    #[test]
    fn a_stored_theme_round_trips_through_its_wire_value() {
        for theme in Theme::all() {
            assert_eq!(resolve_theme(Some(theme.as_wire())), *theme);
        }
    }

    #[test]
    fn the_hint_shows_only_until_it_has_been_shown_once() {
        // A fresh install has never written the key at all.
        assert!(should_show_shortcuts_hint(None));
        assert!(should_show_shortcuts_hint(Some(false)));
        assert!(!should_show_shortcuts_hint(Some(true)));
    }

    #[test]
    fn find_scripts_call_the_pages_own_hooks_and_are_guarded() {
        assert!(open_find_script().contains("mdviewOpenFind"));
        assert!(find_step_script(true).contains("mdviewFindNext"));
        assert!(find_step_script(false).contains("mdviewFindPrevious"));
        for script in [open_find_script(), find_step_script(true), find_step_script(false)] {
            // The error page has no init.js behind it: an unguarded call would
            // throw there on every press of the shortcut.
            assert!(script.contains("&&"), "{script} must guard on the hook existing");
        }
    }

    #[test]
    fn find_next_and_previous_are_different_scripts() {
        assert_ne!(find_step_script(true), find_step_script(false));
    }

    #[test]
    fn history_puts_the_newest_first() {
        assert_eq!(push_history(&v(&["/a"]), "/b", 50), v(&["/b", "/a"]));
    }

    #[test]
    fn history_promotes_instead_of_duplicating() {
        // Reopening a file must move it to the front, not add a second copy.
        assert_eq!(
            push_history(&v(&["/a", "/b", "/c"]), "/c", 50),
            v(&["/c", "/a", "/b"])
        );
    }

    #[test]
    fn history_is_capped_and_drops_the_oldest() {
        assert_eq!(push_history(&v(&["/a", "/b"]), "/c", 2), v(&["/c", "/a"]));
    }

    #[test]
    fn history_cap_of_zero_yields_nothing() {
        assert!(push_history(&v(&["/a"]), "/b", 0).is_empty());
    }

    #[test]
    fn a_recent_row_is_labelled_with_the_file_name() {
        assert_eq!(recent_label("/Users/bo/notes/README.md"), "README.md");
        // Nothing to take a name from, so the row shows the path itself
        // rather than nothing at all.
        assert_eq!(recent_label("/"), "/");
    }

    #[test]
    fn a_recent_row_shows_its_folder_with_home_written_as_a_tilde() {
        let home = Some("/Users/bo");
        assert_eq!(recent_dir("/Users/bo/notes/README.md", home), "~/notes");
        assert_eq!(recent_dir("/Users/bo/README.md", home), "~");
        assert_eq!(recent_dir("/etc/motd.md", home), "/etc");
        // No HOME to shorten against is not an error; the path stands as it is.
        assert_eq!(recent_dir("/Users/bo/notes/README.md", None), "/Users/bo/notes");
        assert_eq!(recent_dir("/Users/bo/notes/README.md", Some("")), "/Users/bo/notes");
    }

    /// The prefix test has to be segment-wise. A home of `/Users/bo` shortening
    /// `/Users/bobby/notes` would name a directory that does not exist.
    #[test]
    fn a_home_prefix_only_shortens_a_whole_segment() {
        assert_eq!(recent_dir("/Users/bobby/notes/a.md", Some("/Users/bo")), "/Users/bobby/notes");
        // A trailing slash on HOME is the same home.
        assert_eq!(recent_dir("/Users/bo/notes/a.md", Some("/Users/bo/")), "~/notes");
    }

    #[test]
    fn the_recents_script_leaves_out_the_document_the_window_is_showing() {
        let history = v(&["/Users/bo/a.md", "/Users/bo/b.md"]);
        let script = recents_script(&history, "/Users/bo/a.md", Some("/Users/bo"));
        assert!(script.contains("window.mdviewSetRecents &&"));
        assert!(!script.contains("a.md"), "the open document is the one row that could do nothing");
        assert!(script.contains(r#"name:"b.md""#));
        assert!(script.contains(r#"dir:"~""#));
        assert!(script.contains(r#"path:"/Users/bo/b.md""#));
    }

    #[test]
    fn the_recents_script_escapes_every_field_and_survives_an_empty_list() {
        let history = v(&["/tmp/</script>\n\"x\".md"]);
        let script = recents_script(&history, "", None);
        assert!(!script.contains("</script>"), "the literal has to be escaped");
        assert!(!script.contains('\n'), "a raw newline would break the literal");
        // An emptied history still has to reach the page, or the palette would
        // go on offering documents Clear Menu has just thrown away.
        assert_eq!(
            recents_script(&[], "", None),
            "window.mdviewSetRecents && window.mdviewSetRecents([]);"
        );
    }

    #[test]
    fn bookmark_toggles_both_ways() {
        let added = toggle_bookmark(&v(&["/a"]), "/b");
        assert_eq!(added, v(&["/b", "/a"]));
        assert_eq!(toggle_bookmark(&added, "/b"), v(&["/a"]));
    }

    #[test]
    fn is_bookmarked_reports_membership() {
        assert!(is_bookmarked(&v(&["/a", "/b"]), "/b"));
        assert!(!is_bookmarked(&v(&["/a"]), "/b"));
    }

    #[test]
    fn parses_each_message_kind() {
        assert_eq!(parse_message("toggleBookmark"), Some(Message::ToggleBookmark));
        assert_eq!(
            parse_message("setTheme:mocha"),
            Some(Message::SetTheme(Theme::Mocha, None))
        );
        assert_eq!(
            parse_message("openPath:/Users/x/a.md"),
            Some(Message::OpenPath("/Users/x/a.md".into()))
        );
        assert_eq!(parse_message("toggleDiff"), Some(Message::ToggleDiff));
        assert_eq!(
            parse_message("setDiffLayout:split"),
            Some(Message::SetDiffLayout(DiffLayout::Split))
        );
        assert_eq!(parse_message("toggleFullWidth"), Some(Message::ToggleFullWidth));
        assert_eq!(parse_message("reloadDocument"), Some(Message::ReloadDocument));
        assert_eq!(parse_message("zoomIn"), Some(Message::ZoomIn));
        assert_eq!(parse_message("zoomOut"), Some(Message::ZoomOut));
        assert_eq!(parse_message("zoomReset"), Some(Message::ZoomReset));
        assert_eq!(
            parse_message("setSidebar:1:bookmarks"),
            Some(Message::SetSidebar { open: true, tab: "bookmarks".into() })
        );
    }

    #[test]
    fn a_path_containing_a_colon_survives_parsing() {
        // splitn(2) keeps everything after the first colon, colons included.
        assert_eq!(
            parse_message("openPath:/Users/x/a:b.md"),
            Some(Message::OpenPath("/Users/x/a:b.md".into()))
        );
    }

    #[test]
    fn malformed_messages_are_ignored_not_fatal() {
        assert_eq!(parse_message(""), None);
        assert_eq!(parse_message("nonsense"), None);
        assert_eq!(parse_message("openPath:"), None);
        assert_eq!(parse_message("setSidebar:1"), None);
        assert_eq!(parse_message("setTheme:tokyo-night"), Some(Message::SetTheme(Theme::System, None)));
    }

    #[test]
    fn set_theme_carries_an_optional_scroll_offset() {
        assert_eq!(parse_message("setTheme:mocha:1234"),
            Some(Message::SetTheme(Theme::Mocha, Some(1234))));
        assert_eq!(parse_message("setTheme:mocha"),
            Some(Message::SetTheme(Theme::Mocha, None)));
    }

    #[test]
    fn a_malformed_scroll_offset_is_ignored_not_fatal() {
        // The theme still applies; only the scroll hint is dropped.
        assert_eq!(parse_message("setTheme:mocha:abc"),
            Some(Message::SetTheme(Theme::Mocha, None)));
    }

    #[test]
    fn sidebar_closed_state_is_carried_not_assumed() {
        // Pins the `open == "1"` branch against an implementation that
        // hardcodes true: every other setSidebar test passes "1".
        assert_eq!(
            parse_message("setSidebar:0:outline"),
            Some(Message::SetSidebar { open: false, tab: "outline".into() })
        );
    }

    #[test]
    fn an_empty_tab_name_is_rejected() {
        // "setSidebar:1" fails earlier for want of a second colon, so this is
        // the only case that reaches the tab.is_empty() guard. Without it an
        // empty string would reach callers as a real tab name.
        assert_eq!(parse_message("setSidebar:1:"), None);
    }

    #[test]
    fn fullwidth_defaults_to_centered_until_a_value_is_stored() {
        assert!(!resolve_full_width(None));
        assert!(resolve_full_width(Some(true)));
        assert!(!resolve_full_width(Some(false)));
    }

    #[test]
    fn fullwidth_toggle_inverts_the_resolved_value() {
        assert!(next_full_width(None));
        assert!(!next_full_width(Some(true)));
        assert!(next_full_width(Some(false)));
    }

    #[test]
    fn fullwidth_script_sets_or_removes_the_root_attribute() {
        assert_eq!(
            full_width_script(true),
            "document.documentElement.setAttribute('data-fullwidth','1');"
        );
        assert_eq!(
            full_width_script(false),
            "document.documentElement.removeAttribute('data-fullwidth');"
        );
    }

    #[test]
    fn queueing_fullwidth_replaces_a_stale_script_but_keeps_other_scripts_ordered() {
        let mut scripts = vec![
            full_width_script(false).to_string(),
            "sidebar".to_string(),
            full_width_script(true).to_string(),
            "bookmarks".to_string(),
            full_width_script(false).to_string(),
        ];

        queue_full_width_script(&mut scripts, true);

        assert_eq!(
            scripts,
            vec!["sidebar", "bookmarks", full_width_script(true)]
        );
    }

    #[test]
    fn diff_layout_defaults_to_unified_and_round_trips_split() {
        assert_eq!(resolve_diff_layout(None), DiffLayout::Unified);
        assert_eq!(resolve_diff_layout(Some("unified")), DiffLayout::Unified);
        assert_eq!(resolve_diff_layout(Some("split")), DiffLayout::Split);
        assert_eq!(diff_layout_wire(DiffLayout::Unified), "unified");
        assert_eq!(diff_layout_wire(DiffLayout::Split), "split");
    }
}
