use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use mdcore::Highlighter;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSNotification, NSPoint, NSRect, NSSize, NSString, NSURL};
use objc2_web_kit::{WKWebView, WKWebViewConfiguration};

/// Ivars for `WindowCloseDelegate`: just the shared flag it flips.
pub struct WindowCloseState {
    closed: Rc<Cell<bool>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDViewWindowCloseDelegate"]
    #[ivars = WindowCloseState]
    pub struct WindowCloseDelegate;

    unsafe impl NSObjectProtocol for WindowCloseDelegate {}

    unsafe impl NSWindowDelegate for WindowCloseDelegate {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            self.ivars().closed.set(true);
        }
    }
);

impl WindowCloseDelegate {
    fn new(mtm: MainThreadMarker, closed: Rc<Cell<bool>>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(WindowCloseState { closed });
        unsafe { objc2::msg_send![super(this), init] }
    }
}

/// One window showing one document. Owns its web view and its own file
/// watcher; closing a window tears down only that window's resources.
pub struct DocumentWindow {
    pub window: Retained<NSWindow>,
    pub webview: Retained<WKWebView>,
    pub path: RefCell<PathBuf>,
    /// Held so the delegate outlives the web view; WKWebView keeps only a
    /// weak reference to its navigation delegate.
    _navigation: Retained<crate::navigation::NavigationDelegate>,
    /// Held so the delegate outlives the window; `NSWindow.delegate` is also
    /// a weak property, so an unheld delegate would be silently dropped.
    _window_delegate: Retained<WindowCloseDelegate>,
    pub watcher: RefCell<Option<crate::watcher::FileWatcher>>,
    /// Banners raised by a load, drained once the page is ready to receive them.
    pub pending_banners: RefCell<Vec<(String, String)>>,
    /// Flipped to `true` by `WindowCloseDelegate::windowWillClose` — the only
    /// reliable signal that this window is gone for good. `NSWindow::isVisible`
    /// is also false while merely hidden (⌘H, ⌘M), which is not the same thing.
    closed: Rc<Cell<bool>>,
    /// False while showing the error page (no `#mdview-content` in the DOM),
    /// so `live_update` knows to fall back to a full `reload` instead of
    /// silently discarding a JS swap into a page that has nowhere to put it.
    content_ready: Cell<bool>,
    /// One-shot token: set immediately before this window's own
    /// `loadHTMLString_baseURL` calls, read-and-cleared by the navigation
    /// delegate so only the load the app just initiated is ever allowed
    /// through. Shared with `NavigationDelegate`, mirroring how `closed` is
    /// shared with `WindowCloseDelegate`.
    expecting_own_load: Rc<Cell<bool>>,
}

impl DocumentWindow {
    pub fn open(
        path: &Path,
        mtm: MainThreadMarker,
        highlighter: &Highlighter,
        on_navigate: Rc<dyn Fn(crate::navigation::NavigationRequest)>,
    ) -> Rc<Self> {
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(960.0, 720.0));
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;

        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };

        let config = unsafe { WKWebViewConfiguration::new(mtm) };
        let webview = unsafe {
            WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &config)
        };

        let expecting_own_load = Rc::new(Cell::new(false));
        let navigation = crate::navigation::NavigationDelegate::new(
            mtm,
            on_navigate,
            expecting_own_load.clone(),
        );
        unsafe {
            webview.setNavigationDelegate(Some(ProtocolObject::from_ref(&*navigation)));
        }

        let closed = Rc::new(Cell::new(false));
        let window_delegate = WindowCloseDelegate::new(mtm, closed.clone());
        unsafe {
            window.setDelegate(Some(ProtocolObject::from_ref(&*window_delegate)));

            window.setContentView(Some(&webview));
            window.setReleasedWhenClosed(false);
            // Paint the window in the current appearance before the page's own
            // CSS applies. Without this, every load flashes white in dark mode.
            window.setBackgroundColor(Some(&NSColor::textBackgroundColor()));
            // The WKWebView is the opaque content view painted on top of the
            // window, so it is the web view's own background — not the
            // window's — that is visible during the load. Set both: this one
            // is what actually prevents the white flash in dark mode.
            //
            // `setUnderPageBackgroundColor:` is macOS 12+, but the bundle
            // supports macOS 11. objc2 encodes no availability information,
            // so ask the receiver directly — on Big Sur this selector does
            // not exist and calling it unguarded aborts the process.
            if webview.respondsToSelector(objc2::sel!(setUnderPageBackgroundColor:)) {
                webview.setUnderPageBackgroundColor(Some(&NSColor::textBackgroundColor()));
            }
        }
        window.center();

        let doc_window = Rc::new(DocumentWindow {
            window,
            webview,
            path: RefCell::new(path.to_path_buf()),
            _navigation: navigation,
            _window_delegate: window_delegate,
            watcher: RefCell::new(crate::watcher::FileWatcher::start(path).ok()),
            pending_banners: RefCell::new(Vec::new()),
            closed,
            content_ready: Cell::new(false),
            expecting_own_load,
        });

        doc_window.reload(highlighter);
        doc_window.window.makeKeyAndOrderFront(None);
        doc_window
    }

    /// True once `windowWillClose:` has fired for this window — the only
    /// reliable "this window is gone" signal. Used to prune `AppState.windows`
    /// without also dropping windows that are merely hidden or miniaturized.
    pub fn is_closed(&self) -> bool {
        self.closed.get()
    }

    /// Re-render from disk and replace the whole page. There is also an
    /// incremental path (`live_update`) that preserves scroll position; this
    /// full load is what runs on first open, on explicit File > Reload, and
    /// as the recovery path when live reload lands on a window that is
    /// currently showing the error page.
    pub fn reload(&self, highlighter: &Highlighter) {
        let path = self.path.borrow().clone();

        // Clear unconditionally, before we know whether this load succeeds:
        // a stale queue from a previous load must never survive onto either
        // a fresh success page or the error page, where `show_banner`'s
        // `if (!host) return;` would silently drop it for good.
        *self.pending_banners.borrow_mut() = Vec::new();

        let title = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "MDView".to_string());
        self.window.setTitle(&NSString::from_str(&title));

        match mdcore::render_document_with(&path, highlighter) {
            Ok(doc) => {
                let base = NSURL::fileURLWithPath(&NSString::from_str(&doc.base_dir.to_string_lossy()));
                self.expecting_own_load.set(true);
                unsafe {
                    self.webview.loadHTMLString_baseURL(
                        &NSString::from_str(&doc.html),
                        Some(&base),
                    );
                }
                self.content_ready.set(true);
                // Banners cannot be injected until the page has loaded; the
                // watch tick raises anything pending on its next pass.
                if doc.lossy {
                    self.pending_banners.borrow_mut().push((
                        "lossy".to_string(),
                        "This file is not valid UTF-8. Some characters were replaced."
                            .to_string(),
                    ));
                }
                if self.watcher.borrow().is_none() {
                    self.pending_banners.borrow_mut().push((
                        "watch".to_string(),
                        "Live reload is unavailable for this file. Press ⌘R to refresh."
                            .to_string(),
                    ));
                }
            }
            Err(err) => self.show_error(&err.to_string()),
        }
    }

    /// Point this window at a different document: swap the path, rebuild the
    /// watcher for the new parent directory, and re-render.
    ///
    /// The old watcher MUST be dropped before the new one starts. A stale
    /// watcher keeps firing live updates for the previous document's
    /// directory, which would re-render this window from the wrong file.
    pub fn load(&self, path: &Path, highlighter: &Highlighter) {
        *self.path.borrow_mut() = path.to_path_buf();
        *self.watcher.borrow_mut() = None;
        *self.watcher.borrow_mut() = crate::watcher::FileWatcher::start(path).ok();
        self.pending_banners.borrow_mut().clear();
        self.reload(highlighter);
        self.clear_banner("missing");
        self.clear_banner("lossy");
    }

    /// Replace the window contents with a readable error page. Used when the
    /// file cannot be read at all; never leaves the window blank.
    pub fn show_error(&self, message: &str) {
        let html = format!(
            "<!DOCTYPE html><html><head><meta charset=\"utf-8\">\
<style>body{{font:15px -apple-system,sans-serif;padding:3rem;color:#b3261e}}\
@media(prefers-color-scheme:dark){{body{{background:#0d1117;color:#ff8a80}}}}</style>\
</head><body><h2>Cannot display this file</h2><p>{}</p></body></html>",
            mdcore::escape::escape_html(message)
        );
        self.expecting_own_load.set(true);
        unsafe {
            self.webview
                .loadHTMLString_baseURL(&NSString::from_str(&html), None);
        }
        // The error page has no `#mdview-content` or `#mdview-banners`: a
        // later `live_update`'s JS swap would silently no-op forever, and a
        // queued banner would have nowhere to land. Mark content not-ready so
        // the next successful poll does a full `reload` instead.
        self.content_ready.set(false);
    }

    /// Re-render and swap the body in place, preserving scroll position.
    /// Falls back to a full reload if the document cannot be read, or if the
    /// window is currently showing the error page (no `#mdview-content` to
    /// swap into).
    pub fn live_update(&self, highlighter: &Highlighter) {
        if !self.content_ready.get() {
            self.reload(highlighter);
            return;
        }

        let path = self.path.borrow().clone();

        let (body, lossy) = match mdcore::render_body_of(&path, highlighter) {
            Ok(result) => result,
            Err(err) => {
                // Keep the last good render on screen and say why it is stale.
                self.show_banner("missing", &format!("Showing the last version: {err}"));
                return;
            }
        };

        self.clear_banner("missing");
        if lossy {
            self.show_banner(
                "lossy",
                "This file is not valid UTF-8. Some characters were replaced.",
            );
        } else {
            self.clear_banner("lossy");
        }

        let script = format!(
            "(function() {{ \
               var y = window.scrollY; \
               var target = document.getElementById('mdview-content'); \
               if (!target) return; \
               target.innerHTML = {body}; \
               window.scrollTo(0, y); \
               if (window.mdviewRenderAll) window.mdviewRenderAll(); \
             }})();",
            body = mdcore::escape::js_string_literal(&body)
        );
        self.eval(&script);
    }

    /// Show (or replace) a banner. `id` identifies the condition, so the code
    /// that resolves it can clear exactly its own banner.
    pub fn show_banner(&self, id: &str, message: &str) {
        let script = format!(
            "(function() {{ \
               var host = document.getElementById('mdview-banners'); \
               if (!host) return; \
               var id = {id}; \
               var existing = document.getElementById(id); \
               if (existing) existing.remove(); \
               var el = document.createElement('div'); \
               el.id = id; \
               el.className = 'mdview-banner'; \
               el.textContent = {message}; \
               el.title = 'Click to dismiss'; \
               el.addEventListener('click', function() {{ el.remove(); }}); \
               host.appendChild(el); \
             }})();",
            id = mdcore::escape::js_string_literal(&format!("mdview-banner-{id}")),
            message = mdcore::escape::js_string_literal(message),
        );
        self.eval(&script);
    }

    pub fn clear_banner(&self, id: &str) {
        let script = format!(
            "(function() {{ var el = document.getElementById({id}); if (el) el.remove(); }})();",
            id = mdcore::escape::js_string_literal(&format!("mdview-banner-{id}")),
        );
        self.eval(&script);
    }

    /// Inject any banners queued by the last load. Safe to call repeatedly.
    /// Does nothing while the page is still loading — `loadHTMLString` is
    /// asynchronous, and draining early would empty the queue into a document
    /// that does not exist yet, losing the banner permanently.
    pub fn drain_pending_banners(&self) {
        if unsafe { self.webview.isLoading() } {
            return;
        }
        let pending = std::mem::take(&mut *self.pending_banners.borrow_mut());
        for (id, message) in pending {
            self.show_banner(&id, &message);
        }
    }

    fn eval(&self, script: &str) {
        unsafe {
            self.webview
                .evaluateJavaScript_completionHandler(&NSString::from_str(script), None);
        }
    }
}
