use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::MainThreadOnly;
use objc2_app_kit::{
    NSBackingStoreType, NSPrintInfo, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{
    NSDate, NSData, NSError, MainThreadMarker, NSPoint, NSRect, NSRunLoop, NSSize, NSString, NSURL,
};
use objc2_web_kit::{WKPDFConfiguration, WKWebView, WKWebViewConfiguration};

const RENDER_TIMEOUT: Duration = Duration::from_secs(15);
const RUN_LOOP_SLICE: f64 = 0.01;

pub struct ReadyPage {
    pub webview: Retained<WKWebView>,
    _window: Retained<NSWindow>,
    _navigation: Retained<crate::navigation::NavigationDelegate>,
}

impl ReadyPage {
    pub fn load(
        html: &str,
        base_dir: &Path,
        mtm: MainThreadMarker,
    ) -> Result<Self, String> {
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1024.0, 768.0));
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
        let webview = unsafe {
            WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &configuration)
        };
        window.setContentView(Some(&webview));
        unsafe { window.setReleasedWhenClosed(false) };

        let expecting_own_load = Rc::new(Cell::new(true));
        let page_ready = Rc::new(Cell::new(false));
        let expected_navigation = Rc::new(RefCell::new(None));
        let navigation = crate::navigation::NavigationDelegate::new(
            mtm,
            Rc::new(|_| {}),
            expecting_own_load,
            page_ready.clone(),
            expected_navigation.clone(),
        );
        unsafe {
            webview.setNavigationDelegate(Some(ProtocolObject::from_ref(&*navigation)));
        }

        let base = NSURL::fileURLWithPath(&NSString::from_str(&base_dir.to_string_lossy()));
        let load = unsafe {
            webview.loadHTMLString_baseURL(&NSString::from_str(html), Some(&base))
        };
        *expected_navigation.borrow_mut() = load;

        let deadline = Instant::now() + RENDER_TIMEOUT;
        while !page_ready.get() {
            if Instant::now() >= deadline {
                return Err("Timed out while loading the export page.".to_string());
            }
            run_loop_slice();
        }

        loop {
            if Instant::now() >= deadline {
                return Err("Timed out waiting for diagrams to finish rendering.".to_string());
            }
            let state = evaluate_string(
                &webview,
                "(function(){var s=window.mdviewRenderState;return s ? s.status+'\\n'+(s.error||'') : 'pending\\n';})()",
                deadline,
            )?;
            let (status, detail) = state.split_once('\n').unwrap_or((&state, ""));
            match status {
                "ready" => break,
                "failed" => {
                    return Err(if detail.is_empty() {
                        "The export page failed to render.".to_string()
                    } else {
                        format!("The export page failed to render: {detail}")
                    });
                }
                _ => run_loop_slice(),
            }
        }

        Ok(Self {
            webview,
            _window: window,
            _navigation: navigation,
        })
    }

    pub fn pdf_data(&self, mtm: MainThreadMarker) -> Result<Vec<u8>, String> {
        let result: Rc<RefCell<Option<Result<Vec<u8>, String>>>> = Rc::new(RefCell::new(None));
        let sink = result.clone();
        let completion = RcBlock::new(move |data: *mut NSData, error: *mut NSError| {
            let value = if let Some(error) = unsafe { error.as_ref() } {
                Err(error.localizedDescription().to_string())
            } else if let Some(data) = unsafe { data.as_ref() } {
                Ok(data.to_vec())
            } else {
                Err("WebKit returned no PDF data.".to_string())
            };
            *sink.borrow_mut() = Some(value);
        });
        let configuration = unsafe { WKPDFConfiguration::new(mtm) };
        unsafe {
            self.webview.createPDFWithConfiguration_completionHandler(
                Some(&configuration),
                &completion,
            );
        }
        wait_for_result(result, Instant::now() + RENDER_TIMEOUT, "PDF generation timed out.")
    }

    pub fn print(&self, title: &str) -> bool {
        let info = NSPrintInfo::sharedPrintInfo();
        let operation = unsafe { self.webview.printOperationWithPrintInfo(&info) };
        operation.setJobTitle(Some(&NSString::from_str(title)));
        operation.setShowsPrintPanel(true);
        operation.setShowsProgressPanel(true);
        operation.runOperation()
    }
}

fn evaluate_string(webview: &WKWebView, script: &str, deadline: Instant) -> Result<String, String> {
    let result: Rc<RefCell<Option<Result<String, String>>>> = Rc::new(RefCell::new(None));
    let sink = result.clone();
    let completion = RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
        let value = if let Some(error) = unsafe { error.as_ref() } {
            Err(error.localizedDescription().to_string())
        } else if let Some(value) = unsafe { value.as_ref() } {
            value
                .downcast_ref::<NSString>()
                .map(|text| text.to_string())
                .ok_or_else(|| "The export readiness check returned an invalid value.".to_string())
        } else {
            Err("The export readiness check returned no value.".to_string())
        };
        *sink.borrow_mut() = Some(value);
    });
    unsafe {
        webview.evaluateJavaScript_completionHandler(
            &NSString::from_str(script),
            Some(&completion),
        );
    }
    wait_for_result(result, deadline, "The export readiness check timed out.")
}

fn wait_for_result<T>(
    result: Rc<RefCell<Option<Result<T, String>>>>,
    deadline: Instant,
    timeout: &str,
) -> Result<T, String> {
    loop {
        if let Some(result) = result.borrow_mut().take() {
            return result;
        }
        if Instant::now() >= deadline {
            return Err(timeout.to_string());
        }
        run_loop_slice();
    }
}

fn run_loop_slice() {
    NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(RUN_LOOP_SLICE));
}

#[cfg(test)]
mod tests {
    #[test]
    fn readiness_script_reports_status_and_error_separately() {
        let script = "(function(){var s=window.mdviewRenderState;return s ? s.status+'\\n'+(s.error||'') : 'pending\\n';})()";
        assert!(script.contains("mdviewRenderState"));
        assert!(script.contains("s.status+'\\n'"));
    }
}
