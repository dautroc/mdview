use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

use mdcore::Highlighter;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSMenu, NSMenuItem,
    NSMenuItemValidation, NSWindowOrderingMode,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSNotification, NSRunLoop, NSRunLoopCommonModes, NSString, NSTimer,
    NSURL,
};

use crate::window::DocumentWindow;

pub(crate) struct HistoryResponse {
    tab_id: u64,
    generation: u64,
    result: Result<Vec<mdcore::HistoryEntry>, String>,
}

pub(crate) enum WorkspaceResponse {
    Scan {
        source_id: Option<u64>,
        generation: u64,
        open_first: bool,
        result: Result<mdcore::WorkspaceSnapshot, String>,
    },
    Indexed {
        generation: u64,
        result: Result<mdcore::WorkspaceIndex, String>,
    },
    Incremental {
        generation: u64,
        result: Result<mdcore::WorkspaceIndex, String>,
    },
    Search {
        tab_id: u64,
        workspace_generation: u64,
        request_generation: u64,
        query: String,
        result: Vec<mdcore::SearchHit>,
    },
}

pub(crate) struct WorkspaceData {
    snapshot: mdcore::WorkspaceSnapshot,
    index: Option<Arc<mdcore::WorkspaceIndex>>,
}

pub(crate) struct WorkspaceRestore {
    paths: Vec<PathBuf>,
    selected: Option<PathBuf>,
    positions: HashMap<PathBuf, u32>,
}

/// Everything the delegate owns. Held in the delegate's ivars.
pub struct AppState {
    pub windows: RefCell<Vec<Rc<DocumentWindow>>>,
    /// Stable identity for the native tab AppKit most recently made key. It
    /// remains meaningful while an Open panel temporarily owns key focus.
    pub active_tab: Rc<Cell<Option<u64>>>,
    pub next_tab_id: Cell<u64>,
    /// Built once: loading syntect's syntax set costs tens of milliseconds and
    /// every window and every live reload shares this one.
    pub highlighter: Highlighter,
    pub startup_paths: RefCell<Vec<PathBuf>>,
    pub recent_menu: RefCell<Option<Retained<NSMenu>>>,
    pub history_sender: Sender<HistoryResponse>,
    pub history_receiver: RefCell<Receiver<HistoryResponse>>,
    pub next_history_generation: Cell<u64>,
    pub history_requests: RefCell<HashMap<u64, u64>>,
    pub history_cache: RefCell<HashMap<u64, Vec<mdcore::HistoryEntry>>>,
    pub workspace_sender: Sender<WorkspaceResponse>,
    pub workspace_receiver: RefCell<Receiver<WorkspaceResponse>>,
    pub next_workspace_generation: Cell<u64>,
    pub workspace_generation: Cell<u64>,
    pub workspace: RefCell<Option<WorkspaceData>>,
    pub next_workspace_search_generation: Cell<u64>,
    pub workspace_search_requests: RefCell<HashMap<u64, u64>>,
    pub workspace_search_cache: RefCell<HashMap<u64, (String, Vec<mdcore::SearchHit>)>>,
    pub workspace_watcher: RefCell<Option<crate::watcher::WorkspaceWatcher>>,
    pub workspace_watcher_root: RefCell<Option<PathBuf>>,
    pub workspace_watch_retry_at: Cell<Option<std::time::Instant>>,
    pub pending_workspace_changes: RefCell<Vec<PathBuf>>,
    pub pending_workspace_restore: RefCell<Option<WorkspaceRestore>>,
    pub workspace_positions: RefCell<HashMap<PathBuf, u32>>,
    pub last_workspace_session: RefCell<Vec<String>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDViewAppDelegate"]
    #[ivars = AppState]
    pub struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSMenuItemValidation for AppDelegate {
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            // The theme is a global preference, so it is stamped and enabled
            // ABOVE the window check: an app with every window closed must
            // still be able to change its theme. Stamping here rather than at
            // click time is also what keeps the checkmark honest when the
            // change came from the page's own palette instead of this menu.
            if item.action() == Some(objc2::sel!(openFolder:)) {
                return true.into();
            }
            if item.action() == Some(objc2::sel!(selectTheme:)) {
                let active = crate::state::resolve_theme(
                    crate::defaults::get_string(crate::defaults::THEME_KEY).as_deref(),
                );
                let selected = crate::menu::item_theme_wire(item).as_deref()
                    == Some(active.as_wire());
                item.setState(crate::menu::theme_menu_state(selected));
                return true.into();
            }
            let Some(window) = self.frontmost_window() else {
                return false.into();
            };
            if item.action() == Some(objc2::sel!(showWorkspaceFiles:))
                || item.action() == Some(objc2::sel!(showWorkspaceSearch:))
            {
                return self.ivars().workspace.borrow().is_some().into();
            }
            let valid = match item.action() {
                Some(action)
                    if action == objc2::sel!(selectNextDocumentTab:)
                        || action == objc2::sel!(selectPreviousDocumentTab:) =>
                {
                    window
                        .window
                        .tabGroup()
                        .is_some_and(|group| group.windows().len() > 1)
                }
                Some(action) if action == objc2::sel!(toggleDiff:) => {
                    window.can_show_diff() || window.view_mode() == crate::window::ViewMode::Diff
                },
                Some(action) if action == objc2::sel!(showDocumentHistory:) => {
                    window.can_show_diff()
                },
                Some(action)
                    if action == objc2::sel!(setUnifiedDiff:)
                        || action == objc2::sel!(setSplitDiff:)
                        || action == objc2::sel!(setRenderedDiff:)
                        || action == objc2::sel!(setRenderedSplitDiff:) =>
                {
                    window.view_mode() == crate::window::ViewMode::Diff
                }
                _ => true,
            };
            valid.into()
        }
    }

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            let paths = self.ivars().startup_paths.take();
            // Explicit files win over a restored workspace. A launch with no
            // explicit request restores the last versioned local session.
            if paths.is_empty() {
                self.restore_workspace_session();
            } else {
                self.open_documents(&paths);
            }

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
            // Preserve the event's order: every local file becomes a tab and
            // the first requested document is selected when the batch is done.
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
            self.open_documents(&paths);
        }

        #[unsafe(method(applicationShouldOpenUntitledFile:))]
        fn should_open_untitled(&self, _app: &NSApplication) -> bool {
            // AppKit cannot see documents we opened from argv or from an Apple
            // event, so answer "yes" only when there is genuinely nothing on
            // screen and nothing pending. Otherwise a CLI launch would get an
            // Open panel on top of the document it asked for.
            let state = self.ivars();
            state.windows.borrow().is_empty()
                && state.startup_paths.borrow().is_empty()
                && state.workspace_generation.get() == 0
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
            self.drain_history_results();
            self.drain_workspace_results();
            let workspace_changes = state
                .workspace_watcher
                .borrow_mut()
                .as_mut()
                .map(|watcher| watcher.poll(now))
                .unwrap_or_default();
            if state.workspace_watcher.borrow().is_none()
                && state
                    .workspace_watch_retry_at
                    .get()
                    .map_or(true, |retry_at| now >= retry_at)
            {
                let root = state
                    .workspace
                    .borrow()
                    .as_ref()
                    .map(|workspace| workspace.snapshot.root.path().to_path_buf());
                if let Some(root) = root {
                    let watcher = crate::watcher::WorkspaceWatcher::start(&root).ok();
                    if watcher.is_some() {
                        *state.workspace_watcher_root.borrow_mut() = Some(root);
                        state.workspace_watch_retry_at.set(None);
                    } else {
                        *state.workspace_watcher_root.borrow_mut() = None;
                        state.workspace_watch_retry_at.set(Some(
                            now + std::time::Duration::from_secs(2),
                        ));
                    }
                    *state.workspace_watcher.borrow_mut() = watcher;
                }
            }
            if !workspace_changes.is_empty() {
                let index_ready = state
                    .workspace
                    .borrow()
                    .as_ref()
                    .is_some_and(|workspace| workspace.index.is_some());
                if index_ready {
                    self.reconcile_workspace(workspace_changes);
                } else {
                    state
                        .pending_workspace_changes
                        .borrow_mut()
                        .extend(workspace_changes);
                }
            }
            self.persist_workspace_session();

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

            // A review file that changed under us. `C` asks Claude to delete
            // the records it has addressed, and this is the only thing that
            // notices: without it the comment stays in the margin, struck
            // through or not, until the document is reloaded for some
            // unrelated reason. Collected before pushing, for the same
            // reentrancy reason as above -- `push_comments_to_pages` borrows
            // `windows` too. MDView's own writes come back through here as
            // well; the push is idempotent, so that costs a rebuilt rail and
            // nothing else.
            let reviews_changed = state.windows.borrow().iter().any(|window| {
                window
                    .review_watcher
                    .borrow_mut()
                    .as_mut()
                    .map(|w| w.poll(now))
                    .unwrap_or(false)
            });
            if reviews_changed {
                self.push_comments_to_pages();
            }
        }

        #[unsafe(method(openDocument:))]
        fn open_document_action(&self, _sender: Option<&NSObject>) {
            self.present_open_panel();
        }

        #[unsafe(method(openFolder:))]
        fn open_folder_action(&self, _sender: Option<&NSObject>) {
            self.present_open_folder_panel();
        }

        #[unsafe(method(selectNextDocumentTab:))]
        fn select_next_document_tab_action(&self, _sender: Option<&NSObject>) {
            self.select_document_tab(None, true);
        }

        #[unsafe(method(selectPreviousDocumentTab:))]
        fn select_previous_document_tab_action(&self, _sender: Option<&NSObject>) {
            self.select_document_tab(None, false);
        }

        #[unsafe(method(reloadDocument:))]
        fn reload_document_action(&self, _sender: Option<&NSObject>) {
            // The reload itself carries the same hazard as the theme-change
            // reload -- the fresh page's star and bookmark list start empty,
            // so a bookmarked document would read as unbookmarked until they
            // are pushed again. `handle_message` does both.
            self.handle_message(crate::state::Message::ReloadDocument);
        }

        #[unsafe(method(zoomIn:))]
        fn zoom_in_action(&self, _sender: Option<&NSObject>) {
            self.handle_message(crate::state::Message::ZoomIn);
        }

        #[unsafe(method(zoomOut:))]
        fn zoom_out_action(&self, _sender: Option<&NSObject>) {
            self.handle_message(crate::state::Message::ZoomOut);
        }

        #[unsafe(method(zoomActual:))]
        fn zoom_actual_action(&self, _sender: Option<&NSObject>) {
            self.handle_message(crate::state::Message::ZoomReset);
        }

        #[unsafe(method(showShortcuts:))]
        fn show_shortcuts_action(&self, _sender: Option<&NSObject>) {
            self.run_page_script(crate::state::shortcuts_script());
        }

        /// Deliberately not `setTheme:`: Objective-C would read that as the KVC
        /// setter for a `theme` property and hand it an NSMenuItem. `openRecent:`
        /// sets the verb-style precedent, and carries its data the same way.
        #[unsafe(method(selectTheme:))]
        fn select_theme_action(&self, sender: Option<&NSMenuItem>) {
            let Some(sender) = sender else { return };
            let Some(object) = sender.representedObject() else { return };
            let Ok(wire) = object.downcast::<NSString>() else { return };
            // Theme::from_wire maps anything unrecognised to System, so this
            // cannot fail. The message is the same one the page's palette
            // sends, minus a scroll offset Rust has no way to read.
            self.handle_message(crate::state::Message::SetTheme(
                mdcore::Theme::from_wire(&wire.to_string()),
                None,
            ));
        }

        #[unsafe(method(showOutline:))]
        fn show_outline_action(&self, _sender: Option<&NSObject>) {
            self.show_sidebar_tab("outline");
        }

        #[unsafe(method(showBookmarks:))]
        fn show_bookmarks_action(&self, _sender: Option<&NSObject>) {
            self.show_sidebar_tab("bookmarks");
        }

        #[unsafe(method(showComments:))]
        fn show_comments_action(&self, _sender: Option<&NSObject>) {
            self.show_sidebar_tab("comments");
        }

        #[unsafe(method(showWorkspaceFiles:))]
        fn show_workspace_files_action(&self, _sender: Option<&NSObject>) {
            self.show_sidebar_tab("files");
        }

        #[unsafe(method(showWorkspaceSearch:))]
        fn show_workspace_search_action(&self, _sender: Option<&NSObject>) {
            self.run_page_script(crate::state::open_workspace_search_script());
        }

        #[unsafe(method(copyReviewPrompt:))]
        fn copy_review_prompt_action(&self, _sender: Option<&NSObject>) {
            // Through handle_message, so the menu and the page's C cannot
            // drift apart.
            self.handle_message(crate::state::Message::CopyReview);
        }

        #[unsafe(method(findInPage:))]
        fn find_in_page_action(&self, _sender: Option<&NSObject>) {
            self.run_page_script(crate::state::open_find_script());
        }

        #[unsafe(method(findNextMatch:))]
        fn find_next_action(&self, _sender: Option<&NSObject>) {
            self.run_page_script(crate::state::find_step_script(true));
        }

        #[unsafe(method(findPreviousMatch:))]
        fn find_previous_action(&self, _sender: Option<&NSObject>) {
            self.run_page_script(crate::state::find_step_script(false));
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

        #[unsafe(method(toggleFullWidth:))]
        fn toggle_full_width_action(&self, sender: Option<&NSMenuItem>) {
            let enabled = self.toggle_full_width_native();
            if let Some(item) = sender {
                item.setState(crate::menu::full_width_menu_state(enabled));
            }
        }

        #[unsafe(method(showDocumentHistory:))]
        fn show_document_history_action(&self, _sender: Option<&NSObject>) {
            self.run_page_script(crate::state::open_history_script());
        }

        #[unsafe(method(toggleDiff:))]
        fn toggle_diff_action(&self, sender: Option<&NSMenuItem>) {
            let Some(window) = self.frontmost_window() else { return };
            if !window.can_show_diff() && window.view_mode() != crate::window::ViewMode::Diff {
                return;
            }
            window.toggle_diff(&self.ivars().highlighter);
            if let Some(item) = sender {
                item.setState(crate::menu::diff_menu_state(
                    window.view_mode() == crate::window::ViewMode::Diff,
                ));
            }
        }

        #[unsafe(method(setUnifiedDiff:))]
        fn set_unified_diff_action(&self, sender: Option<&NSMenuItem>) {
            self.set_diff_layout(mdcore::DiffLayout::Unified, sender);
        }

        #[unsafe(method(setSplitDiff:))]
        fn set_split_diff_action(&self, sender: Option<&NSMenuItem>) {
            self.set_diff_layout(mdcore::DiffLayout::Split, sender);
        }

        #[unsafe(method(setRenderedDiff:))]
        fn set_rendered_diff_action(&self, sender: Option<&NSMenuItem>) {
            self.set_diff_layout(mdcore::DiffLayout::Rendered, sender);
        }

        #[unsafe(method(setRenderedSplitDiff:))]
        fn set_rendered_split_diff_action(&self, sender: Option<&NSMenuItem>) {
            self.set_diff_layout(mdcore::DiffLayout::RenderedSplit, sender);
        }

        #[unsafe(method(toggleBookmark:))]
        fn toggle_bookmark_action(&self, _sender: Option<&NSObject>) {
            self.handle_message(crate::state::Message::ToggleBookmark);
        }

        /// The page owns the strip, so this flips the preference and tells
        /// every window; the page then posts the same value back, which is
        /// what `g m` does too. Off unless asked for: a new surface should not
        /// appear in a window nobody opened it in.
        #[unsafe(method(toggleMinimap:))]
        fn toggle_minimap_action(&self, _sender: Option<&NSObject>) {
            let open = !crate::defaults::get_bool_opt(crate::defaults::MINIMAP_OPEN_KEY)
                .unwrap_or(false);
            crate::defaults::set_bool(crate::defaults::MINIMAP_OPEN_KEY, open);
            let script = crate::state::minimap_script(open);
            for window in self.ivars().windows.borrow().iter() {
                window.eval_script(&script);
            }
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
            self.push_recents_to_pages();
        }
    }
);

/// One press of Zoom In multiplies the page zoom by this; Zoom Out divides.
const ZOOM_STEP: f64 = 1.1;

impl AppDelegate {
    fn toggle_full_width_native(&self) -> bool {
        let enabled = crate::state::next_full_width(crate::defaults::get_bool_opt(
            crate::defaults::FULL_WIDTH_KEY,
        ));
        crate::defaults::set_bool(crate::defaults::FULL_WIDTH_KEY, enabled);
        for window in self.ivars().windows.borrow().iter() {
            window.set_full_width(enabled);
        }
        enabled
    }

    /// Nudge a first-time user once, ever: with no buttons on the page there is
    /// nothing else to notice, so this is all that stands between them and an
    /// apparently inert window.
    ///
    /// The flag is burned here rather than when the page confirms it displayed
    /// the hint. A window closed before the queue drains loses the hint, which
    /// is a better failure than one that keeps coming back.
    fn maybe_queue_shortcuts_hint(&self, window: &DocumentWindow) {
        if !crate::state::should_show_shortcuts_hint(crate::defaults::get_bool_opt(
            crate::defaults::SHORTCUTS_HINT_SHOWN_KEY,
        )) {
            return;
        }
        crate::defaults::set_bool(crate::defaults::SHORTCUTS_HINT_SHOWN_KEY, true);
        // Queued, never evaluated directly: loadHTMLString is asynchronous, so
        // a script run now would hit the previous document or none at all.
        // drain_pending_banners owns the one readiness check.
        window
            .pending_scripts
            .borrow_mut()
            .push(crate::state::shortcuts_hint_script().to_string());
    }

    /// Open the sidebar on one tab. The menu items SHOW a tab rather than
    /// toggling it, unlike the page's own o and b keys: picking "Outline" from
    /// a menu and having the panel shut is not what anyone means by it.
    fn show_sidebar_tab(&self, tab: &str) {
        crate::defaults::set_bool(crate::defaults::SIDEBAR_OPEN_KEY, true);
        crate::defaults::set_string(crate::defaults::SIDEBAR_TAB_KEY, tab);
        let script = format!(
            "window.mdviewShowSidebarTab && window.mdviewShowSidebarTab({});",
            mdcore::escape::js_string_literal(tab)
        );
        for window in self.ivars().windows.borrow().iter() {
            window.eval_script(&script);
        }
    }

    /// Run a script against the frontmost page's own UI -- the find bar, the
    /// keyboard cheat sheet. The web view is made first responder first:
    /// both of those live inside the page, so a window whose keyboard focus
    /// sits anywhere else would show them and then send the user's typing (or
    /// their esc) somewhere they cannot see.
    fn run_page_script(&self, script: &str) {
        let Some(window) = self.frontmost_window() else {
            return;
        };
        window.window.makeFirstResponder(Some(&window.webview));
        window.eval_script(script);
    }

    fn set_diff_layout(&self, layout: mdcore::DiffLayout, sender: Option<&NSMenuItem>) {
        let Some(window) = self.frontmost_window() else {
            return;
        };
        window.set_diff_layout(layout, &self.ivars().highlighter);
        if let Some(sender) = sender {
            crate::menu::set_diff_layout_states(sender, layout);
        }
    }
    pub fn new(
        mtm: MainThreadMarker,
        startup_paths: Vec<PathBuf>,
        recent_menu: Retained<NSMenu>,
    ) -> Retained<Self> {
        let (history_sender, history_receiver) = mpsc::channel();
        let (workspace_sender, workspace_receiver) = mpsc::channel();
        let this = Self::alloc(mtm).set_ivars(AppState {
            windows: RefCell::new(Vec::new()),
            active_tab: Rc::new(Cell::new(None)),
            next_tab_id: Cell::new(1),
            highlighter: Highlighter::new(),
            startup_paths: RefCell::new(startup_paths),
            recent_menu: RefCell::new(Some(recent_menu)),
            history_sender,
            history_receiver: RefCell::new(history_receiver),
            next_history_generation: Cell::new(1),
            history_requests: RefCell::new(HashMap::new()),
            history_cache: RefCell::new(HashMap::new()),
            workspace_sender,
            workspace_receiver: RefCell::new(workspace_receiver),
            next_workspace_generation: Cell::new(1),
            workspace_generation: Cell::new(0),
            workspace: RefCell::new(None),
            next_workspace_search_generation: Cell::new(1),
            workspace_search_requests: RefCell::new(HashMap::new()),
            workspace_search_cache: RefCell::new(HashMap::new()),
            workspace_watcher: RefCell::new(None),
            workspace_watcher_root: RefCell::new(None),
            workspace_watch_retry_at: Cell::new(None),
            pending_workspace_changes: RefCell::new(Vec::new()),
            pending_workspace_restore: RefCell::new(None),
            workspace_positions: RefCell::new(HashMap::new()),
            last_workspace_session: RefCell::new(Vec::new()),
        });
        unsafe { objc2::msg_send![super(this), init] }
    }

    fn request_history(&self, source_id: Option<u64>) {
        let Some(window) = self.message_window(source_id) else {
            return;
        };
        if !window.can_show_diff() {
            window.show_note("Document history needs a tracked file with a commit.");
            return;
        }
        let state = self.ivars();
        let generation = state.next_history_generation.get();
        state.next_history_generation.set(
            generation
                .checked_add(1)
                .expect("history request generation overflow"),
        );
        state
            .history_requests
            .borrow_mut()
            .insert(window.id, generation);
        state.history_cache.borrow_mut().remove(&window.id);
        let sender = state.history_sender.clone();
        let tab_id = window.id;
        let path = window.path.clone();
        std::thread::spawn(move || {
            let result = mdcore::diff::history_for_path(&path, 100).map_err(|err| err.to_string());
            let _ = sender.send(HistoryResponse {
                tab_id,
                generation,
                result,
            });
        });
    }

    fn drain_history_results(&self) {
        loop {
            let response = match self.ivars().history_receiver.borrow().try_recv() {
                Ok(response) => response,
                Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => break,
            };
            let expected = self
                .ivars()
                .history_requests
                .borrow()
                .get(&response.tab_id)
                .copied();
            if expected != Some(response.generation) {
                continue;
            }
            let Some(window) = self.window_by_id(response.tab_id) else {
                continue;
            };
            let script = match response.result {
                Ok(entries) => {
                    let script = crate::state::history_script(&entries, None);
                    self.ivars()
                        .history_cache
                        .borrow_mut()
                        .insert(response.tab_id, entries);
                    script
                }
                Err(error) => crate::state::history_script(&[], Some(&error)),
            };
            window.pending_scripts.borrow_mut().push(script);
        }
    }

    fn select_history(&self, source_id: Option<u64>, revision: &str) {
        let Some(window) = self.message_window(source_id) else {
            return;
        };
        let entry = self
            .ivars()
            .history_cache
            .borrow()
            .get(&window.id)
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|entry| entry.revision.as_str() == revision)
                    .cloned()
            });
        match entry {
            Some(entry) => window.show_history(entry, &self.ivars().highlighter),
            None => window.show_note("That history entry is no longer available."),
        }
    }

    fn restore_workspace_session(&self) {
        if crate::defaults::get_int_opt(crate::defaults::WORKSPACE_SESSION_VERSION_KEY) != Some(1) {
            return;
        }
        let Some(root) = crate::defaults::get_string(crate::defaults::WORKSPACE_ROOT_KEY) else {
            return;
        };
        let paths: Vec<PathBuf> = crate::defaults::get_strings(crate::defaults::WORKSPACE_TABS_KEY)
            .into_iter()
            .map(PathBuf::from)
            .collect();
        let selected = crate::defaults::get_string(crate::defaults::WORKSPACE_SELECTED_KEY)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from);
        let scrolls = crate::defaults::get_strings(crate::defaults::WORKSPACE_SCROLLS_KEY);
        let positions = paths
            .iter()
            .cloned()
            .zip(
                scrolls
                    .iter()
                    .map(|scroll| scroll.parse::<u32>().unwrap_or(0)),
            )
            .collect::<HashMap<_, _>>();
        *self.ivars().workspace_positions.borrow_mut() = positions.clone();
        *self.ivars().pending_workspace_restore.borrow_mut() = Some(WorkspaceRestore {
            paths,
            selected,
            positions,
        });
        *self.ivars().workspace_watcher.borrow_mut() = None;
        *self.ivars().workspace_watcher_root.borrow_mut() = None;
        self.start_workspace_scan(PathBuf::from(root), None, false);
    }

    fn workspace_paths(&self, requested: &[PathBuf]) -> Vec<PathBuf> {
        let workspace = self.ivars().workspace.borrow();
        let Some(workspace) = workspace.as_ref() else {
            return Vec::new();
        };
        requested
            .iter()
            .filter_map(|requested| {
                workspace
                    .snapshot
                    .files
                    .iter()
                    .find(|file| &file.path == requested)
                    .map(|file| file.path.clone())
            })
            .collect()
    }

    fn persist_workspace_session(&self) {
        let workspace = self.ivars().workspace.borrow();
        let Some(workspace) = workspace.as_ref() else {
            return;
        };
        let root = workspace.snapshot.root.path();
        let windows = self.ivars().windows.borrow();
        let workspace_windows = windows
            .iter()
            .filter(|window| {
                !window.is_closed()
                    && workspace
                        .snapshot
                        .files
                        .iter()
                        .any(|file| file.path == window.path)
            })
            .collect::<Vec<_>>();
        let tabs = workspace_windows
            .iter()
            .map(|window| window.path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let positions = self.ivars().workspace_positions.borrow();
        let scrolls = workspace_windows
            .iter()
            .map(|window| {
                positions
                    .get(&window.path)
                    .copied()
                    .unwrap_or(0)
                    .to_string()
            })
            .collect::<Vec<_>>();
        let selected = self
            .ivars()
            .active_tab
            .get()
            .and_then(|id| windows.iter().find(|window| window.id == id))
            .filter(|window| {
                workspace
                    .snapshot
                    .files
                    .iter()
                    .any(|file| file.path == window.path)
            })
            .map(|window| window.path.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut signature = vec![root.to_string_lossy().into_owned(), selected.clone()];
        signature.extend(tabs.iter().cloned());
        signature.extend(scrolls.iter().cloned());
        if *self.ivars().last_workspace_session.borrow() == signature {
            return;
        }
        *self.ivars().last_workspace_session.borrow_mut() = signature;
        crate::defaults::set_string(crate::defaults::WORKSPACE_ROOT_KEY, &root.to_string_lossy());
        crate::defaults::set_strings(crate::defaults::WORKSPACE_TABS_KEY, &tabs);
        crate::defaults::set_strings(crate::defaults::WORKSPACE_SCROLLS_KEY, &scrolls);
        crate::defaults::set_string(crate::defaults::WORKSPACE_SELECTED_KEY, &selected);
        // Written last: version 1 means all fields above form one complete
        // checkpoint, not a half-written mixture from an interrupted update.
        crate::defaults::set_int(crate::defaults::WORKSPACE_SESSION_VERSION_KEY, 1);
    }

    fn open_workspace(&self, root: PathBuf, source_id: Option<u64>) {
        *self.ivars().pending_workspace_restore.borrow_mut() = None;
        *self.ivars().workspace_watcher.borrow_mut() = None;
        *self.ivars().workspace_watcher_root.borrow_mut() = None;
        self.ivars().workspace_watch_retry_at.set(None);
        self.ivars().pending_workspace_changes.borrow_mut().clear();
        self.start_workspace_scan(root, source_id, true);
    }

    fn start_workspace_scan(&self, root: PathBuf, source_id: Option<u64>, open_first: bool) {
        let state = self.ivars();
        let generation = state.next_workspace_generation.get();
        state.next_workspace_generation.set(
            generation
                .checked_add(1)
                .expect("workspace generation overflow"),
        );
        state.workspace_generation.set(generation);
        state.workspace_search_requests.borrow_mut().clear();
        state.workspace_search_cache.borrow_mut().clear();
        self.push_workspace_to_pages();

        let sender = state.workspace_sender.clone();
        std::thread::spawn(move || {
            let result = mdcore::WorkspaceSnapshot::discover(
                &root,
                generation,
                mdcore::WorkspaceLimits::default(),
            )
            .map_err(|error| error.to_string());
            let _ = sender.send(WorkspaceResponse::Scan {
                source_id,
                generation,
                open_first,
                result: result.clone(),
            });
            if let Ok(snapshot) = result {
                let indexed = mdcore::WorkspaceIndex::from_snapshot(
                    &snapshot,
                    mdcore::WorkspaceLimits::default(),
                )
                .map_err(|error| error.to_string());
                let _ = sender.send(WorkspaceResponse::Indexed {
                    generation,
                    result: indexed,
                });
            }
        });
    }

    fn search_workspace(&self, source_id: Option<u64>, raw_query: &str) {
        let Some(window) = self.message_window(source_id) else {
            return;
        };
        let Some(query) = mdcore::SearchQuery::new(raw_query) else {
            self.ivars()
                .workspace_search_requests
                .borrow_mut()
                .remove(&window.id);
            self.ivars()
                .workspace_search_cache
                .borrow_mut()
                .remove(&window.id);
            window
                .pending_scripts
                .borrow_mut()
                .push(crate::state::workspace_search_script(&[], None));
            return;
        };
        let state = self.ivars();
        let index = match state.workspace.borrow().as_ref() {
            Some(workspace) => match workspace.index.clone() {
                Some(index) => index,
                None => {
                    window.pending_scripts.borrow_mut().push(
                        crate::state::workspace_search_script(
                            &[],
                            Some("The workspace is still being indexed."),
                        ),
                    );
                    return;
                }
            },
            None => {
                window
                    .pending_scripts
                    .borrow_mut()
                    .push(crate::state::workspace_search_script(
                        &[],
                        Some("Open a folder to search it."),
                    ));
                return;
            }
        };
        let request_generation = state.next_workspace_search_generation.get();
        state.next_workspace_search_generation.set(
            request_generation
                .checked_add(1)
                .expect("workspace search generation overflow"),
        );
        state
            .workspace_search_requests
            .borrow_mut()
            .insert(window.id, request_generation);
        let workspace_generation = state.workspace_generation.get();
        let sender = state.workspace_sender.clone();
        let tab_id = window.id;
        let query_text = query.as_str().to_string();
        std::thread::spawn(move || {
            let result = index.search(&query);
            let _ = sender.send(WorkspaceResponse::Search {
                tab_id,
                workspace_generation,
                request_generation,
                query: query_text,
                result,
            });
        });
    }

    fn open_workspace_path(&self, source_id: Option<u64>, raw_path: &str) {
        let requested = PathBuf::from(raw_path);
        let current = std::fs::canonicalize(&requested).ok();
        let path = self
            .ivars()
            .workspace
            .borrow()
            .as_ref()
            .and_then(|workspace| {
                workspace
                    .snapshot
                    .files
                    .iter()
                    .find(|file| {
                        file.path == requested
                            && current.as_ref() == Some(&file.path)
                            && std::fs::symlink_metadata(&file.path)
                                .map(|metadata| !metadata.file_type().is_symlink())
                                .unwrap_or(false)
                    })
                    .map(|file| file.path.clone())
            });
        match path {
            Some(path) => {
                let reveal = source_id.and_then(|tab_id| {
                    self.ivars()
                        .workspace_search_cache
                        .borrow()
                        .get(&tab_id)
                        .and_then(|(query, hits)| {
                            hits.iter()
                                .find(|hit| hit.path == path)
                                .map(|hit| (query.clone(), hit.heading.clone()))
                        })
                });
                self.open_documents_from(&[path.clone()], source_id);
                if let Some((query, heading)) = reveal {
                    if let Some(window) = self
                        .ivars()
                        .windows
                        .borrow()
                        .iter()
                        .find(|window| window.path == path)
                        .cloned()
                    {
                        window.pending_scripts.borrow_mut().push(
                            crate::state::workspace_reveal_script(heading.as_deref(), &query),
                        );
                    }
                }
            }
            None => {
                if let Some(window) = self.message_window(source_id) {
                    window.show_note("That file is no longer in the workspace.");
                }
            }
        }
    }

    fn reconcile_workspace(&self, paths: Vec<PathBuf>) {
        let state = self.ivars();
        let (root, mut index) = match state.workspace.borrow().as_ref() {
            Some(workspace) => match workspace.index.as_ref() {
                Some(index) => (
                    workspace.snapshot.root.path().to_path_buf(),
                    (**index).clone(),
                ),
                None => return,
            },
            None => return,
        };
        let generation = state.next_workspace_generation.get();
        state.next_workspace_generation.set(
            generation
                .checked_add(1)
                .expect("workspace generation overflow"),
        );
        state.workspace_generation.set(generation);
        let sender = state.workspace_sender.clone();
        std::thread::spawn(move || {
            let incremental = paths
                .iter()
                .try_for_each(|path| {
                    if path.is_file() {
                        index.upsert(path)
                    } else {
                        index.remove(path).map(|_| ())
                    }
                })
                .map(|_| index)
                .map_err(|error| error.to_string());
            let _ = sender.send(WorkspaceResponse::Incremental {
                generation,
                result: incremental,
            });

            // Watchers can coalesce or omit individual events. The incremental
            // result arrives first; this bounded full scan is the correctness
            // backstop that makes the final tree converge.
            let scanned = mdcore::WorkspaceSnapshot::discover(
                &root,
                generation,
                mdcore::WorkspaceLimits::default(),
            )
            .map_err(|error| error.to_string());
            let _ = sender.send(WorkspaceResponse::Scan {
                source_id: None,
                generation,
                open_first: false,
                result: scanned.clone(),
            });
            if let Ok(snapshot) = scanned {
                let indexed = mdcore::WorkspaceIndex::from_snapshot(
                    &snapshot,
                    mdcore::WorkspaceLimits::default(),
                )
                .map_err(|error| error.to_string());
                let _ = sender.send(WorkspaceResponse::Indexed {
                    generation,
                    result: indexed,
                });
            }
        });
    }

    fn drain_workspace_results(&self) {
        loop {
            let response = match self.ivars().workspace_receiver.borrow().try_recv() {
                Ok(response) => response,
                Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => break,
            };
            match response {
                WorkspaceResponse::Scan {
                    source_id,
                    generation,
                    open_first,
                    result,
                } => {
                    if self.ivars().workspace_generation.get() != generation {
                        continue;
                    }
                    match result {
                        Ok(snapshot) => {
                            let first = snapshot.files.first().map(|file| file.path.clone());
                            let root = snapshot.root.path().to_path_buf();
                            *self.ivars().workspace.borrow_mut() = Some(WorkspaceData {
                                snapshot,
                                index: None,
                            });
                            let watcher_matches = self
                                .ivars()
                                .workspace_watcher_root
                                .borrow()
                                .as_ref()
                                == Some(&root);
                            if !watcher_matches {
                                let watcher = crate::watcher::WorkspaceWatcher::start(&root).ok();
                                if watcher.is_some() {
                                    *self.ivars().workspace_watcher_root.borrow_mut() =
                                        Some(root.clone());
                                } else {
                                    *self.ivars().workspace_watcher_root.borrow_mut() = None;
                                }
                                *self.ivars().workspace_watcher.borrow_mut() = watcher;
                            }
                            self.push_workspace_to_pages();

                            if let Some(restore) =
                                self.ivars().pending_workspace_restore.borrow_mut().take()
                            {
                                let mut valid = self.workspace_paths(&restore.paths);
                                if valid.is_empty() {
                                    if let Some(path) = first.clone() {
                                        valid.push(path);
                                    }
                                }
                                self.open_documents_from(&valid, None);
                                for path in &valid {
                                    if let Some(position) = restore.positions.get(path) {
                                        if let Some(window) = self
                                            .ivars()
                                            .windows
                                            .borrow()
                                            .iter()
                                            .find(|window| &window.path == path)
                                            .cloned()
                                        {
                                            window
                                                .pending_scripts
                                                .borrow_mut()
                                                .push(format!("window.scrollTo(0, {position});"));
                                        }
                                    }
                                }
                                if let Some(selected) = restore.selected {
                                    if let Some(window) = self
                                        .ivars()
                                        .windows
                                        .borrow()
                                        .iter()
                                        .find(|window| window.path == selected)
                                        .cloned()
                                    {
                                        self.ivars().active_tab.set(Some(window.id));
                                        window.window.makeKeyAndOrderFront(None);
                                    }
                                }
                            } else if open_first {
                                if let Some(path) = first {
                                    self.open_documents_from(&[path], source_id);
                                } else if let Some(window) = self.message_window(source_id) {
                                    window.show_note("This folder has no Markdown files.");
                                }
                            }
                        }
                        Err(error) => {
                            self.ivars().pending_workspace_restore.borrow_mut().take();
                            if let Some(window) = self.message_window(source_id) {
                                window.show_note(&error);
                            }
                            if source_id.is_none() && self.ivars().workspace.borrow().is_none() {
                                self.ivars().workspace_generation.set(0);
                                crate::defaults::set_int(
                                    crate::defaults::WORKSPACE_SESSION_VERSION_KEY,
                                    0,
                                );
                                self.present_open_panel();
                            } else if let Some(root) = self
                                .ivars()
                                .workspace
                                .borrow()
                                .as_ref()
                                .map(|workspace| workspace.snapshot.root.path().to_path_buf())
                            {
                                let watcher = crate::watcher::WorkspaceWatcher::start(&root).ok();
                                *self.ivars().workspace_watcher_root.borrow_mut() =
                                    watcher.as_ref().map(|_| root);
                                *self.ivars().workspace_watcher.borrow_mut() = watcher;
                            }
                        }
                    }
                }
                WorkspaceResponse::Indexed { generation, result } => {
                    if self.ivars().workspace_generation.get() != generation {
                        continue;
                    }
                    match result {
                        Ok(index) => {
                            if let Some(workspace) = self.ivars().workspace.borrow_mut().as_mut() {
                                workspace.snapshot.files = index.files().cloned().collect();
                                workspace.snapshot.indexed_bytes = workspace
                                    .snapshot
                                    .files
                                    .iter()
                                    .map(|file| file.size)
                                    .sum();
                                workspace.index = Some(Arc::new(index));
                            }
                            self.push_workspace_to_pages();
                            let pending = std::mem::take(
                                &mut *self.ivars().pending_workspace_changes.borrow_mut(),
                            );
                            if !pending.is_empty() {
                                self.reconcile_workspace(pending);
                            }
                        }
                        Err(error) => {
                            let pending = std::mem::take(
                                &mut *self.ivars().pending_workspace_changes.borrow_mut(),
                            );
                            if !pending.is_empty() {
                                if let Some(root) = self
                                    .ivars()
                                    .workspace
                                    .borrow()
                                    .as_ref()
                                    .map(|workspace| workspace.snapshot.root.path().to_path_buf())
                                {
                                    self.start_workspace_scan(root, None, false);
                                }
                            } else if let Some(window) = self.frontmost_window() {
                                window.show_note(&error);
                            }
                        }
                    }
                }
                WorkspaceResponse::Incremental { generation, result } => {
                    if self.ivars().workspace_generation.get() != generation {
                        continue;
                    }
                    match result {
                        Ok(index) => {
                            if let Some(workspace) = self.ivars().workspace.borrow_mut().as_mut() {
                                workspace.snapshot.files = index.files().cloned().collect();
                                workspace.snapshot.indexed_bytes =
                                    workspace.snapshot.files.iter().map(|file| file.size).sum();
                                workspace.index = Some(Arc::new(index));
                            }
                            self.push_workspace_to_pages();
                        }
                        Err(_) => {
                            let root = self
                                .ivars()
                                .workspace
                                .borrow()
                                .as_ref()
                                .map(|workspace| workspace.snapshot.root.path().to_path_buf());
                            if let Some(root) = root {
                                self.start_workspace_scan(root, None, false);
                            }
                        }
                    }
                }
                WorkspaceResponse::Search {
                    tab_id,
                    workspace_generation,
                    request_generation,
                    query,
                    result,
                } => {
                    if self.ivars().workspace_generation.get() != workspace_generation
                        || self
                            .ivars()
                            .workspace_search_requests
                            .borrow()
                            .get(&tab_id)
                            .copied()
                            != Some(request_generation)
                    {
                        continue;
                    }
                    if let Some(window) = self.window_by_id(tab_id) {
                        self.ivars()
                            .workspace_search_cache
                            .borrow_mut()
                            .insert(tab_id, (query, result.clone()));
                        window
                            .pending_scripts
                            .borrow_mut()
                            .push(crate::state::workspace_search_script(&result, None));
                    }
                }
            }
        }
    }

    fn push_workspace_to_pages(&self) {
        let workspace = self.ivars().workspace.borrow();
        let script = crate::state::workspace_files_script(
            workspace.as_ref().map(|workspace| &workspace.snapshot),
            None,
        );
        for window in self.ivars().windows.borrow().iter() {
            window.pending_scripts.borrow_mut().push(script.clone());
        }
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

    /// Open one ordered batch of documents as tabs. Canonical paths are tab
    /// identities: asking for an already-open document selects it rather than
    /// creating a second watcher and web view for the same file.
    fn open_documents(&self, paths: &[std::path::PathBuf]) {
        self.open_documents_from(paths, None);
    }

    fn open_documents_from(&self, paths: &[std::path::PathBuf], source_id: Option<u64>) {
        use crate::navigation::NavigationRequest;
        use objc2_app_kit::NSWorkspace;

        let mut canonical = Vec::with_capacity(paths.len());
        for path in paths {
            // Persisted state and open-tab identity are keyed by path string, so
            // relative and symlinked spellings must converge before either is
            // inspected. Resolving the parent and rejoining the filename also
            // gives a stable absolute identity to a file that is missing now but
            // may reappear while its error tab remains open.
            let path = crate::watcher::watch_target(path);
            if !canonical.contains(&path) {
                canonical.push(path);
            }
        }
        if canonical.is_empty() {
            return;
        }

        // A non-UTF-8 path still opens, but cannot be represented in the
        // string-backed recent-file store. Record the rest as one batch so the
        // first requested path remains the newest entry.
        let history_paths: Vec<String> = canonical
            .iter()
            .filter_map(|path| path.to_str().map(str::to_owned))
            .collect();
        let history = crate::state::push_history_batch(
            &crate::defaults::get_strings(crate::defaults::HISTORY_KEY),
            &history_paths,
            50,
        );
        crate::defaults::set_strings(crate::defaults::HISTORY_KEY, &history);

        let state = self.ivars();
        let mtm = MainThreadMarker::from(self);
        let mut insertion_anchor = source_id
            .and_then(|id| self.window_by_id(id))
            .or_else(|| self.frontmost_window());
        let mut first_requested = None;

        for path in &canonical {
            let existing = state
                .windows
                .borrow()
                .iter()
                .find(|window| !window.is_closed() && window.path.as_path() == path)
                .cloned();

            let (document, created) = if let Some(existing) = existing {
                (existing, false)
            } else {
                let id = state.next_tab_id.get();
                state
                    .next_tab_id
                    .set(id.checked_add(1).expect("tab identity overflow"));

                // The web view calls back on the main thread, so a plain Rc
                // closure holding a retained delegate is sound here.
                let delegate: Retained<AppDelegate> =
                    unsafe { Retained::retain(self as *const _ as *mut _) }
                        .expect("delegate is alive while its windows are");
                let handler: Rc<dyn Fn(NavigationRequest)> =
                    Rc::new(move |request| match request {
                        NavigationRequest::OpenExternal(url) => {
                            let workspace = NSWorkspace::sharedWorkspace();
                            if let Some(url) = NSURL::URLWithString(&NSString::from_str(&url)) {
                                workspace.openURL(&url);
                            }
                        }
                        NavigationRequest::OpenDocument(path) => {
                            delegate.open_document_from(id, &path)
                        }
                    });

                let msg_delegate: Retained<AppDelegate> =
                    unsafe { Retained::retain(self as *const _ as *mut _) }
                        .expect("delegate is alive while its windows are");
                let on_message: Rc<dyn Fn(crate::state::Message)> =
                    Rc::new(move |message| msg_delegate.handle_message_from(Some(id), message));

                // DocumentWindow constructs and renders without presenting.
                // Attach it first so opening a tab never flashes a detached
                // centered window on screen.
                let document = DocumentWindow::open(
                    id,
                    path,
                    mtm,
                    &state.highlighter,
                    state.active_tab.clone(),
                    handler,
                    on_message,
                );
                if let Some(anchor) = insertion_anchor.as_ref() {
                    if let Some(group) = anchor.window.tabGroup() {
                        let tabs = group.windows();
                        let index = tabs
                            .iter()
                            .position(|candidate| {
                                std::ptr::eq(
                                    Retained::as_ptr(&candidate),
                                    Retained::as_ptr(&anchor.window),
                                )
                            })
                            .map(|index| index + 1)
                            .unwrap_or_else(|| tabs.len());
                        group.insertWindow_atIndex(&document.window, index as isize);
                    } else {
                        anchor
                            .window
                            .addTabbedWindow_ordered(&document.window, NSWindowOrderingMode::Above);
                    }
                } else {
                    document.window.center();
                }
                state.windows.borrow_mut().push(document.clone());
                (document, true)
            };

            if first_requested.is_none() {
                first_requested = Some(document.clone());
            }
            // Only newly created tabs advance the insertion point. Existing tabs
            // keep their position and cannot redirect the rest of a batch into a
            // different native tab group.
            if created {
                insertion_anchor = Some(document);
            }
        }

        if let Some(document) = first_requested {
            state.active_tab.set(Some(document.id));
            document.window.makeKeyAndOrderFront(None);
            self.maybe_queue_shortcuts_hint(&document);
        }
        self.push_bookmarks_to_pages();
        self.push_comments_to_pages();
        self.push_recents_to_pages();
        self.push_workspace_to_pages();
        self.rebuild_recent_menu();
    }

    /// The single-document form used by local links, recents, and bookmarks.
    /// Every other opening source funnels into `open_documents` directly.
    pub fn open_document(&self, path: &std::path::Path) {
        self.open_documents(&[path.to_path_buf()]);
    }

    fn open_document_from(&self, source_id: u64, path: &std::path::Path) {
        // A late navigation from a tab that has already closed must not create
        // new UI beside whichever document happens to be active now.
        if self.window_by_id(source_id).is_some() {
            self.open_documents_from(&[path.to_path_buf()], Some(source_id));
        }
    }

    fn window_by_id(&self, id: u64) -> Option<Rc<DocumentWindow>> {
        self.ivars()
            .windows
            .borrow()
            .iter()
            .find(|document| document.id == id && !document.is_closed())
            .cloned()
    }

    /// The document tab the user is looking at, or None when all tabs are
    /// closed. The key window is authoritative during normal interaction. A
    /// modal panel temporarily becomes key itself, so fall back to AppKit's
    /// selected window in the native tab group before using creation order.
    fn frontmost_window(&self) -> Option<Rc<DocumentWindow>> {
        let windows = self.ivars().windows.borrow();
        windows
            .iter()
            .find(|document| !document.is_closed() && document.window.isKeyWindow())
            .or_else(|| {
                let active = self.ivars().active_tab.get()?;
                windows
                    .iter()
                    .find(|document| document.id == active && !document.is_closed())
            })
            .or_else(|| {
                windows.iter().find(|document| {
                    !document.is_closed()
                        && document
                            .window
                            .tabGroup()
                            .and_then(|group| group.selectedWindow())
                            .is_some_and(|selected| {
                                std::ptr::eq(
                                    Retained::as_ptr(&selected),
                                    Retained::as_ptr(&document.window),
                                )
                            })
                })
            })
            .or_else(|| windows.iter().rev().find(|document| !document.is_closed()))
            .cloned()
    }

    fn select_document_tab(&self, source_id: Option<u64>, forward: bool) {
        let Some(document) = self.message_window(source_id) else {
            return;
        };
        if forward {
            document.window.selectNextTab(None);
        } else {
            document.window.selectPreviousTab(None);
        }
    }

    fn message_window(&self, source_id: Option<u64>) -> Option<Rc<DocumentWindow>> {
        match source_id {
            // Page-originated callbacks are dropped if their tab has closed;
            // falling back here would apply a late message to another document.
            Some(id) => self.window_by_id(id),
            None => self.frontmost_window(),
        }
    }

    fn adjust_zoom(&self, source_id: Option<u64>, factor: f64) {
        if let Some(window) = self.message_window(source_id) {
            let current = unsafe { window.webview.pageZoom() };
            // Clamp so repeated presses cannot make the document unreadable.
            let next = (current * factor).clamp(0.5, 3.0);
            unsafe { window.webview.setPageZoom(next) };
        }
    }

    pub(crate) fn handle_message(&self, message: crate::state::Message) {
        self.handle_message_from(None, message);
    }

    fn handle_message_from(&self, source_id: Option<u64>, message: crate::state::Message) {
        use crate::state::Message;
        match message {
            Message::SetTheme(theme, scroll_opt) => {
                crate::defaults::set_string(crate::defaults::THEME_KEY, theme.as_wire());
                // The theme is a global preference, not a per-window one: every
                // open window must pick it up, or the others would keep the old
                // theme indefinitely. Collect the Rcs first, then reload outside
                // the borrow — matching watch_tick's pattern — since reload can
                // reenter `windows` (e.g. via message handling on the new page).
                let source = self.message_window(source_id);
                let windows: Vec<Rc<DocumentWindow>> =
                    self.ivars().windows.borrow().iter().cloned().collect();
                for window in &windows {
                    // A runtime theme change cannot swap the pinned sheet's
                    // contents — that CSS is baked in by Rust for the theme the
                    // page was built with. Reload so the new theme's pinned
                    // sheet is emitted.
                    window.reload(&self.ivars().highlighter);
                }
                // The fresh page's star is unstarred and its bookmark list is
                // empty until this runs — without it a bookmarked document
                // reads as unbookmarked after a theme change, and ⌘D would then
                // remove the entry instead of adding it back.
                self.push_bookmarks_to_pages();
                self.push_comments_to_pages();
                self.push_recents_to_pages();
                // Only the window whose picker sent this carries a meaningful
                // scroll offset; other windows just reload at the top. Queue
                // after the sidebar-restore script (pushed by `reload` above),
                // or the offset would apply while the sidebar is still hidden
                // and the layout differs.
                if let Some(y) = scroll_opt {
                    if let Some(window) = source {
                        let script = format!("window.scrollTo(0, {});", y);
                        window.pending_scripts.borrow_mut().push(script);
                    }
                }
            }
            Message::ToggleBookmark => {
                let Some(window) = self.message_window(source_id) else {
                    return;
                };
                let path = window.path.to_string_lossy().into_owned();
                let updated = crate::state::toggle_bookmark(
                    &crate::defaults::get_strings(crate::defaults::BOOKMARKS_KEY),
                    &path,
                );
                crate::defaults::set_strings(crate::defaults::BOOKMARKS_KEY, &updated);
                self.push_bookmarks_to_pages();
                self.push_comments_to_pages();
            }
            Message::ToggleDiff => {
                if let Some(window) = self.message_window(source_id) {
                    if window.can_show_diff() || window.view_mode() == crate::window::ViewMode::Diff
                    {
                        window.toggle_diff(&self.ivars().highlighter);
                    }
                }
            }
            Message::SetDiffLayout(layout) => {
                if let Some(window) = self.message_window(source_id) {
                    window.set_diff_layout(layout, &self.ivars().highlighter);
                }
            }
            Message::OpenHistory => self.request_history(source_id),
            Message::SelectHistory(revision) => self.select_history(source_id, &revision),
            Message::SearchWorkspace(query) => self.search_workspace(source_id, &query),
            Message::OpenWorkspacePath(path) => self.open_workspace_path(source_id, &path),
            Message::SetReadingPosition(position) => {
                if let Some(window) = self.message_window(source_id) {
                    if self
                        .ivars()
                        .workspace
                        .borrow()
                        .as_ref()
                        .is_some_and(|workspace| {
                            window
                                .path
                                .strip_prefix(workspace.snapshot.root.path())
                                .is_ok()
                        })
                    {
                        self.ivars()
                            .workspace_positions
                            .borrow_mut()
                            .insert(window.path.clone(), position);
                    }
                }
            }
            Message::ToggleFullWidth => {
                self.toggle_full_width_native();
            }
            Message::NextTab => self.select_document_tab(source_id, true),
            Message::PreviousTab => self.select_document_tab(source_id, false),
            Message::OpenPath(path) => match source_id {
                Some(id) => self.open_document_from(id, std::path::Path::new(&path)),
                None => self.open_document(std::path::Path::new(&path)),
            },
            Message::SetSidebar { open, tab } => {
                crate::defaults::set_bool(crate::defaults::SIDEBAR_OPEN_KEY, open);
                crate::defaults::set_string(crate::defaults::SIDEBAR_TAB_KEY, &tab);
            }
            Message::SetSidebarWidth(px) => {
                let clamped = crate::state::clamp_sidebar_width(px);
                crate::defaults::set_int(crate::defaults::SIDEBAR_WIDTH_KEY, clamped as i64);
            }
            Message::SetMinimap(open) => {
                crate::defaults::set_bool(crate::defaults::MINIMAP_OPEN_KEY, open);
            }
            // These four go through the same paths as their menu items, so the
            // page's `r`, `+`, `-` and `0` cannot drift from ⌘R and ⌘=/⌘-/⌘0.
            Message::ReloadDocument => {
                if let Some(window) = self.message_window(source_id) {
                    window.reload(&self.ivars().highlighter);
                    self.push_bookmarks_to_pages();
                    self.push_comments_to_pages();
                    self.push_recents_to_pages();
                    self.push_workspace_to_pages();
                }
            }
            Message::AddComment {
                heading,
                nth,
                quote,
                note,
            } => {
                let Some(window) = self.message_window(source_id) else {
                    return;
                };
                let doc = window.path.clone();
                let key = doc.to_string_lossy().into_owned();
                let mut comments = crate::store::load(&key).comments;
                if comments.len() >= crate::store::COMMENT_LIMIT {
                    window.show_banner(
                        "mdview-comments",
                        "This document already has as many comments as MDView keeps.",
                    );
                    return;
                }
                let id = crate::review::fresh_id(&comments);
                comments.push(crate::review::Comment::new(
                    &id, heading, nth, &quote, &note,
                ));
                self.write_review(&window, &doc, &comments);
            }
            Message::EditComment { id, note } => {
                let Some(window) = self.message_window(source_id) else {
                    return;
                };
                let doc = window.path.clone();
                let key = doc.to_string_lossy().into_owned();
                let mut comments = crate::store::load(&key).comments;
                let Some(target) = comments.iter_mut().find(|c| c.id == id) else {
                    // It went away under us — a hand-edit of the review file,
                    // most likely. Re-push so the page stops showing it.
                    self.push_comments_to_pages();
                    return;
                };
                *target = crate::review::Comment::new(
                    &target.id,
                    target.heading,
                    target.nth,
                    &target.quote,
                    &note,
                );
                self.write_review(&window, &doc, &comments);
            }
            Message::DeleteComment { id } => {
                let Some(window) = self.message_window(source_id) else {
                    return;
                };
                let doc = window.path.clone();
                let key = doc.to_string_lossy().into_owned();
                // `x` has no undo, so the previous file is the recovery.
                crate::store::backup(&key);
                let mut comments = crate::store::load(&key).comments;
                comments.retain(|c| c.id != id);
                self.write_review(&window, &doc, &comments);
            }
            Message::CopyReview => self.copy_review_prompt(source_id),
            Message::ZoomIn => self.adjust_zoom(source_id, ZOOM_STEP),
            Message::ZoomOut => self.adjust_zoom(source_id, 1.0 / ZOOM_STEP),
            Message::ZoomReset => {
                if let Some(window) = self.message_window(source_id) {
                    unsafe { window.webview.setPageZoom(1.0) };
                }
            }
        }
    }

    /// Persist a document's comments and tell every page about them.
    ///
    /// A failed write raises a banner rather than passing in silence: the page
    /// would otherwise go on showing a comment that is not stored, and the
    /// next launch would lose it with no explanation.
    fn write_review(
        &self,
        window: &Rc<DocumentWindow>,
        doc: &std::path::Path,
        comments: &[crate::review::Comment],
    ) {
        let key = doc.to_string_lossy().into_owned();
        // The one gate on every write. `serialize_review` renders the comment
        // list and nothing else, so writing a file we only partly understood
        // erases every record we had to skip — and it is read again here, not
        // taken from the caller, so nothing that happened between their read
        // and this write slips past.
        if self.review_is_damaged(window, &key) {
            return;
        }
        let headings = crate::store::headings_of(doc);
        match crate::store::save(&key, &headings, comments) {
            Ok(()) => self.push_comments_to_pages(),
            Err(err) => window.show_banner(
                "mdview-comments",
                &format!("Could not save the review: {err}"),
            ),
        }
    }

    /// Whether the review file has records MDView could not read, raising the
    /// banner that says so. True means: do not write to this file.
    fn review_is_damaged(&self, window: &Rc<DocumentWindow>, key: &str) -> bool {
        let damage = crate::store::load(key).damage;
        let path = crate::store::review_path(key).unwrap_or_default();
        match crate::state::damaged_review_banner(&damage, &path.to_string_lossy()) {
            Some(message) => {
                window.show_banner("mdview-review", &message);
                true
            }
            None => false,
        }
    }

    /// Send each page ITS OWN document's comments. Unlike the bookmark list,
    /// which is global, a review belongs to one document — pushing one
    /// window's comments to all of them would anchor them in the wrong text.
    pub(crate) fn push_comments_to_pages(&self) {
        for window in self.ivars().windows.borrow().iter() {
            let key = window.path.to_string_lossy().into_owned();
            let review = crate::store::load(&key);
            // What could be read still goes to the page. Showing nothing
            // because one record is bad would hide the comments that are fine.
            let script = crate::state::comments_script(&review.comments);
            // Queued, never evaluated. See push_bookmarks_to_pages.
            window.pending_scripts.borrow_mut().push(script);
            // The banner tracks the FILE, not the moment it went wrong: raised
            // whenever a record cannot be read and taken away as soon as it can
            // be again. This is the funnel the review watcher runs, so fixing
            // the file by hand clears the banner without touching MDView.
            let path = crate::store::review_path(&key).unwrap_or_default();
            match crate::state::damaged_review_banner(&review.damage, &path.to_string_lossy()) {
                // Queued for the same reason the script is: this runs on every
                // open, when the page it would be injected into does not exist.
                Some(message) => window
                    .pending_banners
                    .borrow_mut()
                    .push(("mdview-review".to_string(), message)),
                // Direct, and safe to lose: a page too new to receive this is
                // also too new to be carrying the banner it would clear.
                None => window.clear_banner("mdview-review"),
            }
        }
    }

    /// Put the review prompt on the pasteboard, for pasting into Claude.
    fn copy_review_prompt(&self, source_id: Option<u64>) {
        use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};

        let Some(window) = self.message_window(source_id) else {
            return;
        };
        let doc = window.path.clone();
        let key = doc.to_string_lossy().into_owned();
        let comments = crate::store::load(&key).comments;
        if comments.is_empty() {
            window.show_note("No comments on this document yet.");
            return;
        }
        // Write before copying, so the path in the prompt names a file that
        // exists and is current even if an earlier write failed. Skipped when
        // the file cannot be wholly read: the prompt is still worth copying,
        // and rewriting it here would erase exactly the records the banner is
        // about — the ones a previous round of this same prompt mangled.
        if !self.review_is_damaged(&window, &key) {
            let headings = crate::store::headings_of(&doc);
            let _ = crate::store::save(&key, &headings, &comments);
        }
        let Some(review) = crate::store::review_path(&key) else {
            return;
        };
        let prompt = crate::state::review_prompt(&review.to_string_lossy(), &key);

        let pasteboard = NSPasteboard::generalPasteboard();
        // Without clearContents the write is refused and returns false, with
        // nothing else to say that nothing was copied.
        unsafe {
            pasteboard.clearContents();
            pasteboard.setString_forType(&NSString::from_str(&prompt), NSPasteboardTypeString);
        }
        window.show_note("Review prompt copied — paste it into Claude.");
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
            let current = window.path.to_string_lossy().into_owned();
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

    /// Send the recent-files list to every open page, each window getting the
    /// list with ITS OWN document taken out. The palette is a way to somewhere
    /// else, and the document already on screen is the one row that could do
    /// nothing -- leaving it in would put it under the highlight the moment
    /// the palette opens, which is where enter lands.
    ///
    /// Entries whose file has gone are filtered out of the DISPLAY only, the
    /// rule Open Recent already follows: an unmounted volume must not silently
    /// erase the history.
    pub(crate) fn push_recents_to_pages(&self) {
        let live = self.live_history();
        let home = std::env::var("HOME").ok();
        for window in self.ivars().windows.borrow().iter() {
            let current = window.path.to_string_lossy().into_owned();
            let script = crate::state::recents_script(&live, &current, home.as_deref());
            // Queued, never evaluated. See push_bookmarks_to_pages.
            window.pending_scripts.borrow_mut().push(script);
        }
    }

    pub(crate) fn present_open_folder_panel(&self) {
        use objc2_app_kit::{NSModalResponse, NSOpenPanel};

        let source_id = self.frontmost_window().map(|window| window.id);
        let mtm = MainThreadMarker::from(self);
        let panel = NSOpenPanel::openPanel(mtm);
        panel.setCanChooseFiles(false);
        panel.setCanChooseDirectories(true);
        panel.setAllowsMultipleSelection(false);

        let response: NSModalResponse = panel.runModal();
        if response != 1 {
            return;
        }
        let Some(url) = panel.URL() else { return };
        let Some(path) = url.path() else { return };
        self.open_workspace(PathBuf::from(path.to_string()), source_id);
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
        self.open_documents(&paths);
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
