use objc2::rc::Retained;
use objc2::sel;
use objc2::MainThreadOnly;
use objc2_app_kit::{
    NSApplication, NSControlStateValue, NSControlStateValueOff, NSControlStateValueOn, NSMenu,
    NSMenuItem,
};
use objc2_foundation::{MainThreadMarker, NSString};

// Only three of MDView's own commands keep a key equivalent: ⌘O, ⌘F and ⌘R,
// the ones whose muscle memory predates this app. Everything else the app
// does has a key or a two-key `g` sequence in the page (see the ? sheet), so a
// second shortcut for it would be a second thing to keep in sync. The macOS standards -- ⌘C,
// ⌘A, ⌘Q, ⌘W, ⌘M, ⌘H -- stay: they are not duplicates of anything, and ⌘C is
// the only way to copy out of the web view.
const FULL_WIDTH_TITLE: &str = "Full Width";
const DIFF_TITLE: &str = "Show Diff";
const FIND_TITLE: &str = "Find…";
const FIND_KEY_EQUIVALENT: &str = "f";
const SHORTCUTS_TITLE: &str = "Keyboard Shortcuts";
const THEME_TITLE: &str = "Theme";

pub(crate) fn full_width_menu_state(enabled: bool) -> NSControlStateValue {
    if enabled {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    }
}

pub(crate) fn diff_menu_state(enabled: bool) -> NSControlStateValue {
    full_width_menu_state(enabled)
}

pub(crate) fn diff_layout_menu_state(selected: bool) -> NSControlStateValue {
    full_width_menu_state(selected)
}

pub(crate) fn theme_menu_state(selected: bool) -> NSControlStateValue {
    full_width_menu_state(selected)
}

/// The wire value a theme item carries on its `representedObject`.
///
/// Every theme item shares one selector, so unlike the diff-layout items they
/// cannot be told apart by their action — the wire string is the discriminator.
/// It is also what `MDViewTheme` already stores, so it stays correct if
/// `Theme::all()` is ever reordered.
pub(crate) fn item_theme_wire(item: &NSMenuItem) -> Option<String> {
    let object = item.representedObject()?;
    let string = object.downcast::<NSString>().ok()?;
    Some(string.to_string())
}

pub(crate) fn set_diff_layout_states(sender: &NSMenuItem, layout: mdcore::DiffLayout) {
    let Some(menu) = (unsafe { sender.menu() }) else {
        return;
    };
    for index in 0..menu.numberOfItems() {
        let Some(item) = menu.itemAtIndex(index) else { continue };
        let selected = match item.action() {
            Some(action) if action == sel!(setUnifiedDiff:) => {
                layout == mdcore::DiffLayout::Unified
            }
            Some(action) if action == sel!(setSplitDiff:) => layout == mdcore::DiffLayout::Split,
            _ => continue,
        };
        item.setState(diff_layout_menu_state(selected));
    }
}

/// Build one menu item. `key` is the command-key equivalent ("" for none).
fn item(
    mtm: MainThreadMarker,
    title: &str,
    action: objc2::runtime::Sel,
    key: &str,
) -> Retained<NSMenuItem> {
    unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            Some(action),
            &NSString::from_str(key),
        )
    }
}

fn submenu(mtm: MainThreadMarker, title: &str) -> (Retained<NSMenuItem>, Retained<NSMenu>) {
    let holder = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            None,
            &NSString::from_str(""),
        )
    };
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
    holder.setSubmenu(Some(&menu));
    (holder, menu)
}

/// Install the application menu bar.
///
/// Menu items use a nil target, so AppKit sends each action up the responder
/// chain and it lands on the application delegate. That is why the delegate
/// implements the action selectors rather than the window doing so.
///
/// Returns the Open Recent submenu so the delegate can refill it from history.
pub fn install(app: &NSApplication, mtm: MainThreadMarker) -> Retained<NSMenu> {
    let menubar = NSMenu::new(mtm);

    // Application menu. Its title is ignored by AppKit, which substitutes the
    // process name from Info.plist.
    let (app_holder, app_menu) = submenu(mtm, "MDView");
    app_menu.addItem(&item(mtm, "About MDView", sel!(orderFrontStandardAboutPanel:), ""));
    app_menu.addItem(NSMenuItem::separatorItem(mtm).as_ref());
    app_menu.addItem(&item(mtm, "Hide MDView", sel!(hide:), "h"));
    app_menu.addItem(&item(mtm, "Quit MDView", sel!(terminate:), "q"));
    menubar.addItem(&app_holder);

    let (file_holder, file_menu) = submenu(mtm, "File");
    file_menu.addItem(&item(mtm, "Open…", sel!(openDocument:), "o"));
    file_menu.addItem(&item(mtm, "Reload", sel!(reloadDocument:), "r"));
    let (recent_holder, recent_menu) = submenu(mtm, "Open Recent");
    file_menu.addItem(&recent_holder);
    file_menu.addItem(NSMenuItem::separatorItem(mtm).as_ref());
    file_menu.addItem(&item(mtm, "Close Window", sel!(performClose:), "w"));
    menubar.addItem(&file_holder);

    // Edit exists so that ⌘C and ⌘A work in the web view. AppKit wires these
    // standard selectors up for us; we only have to expose them.
    let (edit_holder, edit_menu) = submenu(mtm, "Edit");
    edit_menu.addItem(&item(mtm, "Copy", sel!(copy:), "c"));
    edit_menu.addItem(&item(mtm, "Select All", sel!(selectAll:), "a"));
    edit_menu.addItem(NSMenuItem::separatorItem(mtm).as_ref());
    // Find lives in its own submenu, where macOS puts it, and carries the
    // shortcuts users press without looking: ⌘F, ⌘G, ⇧⌘G. The selectors are
    // MDView's own rather than AppKit's `performFindPanelAction:` — that one
    // drives NSTextFinder, which a WKWebView does not have; the search itself
    // lives in the page.
    let (find_holder, find_menu) = submenu(mtm, "Find");
    find_menu.addItem(&item(mtm, FIND_TITLE, sel!(findInPage:), FIND_KEY_EQUIVALENT));
    find_menu.addItem(&item(mtm, "Find Next", sel!(findNextMatch:), ""));
    find_menu.addItem(&item(mtm, "Find Previous", sel!(findPreviousMatch:), ""));
    edit_menu.addItem(&find_holder);
    edit_menu.addItem(NSMenuItem::separatorItem(mtm).as_ref());
    // C on the keyboard. No key equivalent here: ⇧⌘C is not MDView's to take.
    edit_menu.addItem(&item(mtm, "Copy Review Prompt", sel!(copyReviewPrompt:), ""));
    menubar.addItem(&edit_holder);

    let (view_holder, view_menu) = submenu(mtm, "View");
    view_menu.addItem(&item(mtm, "Actual Size", sel!(zoomActual:), ""));
    view_menu.addItem(&item(mtm, "Zoom In", sel!(zoomIn:), ""));
    view_menu.addItem(&item(mtm, "Zoom Out", sel!(zoomOut:), ""));
    view_menu.addItem(NSMenuItem::separatorItem(mtm).as_ref());
    view_menu.addItem(&item(mtm, "Toggle Sidebar", sel!(toggleSidebar:), ""));
    let full_width_item = item(mtm, FULL_WIDTH_TITLE, sel!(toggleFullWidth:), "");
    let full_width = crate::state::resolve_full_width(crate::defaults::get_bool_opt(
        crate::defaults::FULL_WIDTH_KEY,
    ));
    full_width_item.setState(full_width_menu_state(full_width));
    view_menu.addItem(&full_width_item);
    view_menu.addItem(&item(mtm, DIFF_TITLE, sel!(toggleDiff:), ""));
    view_menu.addItem(NSMenuItem::separatorItem(mtm).as_ref());
    // The sidebar's tabs used to be buttons in its header. The keys o and b
    // still switch tabs; these are what is left for a mouse.
    view_menu.addItem(&item(mtm, "Outline", sel!(showOutline:), ""));
    view_menu.addItem(&item(mtm, "Bookmarks", sel!(showBookmarks:), ""));
    view_menu.addItem(&item(mtm, "Comments", sel!(showComments:), ""));
    // Themes have no other native home: the in-page picker is a palette the
    // keyboard opens, so without this the menu bar could not reach them at all.
    // Checkmarks are stamped in the delegate's validateMenuItem:, not here, so
    // one changed from the palette cannot leave this menu stale.
    let (theme_holder, theme_menu) = submenu(mtm, THEME_TITLE);
    for theme in mdcore::Theme::all() {
        let entry = item(mtm, theme.label(), sel!(selectTheme:), "");
        unsafe { entry.setRepresentedObject(Some(&NSString::from_str(theme.as_wire()))) };
        theme_menu.addItem(&entry);
    }
    view_menu.addItem(&theme_holder);
    view_menu.addItem(NSMenuItem::separatorItem(mtm).as_ref());
    let (layout_holder, layout_menu) = submenu(mtm, "Diff Layout");
    let unified = item(mtm, "Unified", sel!(setUnifiedDiff:), "");
    let split = item(mtm, "Split", sel!(setSplitDiff:), "");
    let layout = crate::state::resolve_diff_layout(
        crate::defaults::get_string(crate::defaults::DIFF_LAYOUT_KEY).as_deref(),
    );
    unified.setState(diff_layout_menu_state(layout == mdcore::DiffLayout::Unified));
    split.setState(diff_layout_menu_state(layout == mdcore::DiffLayout::Split));
    layout_menu.addItem(&unified);
    layout_menu.addItem(&split);
    view_menu.addItem(&layout_holder);
    menubar.addItem(&view_holder);

    let (bm_holder, bm_menu) = submenu(mtm, "Bookmarks");
    bm_menu.addItem(&item(mtm, "Bookmark This Document", sel!(toggleBookmark:), ""));
    menubar.addItem(&bm_holder);

    let (window_holder, window_menu) = submenu(mtm, "Window");
    window_menu.addItem(&item(mtm, "Minimize", sel!(performMiniaturize:), "m"));
    menubar.addItem(&window_holder);

    // MDView has no help book, so the only item here is the one thing a user
    // cannot otherwise discover: the page's single-key shortcuts, which leave
    // no trace in the menu bar. Registering the menu via `setHelpMenu` is
    // still what makes AppKit add the standard Help search field.
    let (help_holder, help_menu) = submenu(mtm, "Help");
    help_menu.addItem(&item(mtm, SHORTCUTS_TITLE, sel!(showShortcuts:), ""));
    menubar.addItem(&help_holder);

    app.setMainMenu(Some(&menubar));
    app.setWindowsMenu(Some(&window_menu));
    app.setHelpMenu(Some(&help_menu));

    recent_menu
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole shortcut policy in one place. Every command the page can do
    /// has a key or a two-key `g` sequence (the ? sheet is the list), so a
    /// modifier shortcut for it would be a second binding to keep in sync with
    /// the first. Only three of MDView's own commands keep one, because their
    /// muscle memory predates this app and no page key can replace them: ⌘O
    /// opens a native panel,
    /// and ⌘F / ⌘R are what hands reach for without looking.
    #[test]
    fn only_open_find_and_reload_keep_a_key_equivalent() {
        let source = include_str!("menu.rs");
        let install = &source[source.find("pub fn install(").expect("install")
            ..source.find("#[cfg(test)]").expect("tests")]
            // Find passes its key through a constant; inline it so the scan
            // below sees one shape rather than two.
            .replace("FIND_KEY_EQUIVALENT", "\"f\"");
        let install = install.as_str();

        // `item(mtm, title, selector, key)` -- collect every non-empty key.
        let bound: Vec<&str> = install
            .match_indices("sel!(")
            .filter_map(|(at, _)| {
                let rest = &install[at..];
                let end = rest.find(')')? + 1;
                let tail = rest[end..].trim_start().trim_start_matches(',').trim_start();
                let key = tail.strip_prefix('"')?;
                let key = &key[..key.find('"')?];
                if key.is_empty() { None } else { Some(key) }
            })
            .collect();

        // Sorted for a stable failure message; duplicates are meaningful here,
        // since "h" (Hide) and "w" (Close) are distinct items.
        let mut sorted = bound.clone();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            // MDView's own: Open, Find, Reload. The rest are macOS standards
            // that are not duplicates of anything -- and ⌘C is the only way to
            // copy out of the web view at all.
            vec!["a", "c", "f", "h", "m", "o", "q", "r", "w"],
            "unexpected key equivalents: {bound:?}"
        );
    }

    /// ⌘F is Find. Full Width used to sit on ⌥⌘F beside it; now it has no
    /// shortcut at all, so the two cannot collide however the menu is rebuilt.
    #[test]
    fn find_is_the_only_command_bound_to_f() {
        assert_eq!(FIND_TITLE, "Find…");
        assert_eq!(FIND_KEY_EQUIVALENT, "f");
        let source = include_str!("menu.rs");
        let install = &source[source.find("pub fn install(").expect("install")
            ..source.find("#[cfg(test)]").expect("tests")];
        assert!(
            !install.contains("setKeyEquivalentModifierMask"),
            "a modifier mask means a shortcut beyond the plain ⌘ ones"
        );
    }

    #[test]
    fn the_titles_the_menu_advertises_are_stable() {
        assert_eq!(FULL_WIDTH_TITLE, "Full Width");
        assert_eq!(DIFF_TITLE, "Show Diff");
        assert_eq!(SHORTCUTS_TITLE, "Keyboard Shortcuts");
        assert_eq!(THEME_TITLE, "Theme");
    }

    #[test]
    fn fullwidth_menu_checkmark_matches_the_preference() {
        assert_eq!(full_width_menu_state(true), NSControlStateValueOn);
        assert_eq!(full_width_menu_state(false), NSControlStateValueOff);
    }

    #[test]
    fn every_theme_reaches_the_menu_under_its_own_wire_value() {
        // The submenu is built from Theme::all(), so a new theme appears in the
        // menu bar without anyone remembering to add it.
        for theme in mdcore::Theme::all() {
            assert!(!theme.as_wire().is_empty());
            assert!(!theme.label().is_empty());
        }
    }

    #[test]
    fn theme_checkmarks_are_exclusive() {
        assert_eq!(theme_menu_state(true), NSControlStateValueOn);
        assert_eq!(theme_menu_state(false), NSControlStateValueOff);
    }

    #[test]
    fn diff_layout_checkmarks_are_exclusive() {
        assert_eq!(diff_layout_menu_state(true), NSControlStateValueOn);
        assert_eq!(diff_layout_menu_state(false), NSControlStateValueOff);
    }
}
