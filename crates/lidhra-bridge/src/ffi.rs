//! C ABI for native hosts (the Apple TV app). See `include/lidhra_bridge.h`.
//!
//! One process-wide [`Bridge`] and a small tokio runtime, both created on first
//! use. Calls are asynchronous: the result arrives through the callback, on a
//! runtime worker thread, as a NUL-terminated UTF-8 JSON string that is only
//! valid for the duration of the callback (copy it).

use crate::Bridge;
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::{Arc, OnceLock};

/// `ok` = true: `json` is the command's result. `ok` = false: `json` is `{"error": "..."}`.
pub type LidhraCallback = extern "C" fn(ctx: *mut c_void, ok: bool, json: *const c_char);

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
static BRIDGE: OnceLock<Arc<Bridge>> = OnceLock::new();

fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("lidhra-bridge")
            .enable_all()
            .build()
            .expect("lidhra-bridge: tokio runtime")
    })
}

/// The host's opaque context pointer, carried to the worker thread untouched.
struct Ctx(*mut c_void);
// SAFETY: the pointer is never dereferenced on the Rust side; it is only
// handed back to the host's callback, which owns its thread-safety.
unsafe impl Send for Ctx {}

fn deliver(cb: LidhraCallback, ctx: Ctx, ok: bool, json: String) {
    let s = CString::new(json).unwrap_or_else(|_| CString::new("{\"error\":\"result contained NUL\"}").unwrap());
    cb(ctx.0, ok, s.as_ptr());
}

fn error_json(msg: &str) -> String {
    serde_json::json!({ "error": msg }).to_string()
}

/// Run `name` with `args_json` (a JSON object, or NULL for none) and report to `cb`.
///
/// # Safety
/// `name` must be a valid NUL-terminated string; `args_json` must be NULL or a
/// valid NUL-terminated string. Both are copied before this returns.
#[no_mangle]
pub unsafe extern "C" fn lidhra_bridge_call(
    name: *const c_char,
    args_json: *const c_char,
    ctx: *mut c_void,
    cb: LidhraCallback,
) {
    let ctx = Ctx(ctx);
    if name.is_null() {
        return deliver(cb, ctx, false, error_json("missing command name"));
    }
    let name = CStr::from_ptr(name).to_string_lossy().into_owned();
    let args = if args_json.is_null() {
        serde_json::Value::Null
    } else {
        match serde_json::from_str(&CStr::from_ptr(args_json).to_string_lossy()) {
            Ok(v) => v,
            Err(e) => return deliver(cb, ctx, false, error_json(&format!("bad args: {e}"))),
        }
    };
    let bridge = BRIDGE.get_or_init(|| Arc::new(Bridge::new())).clone();
    runtime().spawn(async move {
        match bridge.call(&name, &args).await {
            Ok(v) => deliver(cb, ctx, true, v.to_string()),
            Err(e) => deliver(cb, ctx, false, error_json(&e)),
        }
    });
}

/// Static version string, e.g. `"0.1.0"`. Never free it.
#[no_mangle]
pub extern "C" fn lidhra_bridge_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    extern "C" fn collect(ctx: *mut c_void, ok: bool, json: *const c_char) {
        let tx = unsafe { &*(ctx as *const mpsc::Sender<(bool, String)>) };
        let s = unsafe { CStr::from_ptr(json) }.to_string_lossy().into_owned();
        tx.send((ok, s)).unwrap();
    }

    #[test]
    fn calls_through_the_c_abi() {
        let (tx, rx) = mpsc::channel::<(bool, String)>();
        let ctx = &tx as *const _ as *mut c_void;
        let name = CString::new("providers").unwrap();
        unsafe { lidhra_bridge_call(name.as_ptr(), std::ptr::null(), ctx, collect) };
        let (ok, json) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert!(ok);
        assert!(json.contains("Real-Debrid"));

        let bad = CString::new("nope").unwrap();
        unsafe { lidhra_bridge_call(bad.as_ptr(), std::ptr::null(), ctx, collect) };
        let (ok, json) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert!(!ok);
        assert!(json.contains("unknown command"));

        let args = CString::new("{bad").unwrap();
        unsafe { lidhra_bridge_call(name.as_ptr(), args.as_ptr(), ctx, collect) };
        let (ok, json) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert!(!ok && json.contains("bad args"));

        let v = unsafe { CStr::from_ptr(lidhra_bridge_version()) };
        assert_eq!(v.to_str().unwrap(), env!("CARGO_PKG_VERSION"));
    }
}
