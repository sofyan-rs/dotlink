//! Windows 11 (Windows 10 isn't supported): everything Linux has. The webcam is a virtual camera
//! whose media source is the `camera` crate's DLL; the mic needs the VB-CABLE driver.

pub mod autostart;
pub(crate) mod camera;
mod input;
mod media;
mod player;
pub mod setup;
pub mod share_menu;

pub(crate) use super::polling_clipboard as clipboard;
pub use camera::{CameraHub, find_camera};
pub use input::Input;
pub use media::MicSink;
pub use player::{media_command, now_playing};

use super::Features;
use anyhow::Result;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

/// AppUserModelID: toasts are shown under the name and icon registered for it.
pub const APP_ID: &str = "dev.dotlink.Dotlink";

/// Keeps console programs (netsh, powershell) from flashing a console window.
pub(crate) const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The hostname is the name the user gave this PC.
pub fn computer_name() -> Option<String> {
    None
}

/// The laptop's battery: (percent, on power); None on a desktop.
pub fn battery() -> Option<(u8, bool)> {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut status: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    if unsafe { GetSystemPowerStatus(&mut status) } == 0 {
        return None;
    }
    // 128: no system battery; 255: unknown. A percent of 255 is unknown too.
    if status.BatteryFlag & 128 != 0 || status.BatteryFlag == 255 || status.BatteryLifePercent > 100 {
        return None;
    }
    Some((status.BatteryLifePercent, status.ACLineStatus == 1))
}

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
///
/// In dark mode Windows shows a dark icon inverted (a white tile, black dots and a cyan one), so
/// there the icon is the dots alone. Checked before every toast: the mode can change meanwhile.
fn register_app_id() {
    static LAST: Mutex<Option<bool>> = Mutex::new(None);
    let light = taskbar_is_light();
    let mut last = LAST.lock().unwrap();
    if *last == Some(light) {
        return;
    }
    let (name, png): (_, &[u8]) = if light {
        ("dotlink.png", include_bytes!("../../../../app/assets/icons/64x64.png"))
    } else {
        ("dotlink-dark.png", include_bytes!("../../../../app/assets/notification-dark.png"))
    };
    let icon = crate::config::config_dir().join(name);
    let _ = std::fs::create_dir_all(crate::config::config_dir());
    let _ = std::fs::write(&icon, png);
    let result = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .create_subkey(format!(r"Software\Classes\AppUserModelId\{APP_ID}"))
        .and_then(|(key, _)| {
            key.set_value("DisplayName", &"Dotlink")?;
            key.set_value("IconUri", &icon.to_string_lossy().into_owned())
        });
    match result {
        Ok(()) => *last = Some(light),
        Err(e) => tracing::warn!("could not register the notification app id: {e}"),
    }
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

/// "Received from …" with Open / Show in folder buttons; clicking the toast itself shows it in
/// its folder too.
///
/// The buttons are `dotlink:` links (protocol activation, see [register_url_scheme]), not
/// in-process click handlers: those only hear clicks while the toast is on screen, so a click
/// from the notification center, after it slid away, did nothing.
pub fn notify_file(from: &str, name: &str, path: &Path) {
    register_app_id();
    register_url_scheme();
    let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if let Err(e) = show_toast_xml(&file_toast_xml(from, name, &file)) {
        tracing::warn!("notification: {e}");
    }
}

/// "X wants to send …" with Accept / Decline: `dotlink:accept/<id>` and `dotlink:decline/<id>`
/// links (`dotlink url` passes them to the service), so they work from the notification
/// center too. A reminder toast: it stays up until answered.
pub fn notify_incoming(_app: &std::sync::Arc<crate::server::App>, incoming: &crate::events::Incoming) {
    register_app_id();
    register_url_scheme();
    let link = |action: &str| xml_escape(&format!("dotlink:{action}/{}", incoming.id));
    let xml = format!(
        r#"<toast scenario="reminder"><visual><binding template="ToastGeneric"><text>{title}</text><text>{body}</text></binding></visual><actions><action content="Accept" activationType="protocol" arguments="{accept}"/><action content="Decline" activationType="protocol" arguments="{decline}"/></actions></toast>"#,
        title = xml_escape(&incoming.title()),
        body = xml_escape(&incoming.names()),
        accept = link("accept"),
        decline = link("decline"),
    );
    if let Err(e) = show_incoming_toast(&xml, &incoming.id) {
        tracing::warn!("notification: {e}");
    }
}

/// The incoming file's toast: tagged with its id, so [dismiss_incoming] can take it away, and
/// gone by itself when the answer is no longer awaited.
fn show_incoming_toast(xml: &str, id: &str) -> windows::core::Result<()> {
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Foundation::{DateTime, IReference, PropertyValue};
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::core::{HSTRING, Interface};
    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(xml))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    toast.SetTag(&HSTRING::from(id))?;
    toast.SetGroup(&HSTRING::from(INCOMING_GROUP))?;
    // DateTime counts 100 ns ticks from 1601; the Unix epoch is 11644473600 s after that.
    let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let expires = (unix.as_secs() as i64 + 11_644_473_600 + 120) * 10_000_000;
    let when: IReference<DateTime> = PropertyValue::CreateDateTime(DateTime { UniversalTime: expires })?.cast()?;
    toast.SetExpirationTime(&when)?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))?.Show(&toast)
}

const INCOMING_GROUP: &str = "incoming";

/// Takes away the toast of an incoming file that was answered (here, in the window) or
/// declined for lack of an answer.
pub fn dismiss_incoming(id: &str) {
    use windows::UI::Notifications::ToastNotificationManager;
    use windows::core::HSTRING;
    let removed = ToastNotificationManager::History().and_then(|h| {
        h.RemoveGroupedTagWithId(&HSTRING::from(id), &HSTRING::from(INCOMING_GROUP), &HSTRING::from(APP_ID))
    });
    if let Err(e) = removed {
        tracing::debug!("notification: {e}");
    }
}

const PHONE_GROUP: &str = "phone";

/// A phone's notification with Dismiss: a `dotlink:dismiss/<tag>` link, which dismisses it on
/// the phone too. The app's icon shows as the toast's logo.
pub fn notify_phone(_app: &std::sync::Arc<crate::server::App>, n: &super::PhoneNotification) {
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::core::HSTRING;
    register_app_id();
    register_url_scheme();
    let logo = n.icon.as_ref().map(|p| {
        format!(r#"<image placement="appLogoOverride" hint-crop="circle" src="{}"/>"#, xml_escape(&p.to_string_lossy()))
    });
    let xml = format!(
        r#"<toast><visual><binding template="ToastGeneric">{logo}<text>{title}</text><text>{body}</text></binding></visual><actions><action content="Dismiss" activationType="protocol" arguments="{dismiss}"/></actions></toast>"#,
        logo = logo.unwrap_or_default(),
        title = xml_escape(&n.title),
        body = xml_escape(&n.body),
        dismiss = xml_escape(&format!("dotlink:dismiss/{}", n.tag)),
    );
    let shown = (|| -> windows::core::Result<()> {
        let doc = XmlDocument::new()?;
        doc.LoadXml(&HSTRING::from(xml))?;
        let toast = ToastNotification::CreateToastNotification(&doc)?;
        toast.SetTag(&HSTRING::from(n.tag.as_str()))?;
        toast.SetGroup(&HSTRING::from(PHONE_GROUP))?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))?.Show(&toast)
    })();
    if let Err(e) = shown {
        tracing::warn!("notification: {e}");
    }
}

/// Takes away a phone's notification once the phone removed it.
pub fn dismiss_phone(tag: &str) {
    use windows::UI::Notifications::ToastNotificationManager;
    use windows::core::HSTRING;
    let removed = ToastNotificationManager::History()
        .and_then(|h| h.RemoveGroupedTagWithId(&HSTRING::from(tag), &HSTRING::from(PHONE_GROUP), &HSTRING::from(APP_ID)));
    if let Err(e) = removed {
        tracing::debug!("notification: {e}");
    }
}

/// `name`: what to show; `file`: the file's name in the download folder, for the links.
fn file_toast_xml(from: &str, name: &str, file: &str) -> String {
    let link = |action: &str| xml_escape(&format!("dotlink:{action}/{}", crate::config::url_encode(file)));
    format!(
        r#"<toast launch="{reveal}" activationType="protocol"><visual><binding template="ToastGeneric"><text>{title}</text><text>{body}</text></binding></visual><actions><action content="Open" activationType="protocol" arguments="{open}"/><action content="Show in folder" activationType="protocol" arguments="{reveal}"/></actions></toast>"#,
        title = xml_escape(&format!("Received from {from}")),
        body = xml_escape(name),
        open = link("open"),
        reveal = link("reveal"),
    )
}

fn show_toast_xml(xml: &str) -> windows::core::Result<()> {
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::core::HSTRING;
    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(xml))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))?.Show(&toast)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// The `dotlink:` URL scheme under `HKCU\Software\Classes\dotlink`: Windows runs
/// `dotlink url "<link>"` for it, also when Dotlink isn't running.
fn register_url_scheme() {
    static DONE: OnceLock<()> = OnceLock::new();
    // A second instance (`DOTLINK_CONFIG_DIR`, for testing) leaves the links to the usual one:
    // they'd run without that variable and reach the usual service anyway.
    if !crate::config::instance_suffix().is_empty() {
        return;
    }
    DONE.get_or_init(|| {
        let result = (|| -> Result<()> {
            let exe = std::env::current_exe()?;
            let (key, _) =
                winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).create_subkey(r"Software\Classes\dotlink")?;
            key.set_value("", &"URL:Dotlink")?;
            key.set_value("URL Protocol", &"")?;
            key.create_subkey(r"shell\open\command")?.0.set_value("", &format!("\"{}\" url \"%1\"", exe.display()))?;
            Ok(())
        })();
        if let Err(e) = result {
            tracing::warn!("could not register the dotlink: links: {e:#}");
        }
    });
}

/// Runs a `dotlink:` link from a notification: `dotlink:open/<file name>` opens a received file,
/// `dotlink:reveal/<file name>` shows it in its folder. Only files directly in the download
/// folder: any web page can ask the browser to open such a link.
pub fn open_link(link: &str) -> Result<()> {
    let rest = link.strip_prefix("dotlink:").ok_or_else(|| anyhow::anyhow!("not a dotlink: link"))?;
    let rest = rest.trim_start_matches('/').trim_end_matches('/');
    let (action, name) = rest.split_once('/').ok_or_else(|| anyhow::anyhow!("unknown link {link}"))?;
    let name = crate::config::url_decode(name).ok_or_else(|| anyhow::anyhow!("bad file name in {link}"))?;
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', ':']) {
        anyhow::bail!("bad file name in {link}");
    }
    let path = crate::config::Config::load()?.download_dir().join(&name);
    if !path.is_file() {
        anyhow::bail!("{name} is no longer in the download folder");
    }
    match action {
        "open" => open_path(&path),
        "reveal" => show_in_folder(&path),
        _ => anyhow::bail!("unknown link {link}"),
    }
    Ok(())
}

pub fn open_path(path: &Path) {
    let _ = Command::new("explorer").arg(path).spawn();
}

/// Opens a web link in the default browser. ShellExecuteW, not `explorer <url>` or `cmd /c
/// start`: those take `&` and `^` in a query string as their own syntax.
pub fn open_url(url: &str) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let (verb, url) = (wide("open"), wide(url));
    // SAFETY: both strings are NUL-terminated and outlive the call.
    let r = unsafe {
        ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), url.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL)
    };
    // Above 32 is success.
    if r as isize <= 32 {
        tracing::warn!("could not open the link ({})", r as isize);
    }
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

/// The program that started this one ("explorer", "powershell"…), for the log: Explorer
/// means a shortcut or a notification, PowerShell the updater.
pub fn parent_process() -> Option<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };
    let me = std::process::id();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        // Every process with its parent and exe name, in one pass.
        let mut all = Vec::new();
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snapshot, &mut entry) != 0;
        while ok {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            all.push((entry.th32ProcessID, entry.th32ParentProcessID, String::from_utf16_lossy(&entry.szExeFile[..len])));
            ok = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        let parent = all.iter().find(|(pid, _, _)| *pid == me)?.1;
        Some(match all.iter().find(|(pid, _, _)| *pid == parent) {
            Some((_, _, name)) => name.trim_end_matches(".exe").to_string(),
            None => format!("pid {parent}, gone"),
        })
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_toast_xml_parses() {
        let xml = file_toast_xml("Pixel <8> & \"co\"", "Résumé & 'notes'.pdf", "Résumé & 'notes'.pdf");
        let doc = windows::Data::Xml::Dom::XmlDocument::new().unwrap();
        doc.LoadXml(&windows::core::HSTRING::from(xml.as_str())).unwrap();
        assert!(xml.contains("dotlink:open/R%C3%A9sum%C3%A9%20%26%20%27notes%27.pdf"));
    }

    /// Shows a real toast: `cargo test -p dotlink-core file_toast_shows -- --ignored`.
    #[test]
    #[ignore]
    fn file_toast_shows() {
        register_app_id();
        show_toast_xml(&file_toast_xml("Test phone", "dotlink toast test & co.txt", "dotlink toast test & co.txt")).unwrap();
    }
}
