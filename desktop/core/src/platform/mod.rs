//! Everything that differs per OS. Each platform module provides the same items:
//!
//! | item | what it does |
//! |---|---|
//! | `notify(title, body)` | desktop notification |
//! | `notify_file(from, name, path)` | "file received" notification with Open / Show in folder |
//! | `open_path(path)` | open a file or folder with the default app |
//! | `clipboard::{watch, get, set}` | clipboard text, and pushing changes to the phone |
//! | `Input` | virtual mouse and keyboard for the Remote screen |
//! | `CameraHub`, `find_camera` | phone camera as a webcam |
//! | `MicSink` | phone mic as a microphone |
//! | `default_shell()`, `process_name(pid)` | the Shell screen |
//! | `autostart::{enabled, set}` | start with the desktop session |
//! | `setup::{checks, fix}` | permissions and system pieces the features need |
//!
//! Linux is complete, and so is Windows (Windows 11). macOS is stubs that report "not supported
//! yet" (written on Linux, never compiled there yet); their TODO comments say what to build.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

/// Clipboard through `arboard`, polled. Used by Linux X11 sessions, and a starting point for
/// Windows and macOS.
#[allow(dead_code)]
pub(crate) mod polling_clipboard;

use serde::{Deserialize, Serialize};

/// One thing a feature needs from the system, as shown in the app's Setup screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Check {
    /// Stable id, passed back to `setup::fix`.
    pub id: String,
    /// Feature it belongs to: "Remote", "Webcam", "Microphone", "Network"…
    pub feature: String,
    pub title: String,
    pub ok: bool,
    /// What is wrong, or how to fix it by hand.
    pub detail: String,
    /// `setup::fix(id)` can fix it (may ask for the admin password).
    pub fixable: bool,
}

/// What this build can do on this OS, so the UI can hide what isn't there.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Features {
    pub clipboard: bool,
    pub remote: bool,
    pub camera: bool,
    pub mic: bool,
    pub shell: bool,
}
