use objc2::rc::Retained;
use objc2::sel;
use objc2::MainThreadOnly;
use objc2_app_kit::{
    NSApplication, NSControlStateValue, NSControlStateValueOff, NSControlStateValueOn,
    NSEventModifierFlags, NSMenu, NSMenuItem,
};
use objc2_foundation::{MainThreadMarker, NSString};

const FULL_WIDTH_TITLE: &str = "Full Width";
const FULL_WIDTH_KEY_EQUIVALENT: &str = "f";
const DIFF_TITLE: &str = "Show Diff";
const DIFF_KEY_EQUIVALENT: &str = "d";
const FIND_TITLE: &str = "Find…";
const FIND_KEY_EQUIVALENT: &str = "f";
const FIND_NEXT_KEY_EQUIVALENT: &str = "g";
const SHORTCUTS_TITLE: &str = "Keyboard Shortcuts";
const SHORTCUTS_KEY_EQUIVALENT: &str = "/";
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

fn full_width_modifier_mask() -> NSEventModifierFlags {
    NSEventModifierFlags::Command | NSEventModifierFlags::Option
}

/// ⇧⌘G. The shift is set on the mask rather than by giving the item an
/// uppercase "G", so the item and Find Next differ only in their modifiers.
fn find_previous_modifier_mask() -> NSEventModifierFlags {
    NSEventModifierFlags::Command | NSEventModifierFlags::Shift
}

/// ⇧⌘/ — which is to say ⌘?, the key the sheet itself is bound to in the page.
/// Same reasoning as Find Previous: the shift lives on the mask, so the key
/// equivalent stays the unshifted "/".
fn shortcuts_modifier_mask() -> NSEventModifierFlags {
    NSEventModifierFlags::Command | NSEventModifierFlags::Shift
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
    find_menu.addItem(&item(
        mtm,
        "Find Next",
        sel!(findNextMatch:),
        FIND_NEXT_KEY_EQUIVALENT,
    ));
    let find_previous = item(
        mtm,
        "Find Previous",
        sel!(findPreviousMatch:),
        FIND_NEXT_KEY_EQUIVALENT,
    );
    find_previous.setKeyEquivalentModifierMask(find_previous_modifier_mask());
    find_menu.addItem(&find_previous);
    edit_menu.addItem(&find_holder);
    menubar.addItem(&edit_holder);

    let (view_holder, view_menu) = submenu(mtm, "View");
    view_menu.addItem(&item(mtm, "Actual Size", sel!(zoomActual:), "0"));
    // "+" as a key equivalent requires ⇧⌘= to fire (AppKit reads it as the
    // shifted character); plain ⌘=, what users actually press for zoom in,
    // needs the unshifted "=" here instead.
    view_menu.addItem(&item(mtm, "Zoom In", sel!(zoomIn:), "="));
    view_menu.addItem(&item(mtm, "Zoom Out", sel!(zoomOut:), "-"));
    view_menu.addItem(NSMenuItem::separatorItem(mtm).as_ref());
    let sidebar_item = item(mtm, "Toggle Sidebar", sel!(toggleSidebar:), "s");
    sidebar_item.setKeyEquivalentModifierMask(
        NSEventModifierFlags::Command | NSEventModifierFlags::Option,
    );
    view_menu.addItem(&sidebar_item);
    let full_width_item = item(
        mtm,
        FULL_WIDTH_TITLE,
        sel!(toggleFullWidth:),
        FULL_WIDTH_KEY_EQUIVALENT,
    );
    full_width_item.setKeyEquivalentModifierMask(full_width_modifier_mask());
    let full_width = crate::state::resolve_full_width(crate::defaults::get_bool_opt(
        crate::defaults::FULL_WIDTH_KEY,
    ));
    full_width_item.setState(full_width_menu_state(full_width));
    view_menu.addItem(&full_width_item);
    let diff_item = item(mtm, DIFF_TITLE, sel!(toggleDiff:), DIFF_KEY_EQUIVALENT);
    diff_item.setKeyEquivalentModifierMask(
        NSEventModifierFlags::Command | NSEventModifierFlags::Option,
    );
    view_menu.addItem(&diff_item);
    view_menu.addItem(NSMenuItem::separatorItem(mtm).as_ref());
    // The sidebar's tabs used to be buttons in its header. The keys o and b
    // still switch tabs; these are what is left for a mouse.
    view_menu.addItem(&item(mtm, "Outline", sel!(showOutline:), ""));
    view_menu.addItem(&item(mtm, "Bookmarks", sel!(showBookmarks:), ""));
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
    bm_menu.addItem(&item(mtm, "Bookmark This Document", sel!(toggleBookmark:), "d"));
    menubar.addItem(&bm_holder);

    let (window_holder, window_menu) = submenu(mtm, "Window");
    window_menu.addItem(&item(mtm, "Minimize", sel!(performMiniaturize:), "m"));
    menubar.addItem(&window_holder);

    // MDView has no help book, so the only item here is the one thing a user
    // cannot otherwise discover: the page's single-key shortcuts, which leave
    // no trace in the menu bar. Registering the menu via `setHelpMenu` is
    // still what makes AppKit add the standard Help search field.
    let (help_holder, help_menu) = submenu(mtm, "Help");
    let shortcuts_item = item(
        mtm,
        SHORTCUTS_TITLE,
        sel!(showShortcuts:),
        SHORTCUTS_KEY_EQUIVALENT,
    );
    shortcuts_item.setKeyEquivalentModifierMask(shortcuts_modifier_mask());
    help_menu.addItem(&shortcuts_item);
    menubar.addItem(&help_holder);

    app.setMainMenu(Some(&menubar));
    app.setWindowsMenu(Some(&window_menu));
    app.setHelpMenu(Some(&help_menu));

    recent_menu
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fullwidth_menu_contract_uses_option_command_f() {
        assert_eq!(FULL_WIDTH_TITLE, "Full Width");
        assert_eq!(FULL_WIDTH_KEY_EQUIVALENT, "f");
        assert_eq!(
            full_width_modifier_mask(),
            NSEventModifierFlags::Command | NSEventModifierFlags::Option
        );
    }

    #[test]
    fn fullwidth_menu_checkmark_matches_the_preference() {
        assert_eq!(full_width_menu_state(true), NSControlStateValueOn);
        assert_eq!(full_width_menu_state(false), NSControlStateValueOff);
    }

    #[test]
    fn diff_menu_contract_uses_option_command_d() {
        assert_eq!(DIFF_TITLE, "Show Diff");
        assert_eq!(DIFF_KEY_EQUIVALENT, "d");
    }

    #[test]
    fn find_menu_contract_uses_the_standard_find_shortcuts() {
        assert_eq!(FIND_TITLE, "Find…");
        assert_eq!(FIND_KEY_EQUIVALENT, "f");
        assert_eq!(FIND_NEXT_KEY_EQUIVALENT, "g");
        assert_eq!(
            find_previous_modifier_mask(),
            NSEventModifierFlags::Command | NSEventModifierFlags::Shift
        );
    }

    /// ⌘F is Find; Full Width has to stay on ⌥⌘F or the two collide and only
    /// one of them ever fires.
    #[test]
    fn find_and_full_width_do_not_share_a_shortcut() {
        assert_eq!(FIND_KEY_EQUIVALENT, FULL_WIDTH_KEY_EQUIVALENT);
        assert_ne!(full_width_modifier_mask(), NSEventModifierFlags::Command);
        assert_ne!(full_width_modifier_mask(), find_previous_modifier_mask());
    }

    #[test]
    fn every_theme_reaches_the_menu_under_its_own_wire_value() {
        // The submenu is built from Theme::all(), so a new theme appears in the
        // menu bar without anyone remembering to add it.
        assert_eq!(THEME_TITLE, "Theme");
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
    fn shortcuts_menu_contract_uses_command_question_mark() {
        assert_eq!(SHORTCUTS_TITLE, "Keyboard Shortcuts");
        // "?" is the shifted "/", so the item carries "/" plus a shift on the
        // mask; an item keyed on "?" itself would need ⇧⌘? to fire.
        assert_eq!(SHORTCUTS_KEY_EQUIVALENT, "/");
        assert_eq!(
            shortcuts_modifier_mask(),
            NSEventModifierFlags::Command | NSEventModifierFlags::Shift
        );
    }

    #[test]
    fn diff_layout_checkmarks_are_exclusive() {
        assert_eq!(diff_layout_menu_state(true), NSControlStateValueOn);
        assert_eq!(diff_layout_menu_state(false), NSControlStateValueOff);
    }
}
