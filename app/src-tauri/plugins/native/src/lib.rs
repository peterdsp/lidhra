//! Lidhra native bridge.
//!
//! On iOS this drives a tiny Swift plugin (`ios/Sources/NativePlugin.swift`):
//! the system share sheet (which includes "Save to Files"), the "Open in…"
//! menu for handing a file to another app (VLC, Infuse, …), the native
//! AVPlayer for playback, and a `canOpenURL` probe for URL schemes.
//!
//! Everywhere else the calls report "unsupported" and the app falls back to
//! the desktop paths (opener plugin + in-app HTML player).

use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_lidhra_native);

pub struct Native<R: Runtime> {
    #[cfg(target_os = "ios")]
    handle: tauri::plugin::PluginHandle<R>,
    _r: std::marker::PhantomData<fn() -> R>,
}

impl<R: Runtime> Native<R> {
    #[cfg(target_os = "ios")]
    fn call(&self, cmd: &str, payload: serde_json::Value) -> Result<serde_json::Value, String> {
        self.handle
            .run_mobile_plugin::<serde_json::Value>(cmd, payload)
            .map_err(|e| e.to_string())
    }

    #[cfg(not(target_os = "ios"))]
    fn call(&self, _cmd: &str, _payload: serde_json::Value) -> Result<serde_json::Value, String> {
        Err("only available on iOS".into())
    }

    /// System share sheet for a local file (AirDrop, Messages, Save to Files, …).
    /// `anchor` is the initiating control's box in the webview's CSS-pixel space
    /// (`{x,y,w,h}`), so the popover points at it on iPad / large layouts; a
    /// missing anchor falls back to the bottom-centre of the window.
    pub fn share(&self, path: &str, anchor: Option<serde_json::Value>) -> Result<(), String> {
        let mut payload = serde_json::json!({ "path": path });
        if let Some(a) = anchor {
            payload["anchor"] = a;
        }
        self.call("share", payload).map(|_| ())
    }

    /// "Open in…" menu: hand a local file to another installed app.
    pub fn open_in(&self, path: &str, anchor: Option<serde_json::Value>) -> Result<(), String> {
        let mut payload = serde_json::json!({ "path": path });
        if let Some(a) = anchor {
            payload["anchor"] = a;
        }
        self.call("openIn", payload).map(|_| ())
    }

    /// Native full-screen player for a local path or an http(s) URL.
    pub fn play(&self, url: &str, title: Option<&str>) -> Result<(), String> {
        self.call("play", serde_json::json!({ "url": url, "title": title })).map(|_| ())
    }

    /// Whether some installed app handles this URL scheme (e.g. `vlc-x-callback://`).
    pub fn can_open(&self, url: &str) -> Result<bool, String> {
        match self.call("canOpen", serde_json::json!({ "url": url })) {
            Ok(v) => Ok(v.get("ok").and_then(|b| b.as_bool()).unwrap_or(false)),
            Err(_) => Ok(false),
        }
    }
}

pub trait NativeExt<R: Runtime> {
    fn native(&self) -> &Native<R>;
}

impl<R: Runtime, T: Manager<R>> NativeExt<R> for T {
    fn native(&self) -> &Native<R> {
        self.state::<Native<R>>().inner()
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("lidhra-native")
        .setup(|app, _api| {
            #[cfg(target_os = "ios")]
            let handle = _api.register_ios_plugin(init_plugin_lidhra_native)?;
            app.manage(Native::<R> {
                #[cfg(target_os = "ios")]
                handle,
                _r: std::marker::PhantomData,
            });
            Ok(())
        })
        .build()
}
