//! Pure application-state logic: history, bookmarks, and the page bridge's
//! wire format. No AppKit, no I/O — everything here is unit-tested.

use mdcore::Theme;

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
    SetTheme(Theme),
    ToggleBookmark,
    OpenPath(String),
    SetSidebar { open: bool, tab: String },
}

/// Parse a bridge message. Returns `None` for anything unrecognised or
/// incomplete — a malformed message from the page is ignored, never fatal.
#[allow(dead_code)]
pub fn parse_message(raw: &str) -> Option<Message> {
    if raw == "toggleBookmark" {
        return Some(Message::ToggleBookmark);
    }
    let (kind, rest) = raw.split_once(':')?;
    match kind {
        "setTheme" => Some(Message::SetTheme(Theme::from_wire(rest))),
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
            Some(Message::SetTheme(Theme::Mocha))
        );
        assert_eq!(
            parse_message("openPath:/Users/x/a.md"),
            Some(Message::OpenPath("/Users/x/a.md".into()))
        );
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
        assert_eq!(parse_message("setTheme:tokyo-night"), Some(Message::SetTheme(Theme::System)));
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
}
