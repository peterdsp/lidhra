//! Desktop tray / menu-bar surface.
//!
//! One status item that shows whether Lidhra is downloading, a tooltip with the
//! live totals, and a small menu: status line, show window, open the downloads
//! folder, quit. On macOS the icon is a monochrome template (the brand mark on
//! alpha) and the overall percentage sits next to it while a download runs;
//! Windows and Linux use the coloured app icon. Closing the window hides it to
//! the tray; "Quit" (or Cmd+Q) really exits.
//!
//! The summary is refreshed by a 1s task in `run` (only touching the tray when
//! something changed), so this file never holds the app state lock for long.

use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

const MENU_STATUS: &str = "tray-status";
const MENU_SHOW: &str = "tray-show";
const MENU_FOLDER: &str = "tray-folder";
const MENU_QUIT: &str = "tray-quit";

/// What the tray shows; compared between refreshes so the OS is only poked on change.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub active: usize,
    pub done: usize,
    pub failed: usize,
    /// 0..=1 across every active download, weighted by bytes when totals are known.
    pub progress: f32,
}

impl Summary {
    pub fn status_line(&self) -> String {
        if self.active > 0 {
            let pct = (self.progress * 100.0).round() as u32;
            let plural = if self.active == 1 { "" } else { "s" };
            format!("{} download{plural} · {pct}%", self.active)
        } else if self.done > 0 || self.failed > 0 {
            let mut parts = Vec::new();
            if self.done > 0 {
                parts.push(format!("{} done", self.done));
            }
            if self.failed > 0 {
                parts.push(format!("{} failed", self.failed));
            }
            parts.join(" · ")
        } else {
            "Idle".to_string()
        }
    }

    pub fn tooltip(&self) -> String {
        format!("Lidhra · {}", self.status_line())
    }

    /// Text beside the menu-bar icon (macOS): the percentage while active, nothing when idle.
    pub fn title(&self) -> Option<String> {
        (self.active > 0).then(|| format!("{}%", (self.progress * 100.0).round() as u32))
    }
}

/// Handles kept in managed state so the refresher can update the surface.
pub struct Tray {
    icon: TrayIcon,
    status: MenuItem<tauri::Wry>,
    last: std::sync::Mutex<Summary>,
}

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, MENU_STATUS, Summary::default().status_line(), false, None::<&str>)?;
    let menu = MenuBuilder::new(app)
        .item(&status)
        .separator()
        .text(MENU_SHOW, "Show Lidhra")
        .text(MENU_FOLDER, "Open Downloads Folder")
        .separator()
        .text(MENU_QUIT, "Quit Lidhra")
        .build()?;

    let icon = if cfg!(target_os = "macos") {
        Image::from_bytes(include_bytes!("../icons/tray-template.png"))?
    } else {
        Image::from_bytes(include_bytes!("../icons/32x32.png"))?
    };

    let tray = TrayIconBuilder::with_id("lidhra")
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip(Summary::default().tooltip())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id().as_ref() {
            MENU_SHOW => show_main(app),
            MENU_FOLDER => open_downloads_folder(app),
            MENU_QUIT => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;

    app.manage(Tray { icon: tray, status, last: std::sync::Mutex::new(Summary::default()) });
    Ok(())
}

/// Bring the main window back (it hides on close) and focus it.
pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn open_downloads_folder(app: &AppHandle) {
    use tauri_plugin_opener::OpenerExt;
    let dir = app.state::<crate::AppState>().0.try_lock().ok().and_then(|g| g.out_dir.clone());
    if let Some(dir) = dir {
        let _ = std::fs::create_dir_all(&dir);
        let _ = app.opener().open_path(dir.to_string_lossy().into_owned(), None::<&str>);
    }
}

/// Push a new summary to the OS if it differs from the last one shown.
pub fn refresh(app: &AppHandle, s: Summary) {
    let Some(tray) = app.try_state::<Tray>() else { return };
    let mut last = tray.last.lock().expect("tray lock");
    if *last == s {
        return;
    }
    let _ = tray.status.set_text(s.status_line());
    let _ = tray.icon.set_tooltip(Some(s.tooltip()));
    #[cfg(target_os = "macos")]
    {
        let _ = tray.icon.set_title(s.title());
    }
    *last = s;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_lines() {
        assert_eq!(Summary::default().status_line(), "Idle");
        assert_eq!(Summary { active: 1, progress: 0.456, ..Default::default() }.status_line(), "1 download · 46%");
        assert_eq!(Summary { active: 3, progress: 0.0, ..Default::default() }.status_line(), "3 downloads · 0%");
        assert_eq!(Summary { done: 2, failed: 1, ..Default::default() }.status_line(), "2 done · 1 failed");
        assert_eq!(Summary { active: 2, progress: 0.5, ..Default::default() }.title().as_deref(), Some("50%"));
        assert_eq!(Summary { done: 2, ..Default::default() }.title(), None);
    }
}
