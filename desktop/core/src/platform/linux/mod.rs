//! Linux: any desktop (GNOME, KDE, Hyprland, Sway, niri, X11 window managers).

pub mod autostart;
pub mod clipboard;
mod input;
mod media;
pub mod setup;

pub use input::Input;
pub use media::{CameraHub, MicSink, find_camera};

use super::Features;
use std::path::Path;

pub fn features() -> Features {
    Features { clipboard: true, remote: true, camera: true, mic: true, shell: true }
}

/// Desktop notification through notify-send (mako, dunst, swaync, GNOME, KDE…).
pub fn notify(title: &str, body: &str) {
    let _ = std::process::Command::new("notify-send").args(["-a", "Dotlink", title, body]).spawn();
}

/// "Received from …" with Open / Show in folder actions.
pub fn notify_file(from: &str, name: &str, path: &Path) {
    let path = path.to_path_buf();
    let (from, name) = (from.to_string(), name.to_string());
    tokio::spawn(async move {
        let out = tokio::process::Command::new("notify-send")
            .args(["-a", "Dotlink", "-i", "document-save", "--action=open=Open", "--action=folder=Show in folder"])
            .arg(format!("Received from {from}"))
            .arg(&name)
            .output()
            .await;
        match out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).as_deref() {
            Ok("open") => open_path(&path),
            Ok("folder") => show_in_folder(&path),
            _ => {}
        }
    });
}

/// Opens a file or folder with the default app.
pub fn open_path(path: &Path) {
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

/// Opens the folder holding `path`.
pub fn show_in_folder(path: &Path) {
    open_path(path.parent().unwrap_or(path));
}

/// The user's login shell and its arguments.
pub fn default_shell() -> (String, Vec<String>) {
    (std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into()), vec!["-l".into()])
}

/// Name of a running program ("zsh", "htop", "nvim"…).
pub fn process_name(pid: u32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/comm")).ok().map(|s| s.trim().to_string())
}
