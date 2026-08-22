use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{define_class, DefinedClass, MainThreadOnly};
use objc2_foundation::MainThreadMarker;
use objc2_web_kit::{
    WKNavigation, WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate,
    WKNavigationType, WKWebView,
};

const MARKDOWN_EXTENSIONS: [&str; 3] = ["md", "markdown", "mdown"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigationRequest {
    /// Hand this URL to the user's default browser.
    OpenExternal(String),
    /// Open this local Markdown file in a new MDView window.
    OpenDocument(PathBuf),
}

/// Decide what a navigation attempt means. Pure logic, no AppKit, so it is
/// unit-tested directly.
pub fn classify(url: &str, scheme: &str, file_path: Option<&str>) -> Option<NavigationRequest> {
    match scheme {
        "http" | "https" | "mailto" => Some(NavigationRequest::OpenExternal(url.to_string())),
        "file" => {
            let path = file_path?;
            let path_buf = PathBuf::from(path);
            let is_markdown = path_buf
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| {
                    MARKDOWN_EXTENSIONS
                        .iter()
                        .any(|known| ext.eq_ignore_ascii_case(known))
                })
                .unwrap_or(false);
            is_markdown.then_some(NavigationRequest::OpenDocument(path_buf))
        }
        // Anything else — javascript:, data:, custom schemes — is dropped.
        _ => None,
    }
}

/// What the navigation delegate should do with one navigation attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Let the web view proceed. Only ever our own programmatic load.
    Allow,
    /// Block the navigation and do nothing else.
    Cancel,
    /// Block the navigation and hand this request to the app.
    CancelAndHandle(NavigationRequest),
}

/// Decide what to do with a navigation attempt. Pure logic, no AppKit.
///
/// `expecting_own_load` is a one-shot token set immediately before the app
/// calls `loadHTMLString`; `is_other` is true when the navigation type is
/// `WKNavigationType::Other`, which is what programmatic loads report and
/// what user link clicks (`LinkActivated`) never do.
pub fn decide(
    absolute: Option<&str>,
    scheme: Option<&str>,
    file_path: Option<&str>,
    is_other: bool,
    expecting_own_load: bool,
) -> Decision {
    // Our own loadHTMLString. Its URL is whatever baseURL we passed — a
    // file:// directory for a document, about:blank for the error page — so
    // the URL itself is not a reliable signal. The one-shot token plus the
    // navigation type is.
    if expecting_own_load && is_other {
        return Decision::Allow;
    }
    match (absolute, scheme) {
        (Some(absolute), Some(scheme)) => match classify(absolute, scheme, file_path) {
            Some(request) => Decision::CancelAndHandle(request),
            None => Decision::Cancel,
        },
        _ => Decision::Cancel,
    }
}

pub struct NavigationState {
    pub handler: Rc<dyn Fn(NavigationRequest)>,
    pub expecting_own_load: Rc<Cell<bool>>,
    pub page_ready: Rc<Cell<bool>>,
    pub expected_navigation: Rc<RefCell<Option<Retained<WKNavigation>>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDViewNavigationDelegate"]
    #[ivars = NavigationState]
    pub struct NavigationDelegate;

    unsafe impl NSObjectProtocol for NavigationDelegate {}

    unsafe impl WKNavigationDelegate for NavigationDelegate {
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn decide_policy(
            &self,
            _webview: &WKWebView,
            action: &WKNavigationAction,
            handler: &block2::Block<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            let request = unsafe { action.request() };
            let url = request.URL();

            let (absolute, scheme, decoded_path) = match url {
                Some(url) => (
                    url.absoluteString().map(|s| s.to_string()),
                    url.scheme().map(|s| s.to_string()),
                    url.path().map(|p| p.to_string()),
                ),
                None => (None, None, None),
            };

            let is_other = unsafe { action.navigationType() } == WKNavigationType::Other;
            // One-shot: read-and-clear so only the load the app just
            // initiated is ever treated as our own.
            let expecting_own_load = self.ivars().expecting_own_load.replace(false);

            let decision = decide(
                absolute.as_deref(),
                scheme.as_deref(),
                decoded_path.as_deref(),
                is_other,
                expecting_own_load,
            );

            match decision {
                Decision::Allow => (*handler).call((WKNavigationActionPolicy::Allow,)),
                Decision::Cancel => (*handler).call((WKNavigationActionPolicy::Cancel,)),
                Decision::CancelAndHandle(req) => {
                    (self.ivars().handler)(req);
                    (*handler).call((WKNavigationActionPolicy::Cancel,));
                }
            }
        }

        #[unsafe(method(webView:didFinishNavigation:))]
        fn did_finish_navigation(&self, _webview: &WKWebView, navigation: Option<&WKNavigation>) {
            let Some(navigation) = navigation else {
                return;
            };
            let expected_navigation = self.ivars().expected_navigation.borrow();
            let is_expected = expected_navigation.as_ref().is_some_and(|expected| {
                std::ptr::eq(Retained::as_ptr(expected), navigation as *const WKNavigation)
            });
            if is_expected {
                self.ivars().page_ready.set(true);
            }
        }
    }
);

impl NavigationDelegate {
    pub fn new(
        mtm: MainThreadMarker,
        handler: Rc<dyn Fn(NavigationRequest)>,
        expecting_own_load: Rc<Cell<bool>>,
        page_ready: Rc<Cell<bool>>,
        expected_navigation: Rc<RefCell<Option<Retained<WKNavigation>>>>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(NavigationState {
            handler,
            expecting_own_load,
            page_ready,
            expected_navigation,
        });
        unsafe { objc2::msg_send![super(this), init] }
    }
}

#[cfg(test)]
#[derive(Default)]
struct ReadinessState {
    active: Option<u64>,
    ready: bool,
}

#[cfg(test)]
impl ReadinessState {
    fn start(&mut self, navigation: u64) {
        self.active = Some(navigation);
        self.ready = false;
    }

    fn finish(&mut self, navigation: u64) {
        if self.active == Some(navigation) {
            self.ready = true;
        }
    }

    fn is_ready(&self) -> bool {
        self.ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn our_own_document_load_is_allowed() {
        // The exact navigation captured from the real app when it blanked:
        // loadHTMLString's baseURL is a file:// directory, not about:blank.
        assert_eq!(
            decide(
                Some("file:///Users/x/notes/"),
                Some("file"),
                Some("/Users/x/notes"),
                true,  // WKNavigationType::Other
                true,  // token set by reload()
            ),
            Decision::Allow
        );
    }

    #[test]
    fn our_own_error_page_load_is_allowed() {
        assert_eq!(
            decide(Some("about:blank"), Some("about"), None, true, true),
            Decision::Allow
        );
    }

    #[test]
    fn only_the_active_navigation_completion_marks_the_page_ready() {
        let mut readiness = ReadinessState::default();
        readiness.start(1);
        readiness.start(2);

        readiness.finish(1);
        assert!(!readiness.is_ready());

        readiness.finish(2);
        assert!(readiness.is_ready());
    }

    #[test]
    fn a_link_click_to_the_base_directory_is_cancelled() {
        // Same URL as the allowed load, but not our load and not `Other`.
        assert_eq!(
            decide(
                Some("file:///Users/x/notes/"),
                Some("file"),
                Some("/Users/x/notes"),
                false,
                false,
            ),
            Decision::Cancel
        );
    }

    #[test]
    fn a_meta_refresh_after_our_load_is_cancelled() {
        // Type `Other`, but the one-shot token was already consumed.
        assert_eq!(
            decide(Some("https://evil.example/"), Some("https"), None, true, false),
            Decision::CancelAndHandle(NavigationRequest::OpenExternal(
                "https://evil.example/".into()
            ))
        );
    }

    #[test]
    fn an_external_link_is_handed_off_not_followed() {
        assert_eq!(
            decide(Some("https://example.com/x"), Some("https"), None, false, false),
            Decision::CancelAndHandle(NavigationRequest::OpenExternal(
                "https://example.com/x".into()
            ))
        );
    }

    #[test]
    fn a_local_markdown_link_opens_a_document() {
        assert_eq!(
            decide(
                Some("file:///Users/x/a.md"),
                Some("file"),
                Some("/Users/x/a.md"),
                false,
                false,
            ),
            Decision::CancelAndHandle(NavigationRequest::OpenDocument("/Users/x/a.md".into()))
        );
    }

    #[test]
    fn a_missing_url_is_cancelled() {
        assert_eq!(decide(None, None, None, false, false), Decision::Cancel);
    }

    #[test]
    fn http_links_open_externally() {
        assert_eq!(
            classify("https://example.com/x", "https", None),
            Some(NavigationRequest::OpenExternal("https://example.com/x".into()))
        );
    }

    #[test]
    fn markdown_files_open_as_documents() {
        assert_eq!(
            classify("file:///Users/x/notes/a.md", "file", Some("/Users/x/notes/a.md")),
            Some(NavigationRequest::OpenDocument(PathBuf::from("/Users/x/notes/a.md")))
        );
    }

    #[test]
    fn markdown_extension_match_is_case_insensitive() {
        assert!(matches!(
            classify("file:///Users/x/A.MARKDOWN", "file", Some("/Users/x/A.MARKDOWN")),
            Some(NavigationRequest::OpenDocument(_))
        ));
    }

    #[test]
    fn non_markdown_files_are_ignored() {
        assert_eq!(classify("file:///Users/x/photo.png", "file", Some("/Users/x/photo.png")), None);
    }

    #[test]
    fn unknown_schemes_are_ignored() {
        assert_eq!(classify("javascript:alert(1)", "javascript", None), None);
    }

    #[test]
    fn markdown_file_with_spaces_opens_as_document() {
        assert_eq!(
            classify(
                "file:///Users/x/My%20Notes.md",
                "file",
                Some("/Users/x/My Notes.md"),
            ),
            Some(NavigationRequest::OpenDocument(PathBuf::from("/Users/x/My Notes.md")))
        );
    }
}
