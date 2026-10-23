//! Windows. Files, clipboard, notifications and pairing should work as is; the rest are stubs.
//! Written on Linux and not compiled for this OS yet: expect small fixes on the first build.
//!
//! TODO(windows), roughly in order of value:
//! - [ ] `Input`: SendInput (mouse: MOUSEEVENTF_MOVE / WHEEL / HWHEEL, keyboard: virtual-key
//!       codes; `Text` can use KEYEVENTF_UNICODE so any character types). Crate: `windows`
//!       or `enigo`.
//! - [ ] `process_name`: shell tab titles (ConPTY has no foreground process group; the child
//!       pid's exe name is good enough).
//! - [ ] `autostart`: HKCU\Software\Microsoft\Windows\CurrentVersion\Run, value "Dotlink" =
//!       "<exe> --background" (or the `auto-launch` crate).
//! - [ ] `setup`: Windows Defender Firewall rule for the port (the installer can add it;
//!       `netsh advfirewall firewall add rule … dir=in action=allow protocol=TCP localport=…`).
//! - [ ] `notify_file`: toast with Open / Show in folder buttons (notify-rust can't do actions
//!       on Windows; use the `windows` crate's ToastNotification).
//! - [ ] `CameraHub`: virtual camera. Windows 11 has MFCreateVirtualCamera (needs a COM media
//!       source DLL registered by the installer). Windows 10 needs a DirectShow filter.
//! - [ ] `MicSink`: virtual microphone needs an audio driver; simplest is asking the user to
//!       install VB-CABLE and playing into its input with WASAPI (`cpal`).

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
    let _ = std::process::Command::new("explorer").arg(path).spawn();
}

pub fn show_in_folder(path: &Path) {
    let _ = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
}

pub fn default_shell() -> (String, Vec<String>) {
    ("powershell.exe".into(), vec!["-NoLogo".into()])
}

pub fn process_name(_pid: u32) -> Option<String> {
    None
}

pub struct Input;

impl Input {
    pub fn new() -> Result<Self> {
        bail!("remote control is not supported on Windows yet")
    }

    pub fn handle(&mut self, _msg: InputMsg) -> Result<()> {
        Ok(())
    }
}

pub fn find_camera(_configured: &str) -> Result<PathBuf> {
    bail!("the phone camera as a webcam is not supported on Windows yet")
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
        bail!("the phone mic is not supported on Windows yet")
    }

    pub fn write(&mut self, _pcm: &[u8]) {}
}

pub mod autostart {
    use anyhow::{Result, bail};

    pub fn enabled() -> bool {
        false
    }

    pub fn set(_on: bool) -> Result<()> {
        bail!("start at login is not supported on Windows yet")
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
