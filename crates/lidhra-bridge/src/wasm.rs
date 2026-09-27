//! wasm-bindgen front-end for the packaged web TVs (Tizen, webOS, HarmonyOS).
//!
//! ```js
//! const bridge = new wasm_bindgen.LidhraBridge();
//! const json = await bridge.call("providers", "{}");   // resolves to a JSON string
//! ```
//! A rejected promise carries the error message as a string.

use crate::Bridge;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct LidhraBridge {
    inner: Rc<Bridge>,
}

#[wasm_bindgen]
impl LidhraBridge {
    #[wasm_bindgen(constructor)]
    #[allow(clippy::new_without_default)]
    pub fn new() -> LidhraBridge {
        LidhraBridge { inner: Rc::new(Bridge::new()) }
    }

    /// Run `name` with `args_json`; resolves to the result as a JSON string.
    pub fn call(&self, name: String, args_json: String) -> js_sys::Promise {
        let bridge = self.inner.clone();
        wasm_bindgen_futures::future_to_promise(async move {
            let args = if args_json.trim().is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::from_str(&args_json).map_err(|e| JsValue::from_str(&format!("bad args: {e}")))?
            };
            bridge.call(&name, &args).await.map(|v| JsValue::from_str(&v.to_string())).map_err(|e| JsValue::from_str(&e))
        })
    }

    pub fn version() -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }
}
