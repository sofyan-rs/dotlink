//! macOS. Everything works; the webcam goes through OBS (camera.rs) and the mic through
//! BlackHole (media.rs). Notifications are posted by the app, see [Notify].
//!
//! TODO(macos), roughly in order of value:
//! - [ ] `setup`: Local Network permission (macOS 15+) is asked on first mDNS use; the app
//!   firewall asks on first listen. Explain both in the Setup screen.

pub(crate) use super::polling_clipboard as clipboard;

pub mod autostart;
mod camera;
mod input;
mod media;
mod player;
pub mod share_menu;
pub use camera::{CameraHub, camera_url, find_camera};
pub use input::Input;
pub use media::MicSink;
pub use player::{media_command, now_playing};

use super::{Check, Features};
use std::path::Path;

/// "MacBook Air", as set in System Settings → General → Sharing. The hostname often isn't it:
/// a Mac on DHCP may get its IP address (192.168.1.3) as hostname.
pub fn computer_name() -> Option<String> {
    let out = std::process::Command::new("scutil").args(["--get", "ComputerName"]).output().ok()?;
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !name.is_empty()).then_some(name)
}

/// The MacBook's battery: (percent, on power); None on a Mac without one.
pub fn battery() -> Option<(u8, bool)> {
    let out = std::process::Command::new("pmset").args(["-g", "batt"]).output().ok()?;
    super::battery::from_pmset(&String::from_utf8_lossy(&out.stdout))
}

pub fn features() -> Features {
    Features { clipboard: true, remote: true, camera: true, mic: true, shell: true }
}

/// What the app's window process posts to Notification Center (GPUI's notifications, which
/// have buttons). The service runs in that process; a headless `dotlink daemon` only logs.
#[derive(Debug, Clone)]
pub enum Notify {
    /// `tag` says what it's about and what its buttons do: `file:<path>` (Open, Show in
    /// Finder), `incoming:<id>` (Accept, Decline), `notif:<tag>` (a phone's notification:
    /// Dismiss), anything else (click opens Dotlink).
    Show { tag: String, title: String, body: String, actions: Vec<(&'static str, &'static str)> },
    Dismiss { tag: String },
}

static NOTIFIER: std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedSender<Notify>>> = std::sync::Mutex::new(None);

/// Called by the app once its window side runs; notifications go there from then on.
pub fn set_notifier(tx: tokio::sync::mpsc::UnboundedSender<Notify>) {
    *NOTIFIER.lock().unwrap() = Some(tx);
}

fn post(n: Notify) {
    let sent = NOTIFIER.lock().unwrap().as_ref().is_some_and(|tx| tx.send(n.clone()).is_ok());
    if !sent && let Notify::Show { title, body, .. } = n {
        tracing::info!("notification (no window process to show it): {title}: {body}");
    }
}

pub fn notify(title: &str, body: &str) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    post(Notify::Show { tag: format!("note:{n}"), title: title.into(), body: body.into(), actions: Vec::new() });
}

/// "X wants to send …" with Accept / Decline (`App::answer_incoming` through the app).
pub fn notify_incoming(_app: &std::sync::Arc<crate::server::App>, incoming: &crate::events::Incoming) {
    post(Notify::Show {
        tag: format!("incoming:{}", incoming.id),
        title: incoming.title(),
        body: incoming.names(),
        actions: vec![("accept", "Accept"), ("decline", "Decline")],
    });
}

pub fn dismiss_incoming(id: &str) {
    post(Notify::Dismiss { tag: format!("incoming:{id}") });
}

/// A phone's notification with Dismiss (`notif:<tag>`; the app passes Dismiss on to
/// `App::dismiss_phone_notification`).
pub fn notify_phone(_app: &std::sync::Arc<crate::server::App>, n: &super::PhoneNotification) {
    post(Notify::Show { tag: format!("notif:{}", n.tag), title: n.title.clone(), body: n.body.clone(), actions: vec![("dismiss", "Dismiss")] });
}

pub fn dismiss_phone(tag: &str) {
    post(Notify::Dismiss { tag: format!("notif:{tag}") });
}

/// The parent's name, for the log.
pub fn parent_process() -> Option<String> {
    // SAFETY: getppid can't fail.
    process_name(unsafe { libc::getppid() } as u32)
}

/// "Received from …" with Open / Show in Finder.
pub fn notify_file(from: &str, name: &str, path: &Path) {
    post(Notify::Show {
        tag: format!("file:{}", path.display()),
        title: format!("Received from {from}"),
        body: name.into(),
        actions: vec![("open", "Open"), ("reveal", "Show in Finder")],
    });
}

pub fn open_path(path: &Path) {
    let _ = std::process::Command::new("open").arg(path).spawn();
}

/// Opens a web link in the default browser.
pub fn open_url(url: &str) {
    let _ = std::process::Command::new("open").arg(url).spawn();
}

pub fn show_in_folder(path: &Path) {
    let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
}

pub fn default_shell() -> (String, Vec<String>) {
    (std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into()), vec!["-l".into()])
}

/// The program's name ("zsh", "nvim"), as Activity Monitor shows it.
pub fn process_name(pid: u32) -> Option<String> {
    // proc_name truncates to MAXCOMLEN (16) bytes; 2 × that is what it accepts at most.
    let mut buf = [0u8; 2 * 16];
    // SAFETY: proc_name writes at most `buf.len()` bytes and returns how many it wrote.
    let n = unsafe { libc::proc_name(pid as libc::c_int, buf.as_mut_ptr().cast(), buf.len() as u32) };
    if n > 0 {
        return Some(String::from_utf8_lossy(&buf[..n as usize]).into_owned());
    }
    // proc_name only reads this user's processes; `top` and `sudo` run as root (setuid). ps
    // reads any process's name (through sysctl).
    let out = std::process::Command::new("/bin/ps").args(["-c", "-o", "comm=", "-p", &pid.to_string()]).output().ok()?;
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !name.is_empty()).then_some(name)
}

pub mod setup {
    use super::Check;
    use anyhow::{Result, bail};

    const ACCESSIBILITY_SETTINGS: &str =
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

    pub fn checks(_port: u16) -> Vec<Check> {
        let mut out = Vec::new();

        let trusted = super::input::trusted();
        out.push(Check {
            id: "accessibility".into(),
            feature: "Remote".into(),
            title: "Accessibility permission".into(),
            ok: trusted,
            detail: if trusted {
                String::new()
            } else {
                "macOS lets Dotlink move the pointer and type only with it. Fix opens System Settings: turn \
                 Dotlink on (from a terminal, the terminal app)."
                    .into()
            },
            fixable: !trusted,
        });

        // Webcam: OBS reads the phone's frames and is the camera apps see.
        let obs = super::camera::obs_installed();
        out.push(Check {
            id: "obs".into(),
            feature: "Webcam".into(),
            title: "OBS Studio".into(),
            ok: obs,
            detail: if obs {
                "In OBS: add a Browser source, paste the camera link (Phones → Webcam → Copy OBS link) as \
                 its URL, 1280 × 720. Then Start Virtual Camera; apps pick \"OBS Virtual Camera\"."
                    .into()
            } else {
                "macOS only lets signed camera drivers in, so the phone camera goes through OBS's virtual \
                 camera. Fix opens OBS's download page."
                    .into()
            },
            fixable: !obs,
        });

        // Microphone: BlackHole provides the device apps record from.
        let blackhole = super::media::find_blackhole();
        out.push(Check {
            id: "blackhole".into(),
            feature: "Microphone".into(),
            title: "BlackHole audio driver".into(),
            ok: blackhole.is_some(),
            detail: match &blackhole {
                Some((_, name)) => format!("In your app, pick \"{name}\" as the microphone."),
                None => "The phone mic plays into it, and apps use it as their microphone. Fix downloads and \
                         installs it (asks for your password); then restart Dotlink."
                    .into(),
            },
            fixable: blackhole.is_none(),
        });

        // Cast. The scrcpy download includes adb; Cast also downloads it on first use.
        let downloadable = crate::cast::can_install();
        for (id, title) in [("adb", "adb (Android platform tools)"), ("scrcpy", "scrcpy")] {
            let ok = crate::cast::find_tool(id).is_some();
            out.push(Check {
                id: id.into(),
                feature: "Cast".into(),
                title: title.into(),
                ok,
                detail: if ok {
                    String::new()
                } else if downloadable {
                    "Fix downloads scrcpy with adb (from GitHub). Or install: brew install scrcpy".into()
                } else {
                    "Install: brew install scrcpy".into()
                },
                fixable: !ok && downloadable,
            });
        }
        out
    }

    pub async fn fix(id: &str, _port: u16) -> Result<()> {
        match id {
            "accessibility" => {
                // The prompt adds Dotlink to the list; the switch is the user's to turn on.
                if !super::input::ask_trust() {
                    super::open_path(std::path::Path::new(ACCESSIBILITY_SETTINGS));
                }
                Ok(())
            }
            "adb" | "scrcpy" => crate::cast::install().await,
            "blackhole" => super::media::install_blackhole().await,
            "obs" => {
                super::open_path(std::path::Path::new(super::camera::obs_download_url()));
                Ok(())
            }
            _ => bail!("nothing to fix for \"{id}\""),
        }
    }

    pub async fn fix_many(ids: &[String], port: u16) -> Result<()> {
        // One download covers both.
        if ids.iter().any(|id| id == "adb" || id == "scrcpy") {
            crate::cast::install().await?;
        }
        for id in ids.iter().filter(|id| *id != "adb" && *id != "scrcpy") {
            fix(id, port).await?;
        }
        Ok(())
    }
}
