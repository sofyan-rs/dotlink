//! Windows 11 (Windows 10 isn't supported): everything Linux has. The webcam is a virtual camera
//! whose media source is the `camera` crate's DLL; the mic needs the VB-CABLE driver.

pub mod autostart;
pub(crate) mod camera;
mod input;
mod media;
pub mod setup;

pub(crate) use super::polling_clipboard as clipboard;
pub use camera::{CameraHub, find_camera};
pub use input::Input;
pub use media::MicSink;

use super::Features;
use anyhow::Result;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// AppUserModelID: toasts are shown under the name and icon registered for it.
pub const APP_ID: &str = "dev.dotlink.Dotlink";

/// Keeps console programs (netsh, powershell) from flashing a console window.
pub(crate) const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn features() -> Features {
    Features { clipboard: true, remote: true, camera: true, mic: true, shell: true }
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

/// Whether the taskbar is light (Settings → Personalization → Colors, the Windows mode): the
/// tray icon is drawn black on it, white otherwise.
pub fn taskbar_is_light() -> bool {
    winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|key| key.get_value::<u32, _>("SystemUsesLightTheme"))
        .is_ok_and(|light| light != 0)
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

/// Kills a process and everything it starts when dropped (or when Dotlink exits, even by a
/// crash): a job object with KILL_ON_JOB_CLOSE. For scrcpy, which package managers (Chocolatey,
/// Scoop) install as a shim that runs the real scrcpy as a child: killing the shim alone would
/// leave its window open.
pub struct ProcessTree(windows_sys::Win32::Foundation::HANDLE);

// The handle is only closed, from whichever thread drops it.
unsafe impl Send for ProcessTree {}
unsafe impl Sync for ProcessTree {}

impl ProcessTree {
    /// Call right after spawning: children the process starts from then on join the job too.
    pub fn new(pid: u32) -> Option<Self> {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::JobObjects::*;
        use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return None;
            }
            let tree = Self(job);
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let set = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
            if set == 0 || process.is_null() {
                return None;
            }
            let assigned = AssignProcessToJobObject(job, process);
            CloseHandle(process);
            (assigned != 0).then_some(tree)
        }
    }
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}


