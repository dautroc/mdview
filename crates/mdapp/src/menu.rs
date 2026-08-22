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

    // Empty is enough: registering it as the help menu via `setHelpMenu` is
    // what makes AppKit populate the standard Help-menu search field, and
    // MDView has no help book of its own to add items for.
    let (help_holder, help_menu) = submenu(mtm, "Help");
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
    fn diff_layout_checkmarks_are_exclusive() {
        assert_eq!(diff_layout_menu_state(true), NSControlStateValueOn);
        assert_eq!(diff_layout_menu_state(false), NSControlStateValueOff);
    }
}
