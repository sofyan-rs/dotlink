//! macOS. Files, clipboard, notifications, pairing and the Remote work; the rest are stubs.
//!
//! TODO(macos), roughly in order of value:
//! - [ ] `setup`: Local Network permission (macOS 15+) is asked on first mDNS use; the app
//!   firewall asks on first listen. Explain both in the Setup screen.
//! - [ ] `CameraHub`: a CoreMediaIO Camera Extension (system extension inside the .app,
//!   needs a signed + notarized build); the app feeds it frames over its sink stream.
//! - [ ] `MicSink`: an AudioServerPlugIn driver (like BlackHole) installed by the .pkg, fed
//!   over shared memory; or ask the user to install BlackHole and play into it.

pub(crate) use super::polling_clipboard as clipboard;

pub mod autostart;
mod input;
pub use input::Input;

use super::{Check, Features};
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

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
    Features { clipboard: true, remote: true, camera: false, mic: false, shell: true }
}

/// The bundle id notifications are sent under.
const BUNDLE_ID: &str = "dev.dotlink.Dotlink";

/// Notifications need the app bundle: macOS shows them under a bundle id. Outside one
/// (`cargo run`) they're only logged. Left to itself, notify-rust looks up the id of an app
/// named "use_default" through AppleScript, and macOS asks the user "Where is use_default?".
pub fn notify(title: &str, body: &str) {
    static BUNDLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let bundled = *BUNDLED.get_or_init(|| {
        let in_bundle = std::env::current_exe().is_ok_and(|exe| exe.to_string_lossy().contains(".app/Contents/MacOS/"));
        in_bundle && notify_rust::set_application(BUNDLE_ID).is_ok()
    });
    if !bundled {
        tracing::info!("notification (not shown outside the app bundle): {title}: {body}");
        return;
    }
    let _ = notify_rust::Notification::new().appname("Dotlink").summary(title).body(body).show();
}

/// TODO(macos): Accept / Decline buttons (UNUserNotificationCenter); the window asks meanwhile.
pub fn notify_incoming(_app: &std::sync::Arc<crate::server::App>, incoming: &crate::events::Incoming) {
    notify(&format!("{} wants to send a file", incoming.from), &format!("{}: Accept or Decline in Dotlink", incoming.name));
}

pub fn dismiss_incoming(_id: &str) {}

/// The parent's name, for the log.
pub fn parent_process() -> Option<String> {
    // SAFETY: getppid can't fail.
    process_name(unsafe { libc::getppid() } as u32)
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

/// The program's name ("zsh", "nvim"), as Activity Monitor shows it.
pub fn process_name(pid: u32) -> Option<String> {
    // proc_name truncates to MAXCOMLEN (16) bytes; 2 × that is what it accepts at most.
    let mut buf = [0u8; 2 * 16];
    // SAFETY: proc_name writes at most `buf.len()` bytes and returns how many it wrote.
    let n = unsafe { libc::proc_name(pid as libc::c_int, buf.as_mut_ptr().cast(), buf.len() as u32) };
    (n > 0).then(|| String::from_utf8_lossy(&buf[..n as usize]).into_owned())
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
