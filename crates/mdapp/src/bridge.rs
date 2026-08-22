//! The page-to-Rust bridge. The page posts plain `kind:payload` strings to
//! `window.webkit.messageHandlers.mdview`; parsing lives in `state.rs` so the
//! wire format is unit-tested without a web view.

use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{define_class, DefinedClass, MainThreadOnly};
use objc2_foundation::{MainThreadMarker, NSString};
use objc2_web_kit::{WKScriptMessage, WKScriptMessageHandler, WKUserContentController};

use crate::state::{parse_message, Message};

pub struct BridgeState {
    pub handler: Rc<dyn Fn(Message)>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MDViewBridge"]
    #[ivars = BridgeState]
    pub struct Bridge;

    unsafe impl NSObjectProtocol for Bridge {}

    unsafe impl WKScriptMessageHandler for Bridge {
        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        fn did_receive(
            &self,
            _controller: &WKUserContentController,
            message: &WKScriptMessage,
        ) {
            let body = unsafe { message.body() };
            // The page only ever posts strings. Anything else is malformed
            // and is dropped rather than treated as an error.
            let Ok(text) = body.downcast::<NSString>() else {
                return;
            };
            if let Some(parsed) = parse_message(&text.to_string()) {
                (self.ivars().handler)(parsed);
            }
        }
    }
);

impl Bridge {
    pub fn new(mtm: MainThreadMarker, handler: Rc<dyn Fn(Message)>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(BridgeState { handler });
        unsafe { objc2::msg_send![super(this), init] }
    }
}
