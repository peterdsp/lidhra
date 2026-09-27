/* Loaded first by every packaged web-TV build (Samsung Tizen, LG webOS, HarmonyOS,
   the hosted TV page). It turns TV mode on and starts the wasm bridge
   (crates/lidhra-bridge); api() in index.html awaits window.LIDHRA_WEB_BRIDGE.
   The wasm bytes are inlined as base64 (bridge/lidhra_bridge_wasm.js) because
   packaged TV runtimes load pages from file:// or app://, where fetch() of a
   local .wasm file is often refused. Plain ES5 on purpose: old TV engines. */
(function () {
  window.LIDHRA_TV = true;
  function bytes(b64) {
    var bin = atob(b64), out = new Uint8Array(bin.length);
    for (var i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  }
  window.LIDHRA_WEB_BRIDGE = (function () {
    if (typeof WebAssembly !== 'object' || typeof wasm_bindgen !== 'function' || !window.LIDHRA_BRIDGE_WASM) {
      return Promise.reject(new Error('This TV cannot run Lidhra (WebAssembly is missing).'));
    }
    return wasm_bindgen({ module_or_path: bytes(window.LIDHRA_BRIDGE_WASM) }).then(function () {
      window.LIDHRA_BRIDGE_WASM = null; // free the base64 copy
      return new wasm_bindgen.LidhraBridge();
    });
  })();
  // Keep an unhandled rejection from surfacing before api() attaches its handler.
  window.LIDHRA_WEB_BRIDGE.catch(function () {});
})();
