//! App-scoped trust for the response-capture proxy; www.reddit.com only.
//! Adapted from Muninn (MIT), see licenses/MUNINN-MIT.txt. No keychain installer.

use objc2::msg_send;
use objc2::runtime::AnyObject;
use reddit_offline::ca::Ca;
use security_framework::certificate::SecCertificate;
use security_framework::trust::SecTrust;
use std::ffi::{c_char, c_void, CStr};
use std::sync::OnceLock;
use tauri::WebviewWindow;

static CA_DER: OnceLock<Vec<u8>> = OnceLock::new();

extern "C" {
    fn object_getClass(obj: *const c_void) -> *const c_void;
    fn sel_registerName(name: *const c_char) -> *const c_void;
    fn class_addMethod(
        cls: *const c_void,
        name: *const c_void,
        imp: *const c_void,
        types: *const c_char,
    ) -> bool;
    fn objc_getClass(name: *const c_char) -> *const c_void;
    fn class_getInstanceMethod(cls: *const c_void, name: *const c_void) -> *const c_void;
    fn method_getImplementation(method: *const c_void) -> *const c_void;
}

type Completion = block2::Block<dyn Fn(isize, *mut AnyObject)>;

fn host_is_reddit(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    h == "www.reddit.com"
}

unsafe fn nsstring(obj: *mut AnyObject) -> Option<String> {
    if obj.is_null() {
        return None;
    }
    let utf8: *const c_char = msg_send![obj, UTF8String];
    (!utf8.is_null()).then(|| CStr::from_ptr(utf8).to_string_lossy().into_owned())
}

/// Validate only against this installation's CA, retaining WebKit's host policy.
unsafe fn chain_ends_in_reader_ca(trust_ptr: *mut c_void) -> Result<(), String> {
    use core_foundation::base::TCFType;
    let der = CA_DER.get().ok_or("Local source CA is unavailable")?;
    let ca = SecCertificate::from_der(der).map_err(|e| format!("Local source CA: {e}"))?;
    let mut trust = SecTrust::wrap_under_get_rule(trust_ptr as _);
    trust
        .set_anchor_certificates(&[ca])
        .map_err(|e| format!("Set source trust anchor: {e}"))?;
    trust
        .set_trust_anchor_certificates_only(true)
        .map_err(|e| format!("Limit source trust anchor: {e}"))?;
    trust
        .evaluate_with_error()
        .map_err(|e| format!("Source certificate evaluation: {e}"))
}

extern "C" fn did_receive_challenge(
    _this: *mut AnyObject,
    _cmd: *const c_void,
    _webview: *mut AnyObject,
    challenge: *mut AnyObject,
    handler: *mut c_void,
) {
    // 0 = use credential, 1 = perform default handling.
    let (mut disposition, mut credential): (isize, *mut AnyObject) = (1, std::ptr::null_mut());
    unsafe {
        let space: *mut AnyObject = msg_send![challenge, protectionSpace];
        if !space.is_null() {
            let method = nsstring(msg_send![space, authenticationMethod]);
            let host = nsstring(msg_send![space, host]);
            let trust: *mut c_void = msg_send![space, serverTrust];
            let server_trust = method.as_deref() == Some("NSURLAuthenticationMethodServerTrust");
            let allowed_host = host.as_deref().is_some_and(host_is_reddit);
            let validation = if server_trust && allowed_host && !trust.is_null() {
                chain_ends_in_reader_ca(trust)
            } else {
                Err("Outside app-scoped source trust".into())
            };
            if validation.is_ok() {
                let class = objc_getClass(c"NSURLCredential".as_ptr()) as *const AnyObject;
                if !class.is_null() {
                    let cred: *mut AnyObject = msg_send![class, credentialForTrust: trust];
                    if !cred.is_null() {
                        disposition = 0;
                        credential = cred;
                    }
                }
            }
            reddit_offline::diagnostics::event(
                "macos.trust_challenge",
                serde_json::json!({"server_trust":server_trust,"allowed_host":allowed_host,"has_trust":!trust.is_null(),"accepted":disposition==0,"error":validation.err()}),
            );
        }
        if !handler.is_null() {
            let block = &*(handler as *const Completion);
            block.call((disposition, credential));
        }
    }
}

/// Adds the challenge method to the web view's navigation delegate class.
pub fn prepare(
    ca: &Ca,
    wv: &WebviewWindow,
    failed: impl FnOnce(String) + Send + 'static,
) -> Result<(), String> {
    if CA_DER
        .get()
        .is_some_and(|existing| existing.as_slice() != ca.cert_der())
    {
        return Err("Capture certificate changed during this app session".into());
    }
    let _ = CA_DER.set(ca.cert_der().to_vec());
    wv.with_webview(move |w| unsafe {
        let webview = w.inner() as *mut AnyObject;
        let delegate: *mut AnyObject = msg_send![webview, navigationDelegate];
        if delegate.is_null() {
            failed("Native source trust delegate is unavailable".into());
            return;
        }
        let class = object_getClass(delegate as *const c_void);
        let sel = sel_registerName(
            c"webView:didReceiveAuthenticationChallenge:completionHandler:".as_ptr(),
        );
        // v@:@@@? = void, self, _cmd, webview, challenge, block
        let installed = class_addMethod(
            class,
            sel,
            did_receive_challenge as *const c_void,
            c"v@:@@@?".as_ptr(),
        );
        if !installed {
            let method = class_getInstanceMethod(class, sel);
            if method.is_null()
                || method_getImplementation(method) != did_receive_challenge as *const c_void
            {
                failed("Could not install app-scoped source certificate trust".into());
                return;
            }
        }
        // WebKit snapshots respondsToSelector when the delegate is assigned.
        // Wry assigned it before our dynamic method existed; adding the method
        // alone leaves the first webview's challenge callback disabled. Retain
        // and reassign the SAME delegate to refresh WebKit's capability flags.
        // https://github.com/WebKit/WebKit/blob/main/Source/WebKit/UIProcess/Cocoa/NavigationState.mm
        let Some(delegate) = objc2::rc::Retained::<AnyObject>::retain(delegate) else {
            failed("Could not retain source navigation delegate".into());
            return;
        };
        let nil: *mut AnyObject = std::ptr::null_mut();
        let _: () = msg_send![webview,setNavigationDelegate:nil];
        let _: () = msg_send![webview,setNavigationDelegate:&*delegate];
        reddit_offline::diagnostics::event(
            "macos.trust_hook",
            serde_json::json!({"installed":true,"delegate_rebound":true,"scope":"www.reddit.com"}),
        );
    })
    .map_err(|error| error.to_string())
}
