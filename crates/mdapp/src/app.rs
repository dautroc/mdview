use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use mdcore::Highlighter;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate};
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
            for path in paths {
                self.open_document(&path);
            }

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
            // Show the FIRST — discarding it in favour of the last would throw
            // away the file the user most likely meant.
            let mut opened = false;
            for url in urls.iter() {
                if !url.isFileURL() {
                    continue;
                }
                let Some(path) = url.path() else { continue };
                let path = std::path::PathBuf::from(path.to_string());
                if !opened {
                    self.open_document(&path);
                    opened = true;
                }
            }
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
    }
);

impl AppDelegate {
    pub fn new(mtm: MainThreadMarker, startup_paths: Vec<PathBuf>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(AppState {
            windows: RefCell::new(Vec::new()),
            highlighter: Highlighter::new(),
            startup_paths: RefCell::new(startup_paths),
        });
        unsafe { objc2::msg_send![super(this), init] }
    }

    /// The single entry point every way of opening a file funnels into:
    /// startup arguments, Finder, the Open panel, and dropped files.
    pub fn open_document(&self, path: &std::path::Path) {
        use crate::navigation::NavigationRequest;
        use objc2_app_kit::NSWorkspace;

        let state = self.ivars();

        // Reuse the window the user is looking at rather than stacking a new
        // one per document. `frontmost_window` is the same notion of "current
        // window" that Reload and the zoom items use, so they cannot disagree.
        if let Some(existing) = self.frontmost_window() {
            existing.load(path, &state.highlighter);
            existing.window.makeKeyAndOrderFront(None);
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

        let window = DocumentWindow::open(path, mtm, &state.highlighter, handler);
        state.windows.borrow_mut().push(window);
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
        for url in urls.iter() {
            if let Some(path) = url.path() {
                self.open_document(std::path::Path::new(&path.to_string()));
            }
        }
    }
}

pub fn run(paths: Vec<PathBuf>) -> ! {
    let mtm = MainThreadMarker::new().expect("main() runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

    crate::menu::install(&app, mtm);

    let delegate = AppDelegate::new(mtm, paths);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    app.run();
    unreachable!("NSApplication::run does not return");
}
