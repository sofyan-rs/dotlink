//! Windows 10 and 11. Files, clipboard, notifications, pairing, Shell, Remote, start at login
//! and the firewall check work; webcam and mic are stubs.
//!
//! TODO(windows):
//! - [ ] `CameraHub`: virtual camera. Windows 11 has MFCreateVirtualCamera (needs a COM media
//!   source DLL registered by the installer). Windows 10 needs a DirectShow filter.
//! - [ ] `MicSink`: virtual microphone needs an audio driver; simplest is asking the user to
//!   install VB-CABLE and playing into its input with WASAPI (`cpal`).

pub mod autostart;
mod input;
pub mod setup;

pub(crate) use super::polling_clipboard as clipboard;
pub use input::Input;

use super::Features;
use anyhow::{Result, bail};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// AppUserModelID: toasts are shown under the name and icon registered for it.
pub const APP_ID: &str = "dev.dotlink.Dotlink";

/// Keeps console programs (netsh, powershell) from flashing a console window.
pub(crate) const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn features() -> Features {
    Features { clipboard: true, remote: true, camera: false, mic: false, shell: true }
}

/// A command for a console program, without a console window.
pub(crate) fn hidden_command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// The current user's SID ("S-1-5-21-…"): names and secures the control pipe.
pub fn user_sid() -> std::io::Result<String> {
    static SID: OnceLock<Result<String, String>> = OnceLock::new();
    SID.get_or_init(|| read_user_sid().map_err(|e| e.to_string()))
        .clone()
        .map_err(std::io::Error::other)
}

fn read_user_sid() -> std::io::Result<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        // TOKEN_USER is followed by the SID it points to; u64s keep the buffer aligned.
        let mut buf = vec![0u64; 64];
        let mut len = 0u32;
        let ok = GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), (buf.len() * 8) as u32, &mut len);
        CloseHandle(token);
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut s: *mut u16 = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut s) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let len = (0..).take_while(|&i| *s.add(i) != 0).count();
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(s, len));
        LocalFree(s as HLOCAL);
        Ok(sid)
    }
}

/// Registers the AppUserModelID under HKCU (name and icon of our toasts). Without it, toasts
/// from an exe that has no Start-menu shortcut don't show at all.
fn register_app_id() {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        let icon = crate::config::config_dir().join("dotlink.png");
        if !icon.exists() {
            let _ = std::fs::create_dir_all(crate::config::config_dir());
            let _ = std::fs::write(&icon, include_bytes!("../../../../app/assets/icons/64x64.png"));
        }
        let result = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
            .create_subkey(format!(r"Software\Classes\AppUserModelId\{APP_ID}"))
            .and_then(|(key, _)| {
                key.set_value("DisplayName", &"Dotlink")?;
                key.set_value("IconUri", &icon.to_string_lossy().into_owned())
            });
        if let Err(e) = result {
            tracing::warn!("could not register the notification app id: {e}");
        }
    });
}

fn toast(title: &str, body: &str) -> notify_rust::Notification {
    register_app_id();
    let mut n = notify_rust::Notification::new();
    n.app_id(APP_ID).summary(title).body(body);
    n
}

pub fn notify(title: &str, body: &str) {
    if let Err(e) = toast(title, body).show() {
        tracing::warn!("notification: {e}");
    }
}

/// "Received from …" with Open / Show in folder buttons.
pub fn notify_file(from: &str, name: &str, path: &Path) {
    let mut n = toast(&format!("Received from {from}"), name);
    n.action("open", "Open").action("folder", "Show in folder");
    let path = path.to_path_buf();
    // Waiting for the click blocks; the toast reports it on a WinRT thread.
    std::thread::spawn(move || match n.show() {
        Ok(handle) => handle.wait_for_action(|action| match action {
            "open" => open_path(&path),
            "folder" => show_in_folder(&path),
            _ => {}
        }),
        Err(e) => tracing::warn!("notification: {e}"),
    });
}

pub fn open_path(path: &Path) {
    let _ = Command::new("explorer").arg(path).spawn();
}

/// Opens the folder holding `path` with `path` selected.
pub fn show_in_folder(path: &Path) {
    // explorer wants "/select," and the path as one argument, unquoted.
    let _ = Command::new("explorer").raw_arg(format!("/select,\"{}\"", path.display())).spawn();
}

/// PowerShell 7 (`pwsh`) when installed, otherwise Windows PowerShell.
pub fn default_shell() -> (String, Vec<String>) {
    let shell = if find_on_path("pwsh.exe").is_some() { "pwsh.exe" } else { "powershell.exe" };
    (shell.into(), vec!["-NoLogo".into()])
}

pub(crate) fn find_on_path(exe: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|dir| dir.join(exe)).find(|p| p.is_file())
}

/// Name of a running program ("pwsh", "cmd"…), from its exe.
pub fn process_name(pid: u32) -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
        CloseHandle(process);
        if ok == 0 {
            return None;
        }
        let path = PathBuf::from(String::from_utf16_lossy(&buf[..len as usize]));
        Some(path.file_stem()?.to_string_lossy().into_owned())
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

    pub fn write(&mut self, _pcm: axum::body::Bytes) {}
}
