use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use mdcore::Highlighter;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSMenu, NSMenuItem};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSNotification, NSRunLoop, NSRunLoopCommonModes, NSString,
    NSTimer, NSURL,
};

use crate::window::DocumentWindow;

/// Everything the delegate owns. Held in the delegate's ivars.
pub struct AppState {
    pub windows: RefCell<Vec<Rc<DocumentWindow>>>,
    /// Built once: loading syntect's syntax set costs tens of milliseconds and
    /// every window and every live reload shares this one.
    pub highlighter: Highlighter,
    pub startup_paths: RefCell<Vec<PathBuf>>,
    pub recent_menu: RefCell<Option<Retained<NSMenu>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDViewAppDelegate"]
    #[ivars = AppState]
    pub struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            let paths = self.ivars().startup_paths.take();
            // One reusable window can only show one document, so open the
            // FIRST argument for the same reason `application:openURLs:` and
            // `present_open_panel` do, and record the rest in history so
            // they stay reachable from File > Open Recent instead of being
            // silently dropped.
            self.open_first_record_rest(&paths);

            // Refill Open Recent after the menu system is ready.
            self.rebuild_recent_menu();

            unsafe {
                let timer = NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                    0.05,
                    self,
                    objc2::sel!(watchTick:),
                    None,
                    true,
                );
                // `scheduledTimer...` only adds the timer to
                // NSDefaultRunLoopMode, so live reload and banner drains stall
                // during menu tracking, live window resize, and the ⌘O panel's
                // modal loop. Also add it to the common modes so it keeps
                // firing there.
                NSRunLoop::currentRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes);
            }
        }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn terminate_after_last_window(&self, _app: &NSApplication) -> bool {
            true
        }

        #[unsafe(method(application:openURLs:))]
        fn open_urls(&self, _app: &NSApplication, urls: &NSArray<NSURL>) {
            // With one reusable window, opening N files can only show one.
            // Open the FIRST and record the rest in history rather than
            // dropping them — discarding them entirely would throw away
            // files the user explicitly selected.
            let mut paths = Vec::new();
            for url in urls.iter() {
                // Ignore anything that is not a local file; the app has no
                // business fetching remote documents. A path alone is not enough:
                // https://example.com/foo has a path too.
                if !url.isFileURL() {
                    continue;
                }
                let Some(path) = url.path() else { continue };
                paths.push(std::path::PathBuf::from(path.to_string()));
            }
            self.open_first_record_rest(&paths);
        }

        #[unsafe(method(applicationShouldOpenUntitledFile:))]
        fn should_open_untitled(&self, _app: &NSApplication) -> bool {
            // AppKit cannot see documents we opened from argv or from an Apple
            // event, so answer "yes" only when there is genuinely nothing on
            // screen and nothing pending. Otherwise a CLI launch would get an
            // Open panel on top of the document it asked for.
            let state = self.ivars();
            state.windows.borrow().is_empty() && state.startup_paths.borrow().is_empty()
        }

        #[unsafe(method(applicationOpenUntitledFile:))]
        fn open_untitled(&self, _app: &NSApplication) -> bool {
            self.present_open_panel();
            true
        }
    }

    impl AppDelegate {
        #[unsafe(method(watchTick:))]
        fn watch_tick(&self, _timer: Option<&NSObject>) {
            let now = std::time::Instant::now();
            let state = self.ivars();

            // Prune closed windows before polling them. `isVisible` is NOT
            // the right predicate: it is also false for a window that is
            // merely hidden (⌘H, which -[NSApplication hide:] applies to
            // every window in the app) or miniaturized (⌘M), and dropping
            // the sole `Rc<DocumentWindow>` in those cases would tear down a
            // window that is still alive. `is_closed()` is only set by
            // `windowWillClose:`, which fires exclusively when the window is
            // actually closed.
            {
                let mut windows = state.windows.borrow_mut();
                windows.retain(|w| !w.is_closed());
            }

            // Drain any banners that were queued by a recent load and are now
            // ready to be injected (the page has finished loading).
            for window in state.windows.borrow().iter() {
                window.drain_pending_banners();
            }

            // Collect first, then update: live_update can trigger reentrancy
            // into `windows`, and holding the borrow across it would panic.
            let due: Vec<_> = state
                .windows
                .borrow()
                .iter()
                .filter(|window| {
                    window
                        .watcher
                        .borrow_mut()
                        .as_mut()
                        .map(|w| w.poll(now))
                        .unwrap_or(false)
                })
                .cloned()
                .collect();

            for window in due {
                window.live_update(&state.highlighter);
            }
        }

        #[unsafe(method(openDocument:))]
        fn open_document_action(&self, _sender: Option<&NSObject>) {
            self.present_open_panel();
        }

        #[unsafe(method(reloadDocument:))]
        fn reload_document_action(&self, _sender: Option<&NSObject>) {
            if let Some(window) = self.frontmost_window() {
                window.reload(&self.ivars().highlighter);
            }
        }

        #[unsafe(method(zoomIn:))]
        fn zoom_in_action(&self, _sender: Option<&NSObject>) {
            self.adjust_zoom(1.1);
        }

        #[unsafe(method(zoomOut:))]
        fn zoom_out_action(&self, _sender: Option<&NSObject>) {
            self.adjust_zoom(1.0 / 1.1);
        }

        #[unsafe(method(zoomActual:))]
        fn zoom_actual_action(&self, _sender: Option<&NSObject>) {
            if let Some(window) = self.frontmost_window() {
                unsafe { window.webview.setPageZoom(1.0) };
            }
        }

        #[unsafe(method(cycleTheme:))]
        fn cycle_theme_action(&self, _sender: Option<&NSObject>) {
            // Advance through Theme::all() in order, wrapping at the end.
            let current = mdcore::Theme::from_wire(
                &crate::defaults::get_string(crate::defaults::THEME_KEY).unwrap_or_default(),
            );
            let all = mdcore::Theme::all();
            let i = all.iter().position(|t| *t == current).unwrap_or(0);
            let next = all[(i + 1) % all.len()];
            self.handle_message(crate::state::Message::SetTheme(next));
        }

        #[unsafe(method(toggleSidebar:))]
        fn toggle_sidebar_action(&self, _sender: Option<&NSObject>) {
            let current = crate::defaults::get_bool_opt(crate::defaults::SIDEBAR_OPEN_KEY).unwrap_or(true);
            let open = !current;
            let tab = crate::defaults::get_string(crate::defaults::SIDEBAR_TAB_KEY)
                .unwrap_or_else(|| "outline".to_string());
            crate::defaults::set_bool(crate::defaults::SIDEBAR_OPEN_KEY, open);
            let script = format!(
                "window.mdviewSetSidebar && window.mdviewSetSidebar({}, {});",
                open,
                mdcore::escape::js_string_literal(&tab)
            );
            for window in self.ivars().windows.borrow().iter() {
                window.eval_script(&script);
            }
        }

        #[unsafe(method(toggleBookmark:))]
        fn toggle_bookmark_action(&self, _sender: Option<&NSObject>) {
            self.handle_message(crate::state::Message::ToggleBookmark);
        }

        #[unsafe(method(openRecent:))]
        fn open_recent_action(&self, sender: Option<&NSMenuItem>) {
            let Some(sender) = sender else { return };
            let Some(object) = sender.representedObject() else {
                return;
            };
            let Ok(path) = object.downcast::<NSString>() else {
                return;
            };
            // No index, no list, no timing: the item names its own document.
            self.open_document(std::path::Path::new(&path.to_string()));
        }

        #[unsafe(method(clearRecent:))]
        fn clear_recent_action(&self, _sender: Option<&NSObject>) {
            crate::defaults::set_strings(crate::defaults::HISTORY_KEY, &[]);
            self.rebuild_recent_menu();
        }
    }
);

impl AppDelegate {
    pub fn new(
        mtm: MainThreadMarker,
        startup_paths: Vec<PathBuf>,
        recent_menu: Retained<NSMenu>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(AppState {
            windows: RefCell::new(Vec::new()),
            highlighter: Highlighter::new(),
            startup_paths: RefCell::new(startup_paths),
            recent_menu: RefCell::new(Some(recent_menu)),
        });
        unsafe { objc2::msg_send![super(this), init] }
    }

    /// History filtered to entries that still exist on disk. This is a snapshot;
    /// callers must not assume an index into it stays valid across time, or hold
    /// it while the list could change.
    fn live_history(&self) -> Vec<String> {
        crate::defaults::get_strings(crate::defaults::HISTORY_KEY)
            .into_iter()
            .filter(|p| std::path::Path::new(p.as_str()).exists())
            .collect()
    }

    /// Refill File > Open Recent from persisted history. Each item carries the
    /// path as its represented object rather than an index, so clicking the item
    /// opens the document it names even if earlier files have since been deleted.
    pub(crate) fn rebuild_recent_menu(&self) {
        let Some(menu) = self.ivars().recent_menu.borrow().clone() else {
            return;
        };
        let mtm = MainThreadMarker::from(self);
        menu.removeAllItems();

        let live = self.live_history();

        for path in live.iter() {
            let name = std::path::Path::new(path.as_str())
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone());
            let entry = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &NSString::from_str(&name),
                    Some(objc2::sel!(openRecent:)),
                    &NSString::from_str(""),
                )
            };
            // Carry the path on the item rather than an index into a list that
            // is recomputed at click time. An index is only valid for the list
            // that produced it; if a file is deleted before the click, the
            // list shifts and the index silently resolves to a neighbour.
            unsafe { entry.setRepresentedObject(Some(&NSString::from_str(path))) };
            menu.addItem(&entry);
        }

        if !live.is_empty() {
            menu.addItem(NSMenuItem::separatorItem(mtm).as_ref());
        }
        let clear = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str("Clear Menu"),
                Some(objc2::sel!(clearRecent:)),
                &NSString::from_str(""),
            )
        };
        menu.addItem(&clear);
    }

    /// Open the first path and record the rest in history without displaying
    /// them. One window can only show one document, but the user asked for
    /// all of them, so the others stay reachable from File > Open Recent.
    fn open_first_record_rest(&self, paths: &[std::path::PathBuf]) {
        let Some((first, rest)) = paths.split_first() else { return };
        for path in rest.iter().rev() {
            // Canonicalize here too. `open_document` normalises the path it
            // opens, so recording the extras as-given would key the same
            // document two different ways — a relative argv path against the
            // process CWD, or an unresolved symlink — which is the identity
            // split canonicalization exists to prevent.
            let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
            if let Some(s) = path.to_str() {
                let history = crate::state::push_history(
                    &crate::defaults::get_strings(crate::defaults::HISTORY_KEY),
                    s,
                    50,
                );
                crate::defaults::set_strings(crate::defaults::HISTORY_KEY, &history);
            }
        }
        self.open_document(first);
    }

    /// The single entry point every way of opening a file funnels into:
    /// startup arguments, Finder, the Open panel, and dropped files.
    pub fn open_document(&self, path: &std::path::Path) {
        use crate::navigation::NavigationRequest;
        use objc2_app_kit::NSWorkspace;

        // Persisted history and bookmarks are keyed by path string, so a
        // relative path would produce a second identity for the same document
        // and would resolve against whatever CWD the process happens to have.
        let path = &std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

        // Every way of opening a document funnels through here, which makes
        // this the one correct place to record history.
        //
        // `path.to_str()` silently omits a non-UTF-8 path from history (and
        // from Open Recent) while still opening it in the window below — a
        // deliberate, if narrow, gap rather than an oversight.
        if let Some(path_str) = path.to_str() {
            let history = crate::state::push_history(
                &crate::defaults::get_strings(crate::defaults::HISTORY_KEY),
                path_str,
                50,
            );
            crate::defaults::set_strings(crate::defaults::HISTORY_KEY, &history);
        }

        let state = self.ivars();

        // Reuse the window the user is looking at rather than stacking a new
        // one per document. `frontmost_window` is the same notion of "current
        // window" that Reload and the zoom items use, so they cannot disagree.
        if let Some(existing) = self.frontmost_window() {
            existing.load(path, &state.highlighter);
            existing.window.makeKeyAndOrderFront(None);
            // The reuse branch returns early, so the new-window branch's call
            // is unreachable here — and this is the path nearly every open
            // takes. Without it the star and list describe the PREVIOUS
            // document, and a ⌘D against that stale state would toggle the
            // wrong way.
            self.push_bookmarks_to_pages();
            self.rebuild_recent_menu();
            return;
        }

        let mtm = MainThreadMarker::from(self);

        // The web view calls back on the main thread, so a plain Rc closure
        // holding a pointer to the delegate is sound here.
        let delegate: Retained<AppDelegate> = unsafe { Retained::retain(self as *const _ as *mut _) }
            .expect("delegate is alive while its windows are");

        let handler: Rc<dyn Fn(NavigationRequest)> = Rc::new(move |request| match request {
            NavigationRequest::OpenExternal(url) => {
                let workspace = NSWorkspace::sharedWorkspace();
                if let Some(url) = NSURL::URLWithString(&NSString::from_str(&url)) {
                    workspace.openURL(&url);
                }
            }
            NavigationRequest::OpenDocument(path) => delegate.open_document(&path),
        });

        let msg_delegate: Retained<AppDelegate> =
            unsafe { Retained::retain(self as *const _ as *mut _) }
                .expect("delegate is alive while its windows are");
        let on_message: Rc<dyn Fn(crate::state::Message)> = Rc::new(move |message| {
            msg_delegate.handle_message(message);
        });

        let window = DocumentWindow::open(path, mtm, &state.highlighter, handler, on_message);
        state.windows.borrow_mut().push(window);
        self.push_bookmarks_to_pages();
        self.rebuild_recent_menu();
    }

    /// The window the user is looking at, or None when every window is closed.
    fn frontmost_window(&self) -> Option<Rc<DocumentWindow>> {
        let windows = self.ivars().windows.borrow();
        windows
            .iter()
            .find(|w| w.window.isKeyWindow())
            .or_else(|| windows.last())
            .cloned()
    }

    fn adjust_zoom(&self, factor: f64) {
        if let Some(window) = self.frontmost_window() {
            let current = unsafe { window.webview.pageZoom() };
            // Clamp so repeated presses cannot make the document unreadable.
            let next = (current * factor).clamp(0.5, 3.0);
            unsafe { window.webview.setPageZoom(next) };
        }
    }

    pub(crate) fn handle_message(&self, message: crate::state::Message) {
        use crate::state::Message;
        match message {
            Message::SetTheme(theme) => {
                crate::defaults::set_string(crate::defaults::THEME_KEY, theme.as_wire());
                // A runtime theme change cannot swap the pinned sheet's contents —
                // that CSS is baked in by Rust for the theme the page was built with.
                // Reload the page so the new theme's pinned sheet is emitted.
                let Some(window) = self.frontmost_window() else { return };
                window.reload(&self.ivars().highlighter);
            }
            Message::ToggleBookmark => {
                let Some(window) = self.frontmost_window() else { return };
                let path = window.path.borrow().to_string_lossy().into_owned();
                let updated = crate::state::toggle_bookmark(
                    &crate::defaults::get_strings(crate::defaults::BOOKMARKS_KEY),
                    &path,
                );
                crate::defaults::set_strings(crate::defaults::BOOKMARKS_KEY, &updated);
                self.push_bookmarks_to_pages();
            }
            Message::OpenPath(path) => {
                self.open_document(std::path::Path::new(&path));
            }
            Message::SetSidebar { open, tab } => {
                crate::defaults::set_bool(crate::defaults::SIDEBAR_OPEN_KEY, open);
                crate::defaults::set_string(crate::defaults::SIDEBAR_TAB_KEY, &tab);
            }
        }
    }

    /// Send the bookmark list, and whether the current document is among
    /// them, to every open page. Entries whose file has gone are filtered out
    /// of the DISPLAY only — they stay in storage, so an unmounted volume
    /// does not silently erase the list.
    pub(crate) fn push_bookmarks_to_pages(&self) {
        let stored = crate::defaults::get_strings(crate::defaults::BOOKMARKS_KEY);
        let live: Vec<&String> = stored
            .iter()
            .filter(|p| std::path::Path::new(p.as_str()).exists())
            .collect();
        let items = live
            .iter()
            .map(|p| {
                let name = std::path::Path::new(p.as_str())
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| (*p).clone());
                format!(
                    "{{name:{},path:{}}}",
                    mdcore::escape::js_string_literal(&name),
                    mdcore::escape::js_string_literal(p)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        for window in self.ivars().windows.borrow().iter() {
            let current = window.path.borrow().to_string_lossy().into_owned();
            let starred = crate::state::is_bookmarked(&stored, &current);
            let script = format!(
                "window.mdviewSetBookmarks && window.mdviewSetBookmarks([{items}], {starred});"
            );
            // Always queue. `drain_pending_banners` performs the one
            // authoritative isLoading() check at drain time; re-checking it
            // here would be reading it in the very window where it is
            // unreliable — between loadHTMLString returning and WebKit
            // starting the navigation.
            window.pending_scripts.borrow_mut().push(script);
        }
    }

    pub(crate) fn present_open_panel(&self) {
        use objc2_app_kit::{NSModalResponse, NSOpenPanel};

        let mtm = MainThreadMarker::from(self);
        let panel = NSOpenPanel::openPanel(mtm);
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(false);
        panel.setAllowsMultipleSelection(true);

        let response: NSModalResponse = panel.runModal();
        // NSModalResponseOK is 1.
        if response != 1 {
            return;
        }

        let urls = panel.URLs();
        let mut paths = Vec::new();
        for url in urls.iter() {
            if let Some(path) = url.path() {
                paths.push(std::path::PathBuf::from(path.to_string()));
            }
        }
        self.open_first_record_rest(&paths);
    }
}

pub fn run(paths: Vec<PathBuf>) -> ! {
    let mtm = MainThreadMarker::new().expect("main() runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

    let recent_menu = crate::menu::install(&app, mtm);

    let delegate = AppDelegate::new(mtm, paths, recent_menu);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    app.run();
    unreachable!("NSApplication::run does not return");
}
