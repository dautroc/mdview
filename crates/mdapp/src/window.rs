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
}
