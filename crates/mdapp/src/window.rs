use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use mdcore::Highlighter;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::MainThreadOnly;
use objc2_app_kit::{NSBackingStoreType, NSColor, NSWindow, NSWindowStyleMask};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString, NSURL};
use objc2_web_kit::{WKWebView, WKWebViewConfiguration};

/// One window showing one document. Owns its web view and, from Task 10, its
/// own file watcher; closing a window tears down only that window's resources.
pub struct DocumentWindow {
    pub window: Retained<NSWindow>,
    pub webview: Retained<WKWebView>,
    pub path: RefCell<PathBuf>,
    /// Held so the delegate outlives the web view; WKWebView keeps only a
    /// weak reference to its navigation delegate.
    _navigation: Retained<crate::navigation::NavigationDelegate>,
    pub watcher: RefCell<Option<crate::watcher::FileWatcher>>,
    /// Banners raised by a load, drained once the page is ready to receive them.
    pub pending_banners: RefCell<Vec<(String, String)>>,
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

        let navigation = crate::navigation::NavigationDelegate::new(mtm, on_navigate);
        unsafe {
            webview.setNavigationDelegate(Some(ProtocolObject::from_ref(&*navigation)));
        }

        unsafe {
            window.setContentView(Some(&webview));
            window.setReleasedWhenClosed(false);
            // Paint the window in the current appearance before the page's own
            // CSS applies. Without this, every load flashes white in dark mode.
            window.setBackgroundColor(Some(&NSColor::textBackgroundColor()));
        }
        window.center();

        let doc_window = Rc::new(DocumentWindow {
            window,
            webview,
            path: RefCell::new(path.to_path_buf()),
            _navigation: navigation,
            watcher: RefCell::new(crate::watcher::FileWatcher::start(path).ok()),
            pending_banners: RefCell::new(Vec::new()),
        });

        doc_window.reload(highlighter);
        doc_window.window.makeKeyAndOrderFront(None);
        doc_window
    }

    /// Re-render from disk and replace the whole page. Task 10 adds an
    /// incremental path that preserves scroll position; this full load is what
    /// runs on first open and on explicit File > Reload.
    pub fn reload(&self, highlighter: &Highlighter) {
        let path = self.path.borrow().clone();

        let title = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "MDView".to_string());
        self.window.setTitle(&NSString::from_str(&title));

        match mdcore::render_document_with(&path, highlighter) {
            Ok(doc) => {
                let base = NSURL::fileURLWithPath(&NSString::from_str(&doc.base_dir.to_string_lossy()));
                unsafe {
                    self.webview.loadHTMLString_baseURL(
                        &NSString::from_str(&doc.html),
                        Some(&base),
                    );
                }
                // Banners cannot be injected until the page has loaded; the
                // watch tick raises anything pending on its next pass.
                *self.pending_banners.borrow_mut() = Vec::new();
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
        unsafe {
            self.webview
                .loadHTMLString_baseURL(&NSString::from_str(&html), None);
        }
    }

    /// Re-render and swap the body in place, preserving scroll position.
    /// Falls back to a full reload if the document cannot be read.
    pub fn live_update(&self, highlighter: &Highlighter) {
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
