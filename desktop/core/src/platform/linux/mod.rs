//! Linux: any desktop (GNOME, KDE, Hyprland, Sway, niri, X11 window managers).

pub mod autostart;
pub mod clipboard;
mod input;
mod media;
pub mod setup;

/// TODO(linux): Nautilus scripts / Dolphin service menus / Thunar custom actions.
pub mod share_menu {
    pub fn set(_phones: &[(String, String)]) {}
}

pub use input::Input;
pub use media::{CameraHub, MicSink, find_camera};

use super::Features;
use std::path::Path;

/// The hostname is the name the user gave this PC.
pub fn computer_name() -> Option<String> {
    None
}

/// The laptop's battery: (percent, on power); None on a desktop.
pub fn battery() -> Option<(u8, bool)> {
    super::battery::from_sysfs(Path::new("/sys/class/power_supply"))
}

pub fn features() -> Features {
    Features { clipboard: true, remote: true, camera: true, mic: true, shell: true }
}

/// Desktop notification through notify-send (mako, dunst, swaync, GNOME, KDE…).
pub fn notify(title: &str, body: &str) {
    let _ = std::process::Command::new("notify-send").args(["-a", "Dotlink", title, body]).spawn();
}

/// "X wants to send …" with Accept / Decline; the answer goes to [crate::server::App::answer_incoming].
pub fn notify_incoming(app: &std::sync::Arc<crate::server::App>, incoming: &crate::events::Incoming) {
    let (app, incoming) = (app.clone(), incoming.clone());
    tokio::spawn(async move {
        let out = tokio::process::Command::new("notify-send")
            .args(["-a", "Dotlink", "-u", "critical", "-t", "120000", "--action=accept=Accept", "--action=decline=Decline"])
            .arg(incoming.title())
            .arg(incoming.names())
            .output()
            .await;
        match out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).as_deref() {
            Ok("accept") => drop(app.answer_incoming(&incoming.id, true)),
            Ok("decline") => drop(app.answer_incoming(&incoming.id, false)),
            _ => {}
        }
    });
}

/// A phone's notification with Dismiss, which dismisses it on the phone too.
pub fn notify_phone(app: &std::sync::Arc<crate::server::App>, n: &super::PhoneNotification) {
    let (app, n) = (app.clone(), n.clone());
    tokio::spawn(async move {
        let mut cmd = tokio::process::Command::new("notify-send");
        cmd.args(["-a", "Dotlink", "--action=dismiss=Dismiss"]);
        if let Some(icon) = &n.icon {
            cmd.arg("-i").arg(icon);
        }
        let out = cmd.arg(&n.title).arg(&n.body).output().await;
        if out.map(|o| String::from_utf8_lossy(&o.stdout).trim() == "dismiss").unwrap_or(false)
            && let Err(e) = app.dismiss_phone_notification(&n.tag)
        {
            tracing::debug!("dismiss: {e}");
        }
    });
}

/// notify-send can't take a notification back: it expires by itself.
pub fn dismiss_phone(_tag: &str) {}

/// The program that started this one, for the log.
pub fn parent_process() -> Option<String> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    // The command name is in parentheses and may hold spaces: the ppid follows its end.
    let ppid = stat.rsplit(')').next()?.split_whitespace().nth(1)?;
    std::fs::read_to_string(format!("/proc/{ppid}/comm")).ok().map(|s| s.trim().to_string())
}

/// notify-send's notification expires by itself (`-t`): nothing to take away.
pub fn dismiss_incoming(_id: &str) {}

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

/// Opens a web link in the default browser.
pub fn open_url(url: &str) {
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
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
