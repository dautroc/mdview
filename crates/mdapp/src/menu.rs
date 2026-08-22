use objc2::rc::Retained;
use objc2::sel;
use objc2::MainThreadOnly;
use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
use objc2_foundation::{MainThreadMarker, NSString};

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
pub fn install(app: &NSApplication, mtm: MainThreadMarker) {
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
    view_menu.addItem(&item(mtm, "Toggle Sidebar", sel!(toggleSidebar:), "s"));
    view_menu.addItem(&item(mtm, "Toggle Theme", sel!(cycleTheme:), "t"));
    menubar.addItem(&view_holder);

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
}
