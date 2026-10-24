//! Clipboard on any Linux desktop.
//!
//! - Wayland compositors with the data-control protocol (Hyprland, Sway, niri, KDE, COSMIC…):
//!   `wl-paste --watch` / `wl-copy`, event driven.
//! - Everything else (GNOME on Wayland, any X11 session): the X11 clipboard through `arboard`,
//!   read when XFixes reports a new owner (polled where XFixes is missing). On GNOME this goes through XWayland, which Mutter keeps in sync
//!   with the Wayland clipboard.

use super::super::polling_clipboard::shared as x11;
use crate::server::App;
use base64::Engine;
use std::process::Stdio;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Backend {
    Wayland,
    X11,
}

static BACKEND: OnceLock<Backend> = OnceLock::new();

/// Watches the desktop clipboard and pushes text changes to the phone.
pub async fn watch(app: Arc<App>) {
    // DOTLINK_CLIPBOARD=x11 skips wl-clipboard (for testing, or a compositor that misbehaves).
    let force_x11 = std::env::var("DOTLINK_CLIPBOARD").is_ok_and(|v| v == "x11");
    if !force_x11 && std::env::var_os("WAYLAND_DISPLAY").is_some() && which("wl-paste") {
        let why = watch_wayland(&app).await;
        tracing::info!("clipboard: wl-clipboard unavailable ({why}), using X11");
    }
    if std::env::var_os("DISPLAY").is_none() {
        tracing::warn!("clipboard sync disabled: no data-control Wayland compositor and no X11/XWayland display");
        return;
    }
    let _ = BACKEND.set(Backend::X11);
    tracing::info!("clipboard: using the X11 clipboard");
    let mut changes = x11().subscribe();
    while let Some(text) = changes.recv().await {
        app.clip_from_desktop(text);
    }
}

/// Puts `text` on the desktop clipboard.
pub async fn set(text: &str) -> anyhow::Result<()> {
    if backend() == Backend::X11 {
        return x11().set(text.to_string());
    }
    let mut child = Command::new("wl-copy").stdin(Stdio::piped()).spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(text.as_bytes()).await?;
    drop(stdin);
    child.wait().await?;
    Ok(())
}

/// Current desktop clipboard text, if any.
pub async fn get() -> Option<String> {
    if backend() == Backend::X11 {
        return x11().get();
    }
    let out = Command::new("wl-paste").args(["--type", "text", "--no-newline"]).output().await.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The backend `watch` settled on; before that, a best guess from the environment.
fn backend() -> Backend {
    if std::env::var("DOTLINK_CLIPBOARD").is_ok_and(|v| v == "x11") {
        return Backend::X11;
    }
    *BACKEND.get().unwrap_or(
        &if std::env::var_os("WAYLAND_DISPLAY").is_some() && which("wl-copy") { Backend::Wayland } else { Backend::X11 },
    )
}

// ---------- Wayland (wl-clipboard) ----------

/// Runs while wl-clipboard works (restarting it if it dies). Returns only when it can't work
/// here, e.g. the compositor lacks data-control (GNOME), with the reason.
async fn watch_wayland(app: &Arc<App>) -> String {
    // Entries marked by password managers (`x-kde-passwordManagerHint`) are never sent.
    let script = "if wl-paste -l 2>/dev/null | grep -q x-kde-passwordManagerHint; \
                  then cat >/dev/null; echo; else base64 -w0; echo; fi";
    let mut worked = false;
    loop {
        let started = std::time::Instant::now();
        let child = Command::new("wl-paste")
            .args(["--type", "text", "--watch", "sh", "-c", script])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => return e.to_string(),
        };
        let mut stderr = child.stderr.take().unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            worked = true;
            if line.is_empty() {
                continue;
            }
            let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(line.trim()) else { continue };
            app.clip_from_desktop(String::from_utf8_lossy(&bytes).into_owned());
        }
        let _ = child.wait().await;
        // Exiting right away without ever reporting means it can't work here (no data-control).
        if !worked && started.elapsed() < Duration::from_secs(3) {
            let mut err = String::new();
            let _ = stderr.read_to_string(&mut err).await;
            return err.lines().next().unwrap_or("exited").trim().to_string();
        }
        let _ = BACKEND.set(Backend::Wayland);
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

pub(super) fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file()))
        .unwrap_or(false)
}
