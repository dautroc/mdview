use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use mdcore::Highlighter;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate};
use objc2_foundation::{MainThreadMarker, NSArray, NSNotification, NSURL};

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
        }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn terminate_after_last_window(&self, _app: &NSApplication) -> bool {
            true
        }

        #[unsafe(method(application:openURLs:))]
        fn open_urls(&self, _app: &NSApplication, urls: &NSArray<NSURL>) {
            for url in urls.iter() {
                // Ignore anything that is not a local file; the app has no
                // business fetching remote documents.
                let Some(path) = url.path() else {
                    continue;
                };
                self.open_document(std::path::Path::new(&path.to_string()));
            }
        }

        #[unsafe(method(applicationShouldOpenUntitledFile:))]
        fn should_open_untitled(&self, _app: &NSApplication) -> bool {
            // A viewer has no untitled state. Clicking the Dock icon with no
            // windows open should present the Open panel instead of a blank
            // window, which is what `applicationOpenUntitledFile:` does below.
            true
        }

        #[unsafe(method(applicationOpenUntitledFile:))]
        fn open_untitled(&self, _app: &NSApplication) -> bool {
            self.present_open_panel();
            true
        }
    }

    impl AppDelegate {
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
        let mtm = MainThreadMarker::from(self);
        let state = self.ivars();
        let window = DocumentWindow::open(path, mtm, &state.highlighter);
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
