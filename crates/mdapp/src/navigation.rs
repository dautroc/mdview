use std::path::PathBuf;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{define_class, DefinedClass, MainThreadOnly};
use objc2_foundation::MainThreadMarker;
use objc2_web_kit::{
    WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate, WKWebView,
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

pub struct NavigationState {
    pub handler: Rc<dyn Fn(NavigationRequest)>,
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

            if let (Some(absolute), Some(scheme)) = (absolute, scheme) {
                // `loadHTMLString` itself arrives here as an about:blank
                // navigation. Letting it through is what actually displays the
                // document; everything else is cancelled.
                if scheme == "about" {
                    (*handler).call((WKNavigationActionPolicy::Allow,));
                    return;
                }
                if let Some(request) = classify(&absolute, &scheme, decoded_path.as_deref()) {
                    (self.ivars().handler)(request);
                }
            }

            (*handler).call((WKNavigationActionPolicy::Cancel,));
        }
    }
);

impl NavigationDelegate {
    pub fn new(
        mtm: MainThreadMarker,
        handler: Rc<dyn Fn(NavigationRequest)>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(NavigationState { handler });
        unsafe { objc2::msg_send![super(this), init] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
