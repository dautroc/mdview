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
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdcore::Theme;

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
