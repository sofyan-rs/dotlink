//! macOS. Files, clipboard, notifications and pairing should work as is; the rest are stubs.
//! Written on Linux and not compiled for this OS yet: expect small fixes on the first build.
//!
//! TODO(macos), roughly in order of value:
//! - [ ] `Input`: CGEventCreateMouseEvent / CGEventCreateScrollWheelEvent2 /
//!       CGEventCreateKeyboardEvent (`core-graphics` crate, or `enigo`). Needs the
//!       Accessibility permission: `setup` should check AXIsProcessTrusted() and open
//!       System Settings → Privacy & Security → Accessibility.
//! - [ ] `process_name`: shell tab titles (`proc_name` from libproc for the foreground pid).
//! - [ ] `autostart`: a LaunchAgent plist in ~/Library/LaunchAgents running
//!       "<app>/Contents/MacOS/dotlink --background" (or SMAppService on macOS 13+).
//! - [ ] `setup`: Local Network permission (macOS 15+) is asked on first mDNS use; the app
//!       firewall asks on first listen. Explain both in the Setup screen.
//! - [ ] `CameraHub`: a CoreMediaIO Camera Extension (system extension inside the .app,
//!       needs a signed + notarized build); the app feeds it frames over its sink stream.
//! - [ ] `MicSink`: an AudioServerPlugIn driver (like BlackHole) installed by the .pkg, fed
//!       over shared memory; or ask the user to install BlackHole and play into it.

pub(crate) use super::polling_clipboard as clipboard;

use super::{Check, Features};
use crate::input::InputMsg;
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

pub fn features() -> Features {
    Features { clipboard: true, remote: false, camera: false, mic: false, shell: true }
}

pub fn notify(title: &str, body: &str) {
    let _ = notify_rust::Notification::new().appname("Dotlink").summary(title).body(body).show();
}

pub fn notify_file(from: &str, name: &str, _path: &Path) {
    notify(&format!("Received from {from}"), name);
}

pub fn open_path(path: &Path) {
    let _ = std::process::Command::new("open").arg(path).spawn();
}

pub fn show_in_folder(path: &Path) {
    let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
}

pub fn default_shell() -> (String, Vec<String>) {
    (std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into()), vec!["-l".into()])
}

pub fn process_name(_pid: u32) -> Option<String> {
    None
}

pub struct Input;

impl Input {
    pub fn new() -> Result<Self> {
        bail!("remote control is not supported on macOS yet")
    }

    pub fn handle(&mut self, _msg: InputMsg) -> Result<()> {
        Ok(())
    }
}

pub fn find_camera(_configured: &str) -> Result<PathBuf> {
    bail!("the phone camera as a webcam is not supported on macOS yet")
}

#[derive(Clone)]
pub struct CameraHub;

impl CameraHub {
    pub fn start(_configured_device: String, _size: (u32, u32)) -> Self {
        Self
    }

    pub fn send(&self, _jpeg: axum::body::Bytes) {}
}

pub struct MicSink;

impl MicSink {
    pub fn start(_rate: u32, _channels: u32) -> Result<Self> {
        bail!("the phone mic is not supported on macOS yet")
    }

    pub fn write(&mut self, _pcm: axum::body::Bytes) {}
}

/// TODO(macos): a Finder Share extension or a Quick Action.
pub mod share_menu {
    pub fn set(_phones: &[(String, String)]) {}
}

pub mod autostart {
    use anyhow::{Result, bail};

    pub fn enabled() -> bool {
        false
    }

    pub fn set(_on: bool) -> Result<()> {
        bail!("start at login is not supported on macOS yet")
    }

    pub fn wm_hint() -> Option<String> {
        None
    }
}

pub mod setup {
    use super::Check;
    use anyhow::{Result, bail};

    pub fn checks(_port: u16) -> Vec<Check> {
        Vec::new()
    }

    pub async fn fix(id: &str, _port: u16) -> Result<()> {
        bail!("nothing to fix for \"{id}\"")
    }

    pub async fn fix_many(ids: &[String], port: u16) -> Result<()> {
        for id in ids {
            fix(id, port).await?;
        }
        Ok(())
    }
}
