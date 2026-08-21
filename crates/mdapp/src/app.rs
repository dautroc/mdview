use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use mdcore::Highlighter;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate};
use objc2_foundation::{MainThreadMarker, NSNotification};

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
}

pub fn run(paths: Vec<PathBuf>) -> ! {
    let mtm = MainThreadMarker::new().expect("main() runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

    let delegate = AppDelegate::new(mtm, paths);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    app.run();
    unreachable!("NSApplication::run does not return");
}
