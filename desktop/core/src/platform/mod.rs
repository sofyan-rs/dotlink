//! Everything that differs per OS. Each platform module provides the same items:
//!
//! | item | what it does |
//! |---|---|
//! | `notify(title, body)` | desktop notification |
//! | `notify_file(from, name, path)` | "file received" notification with Open / Show in folder |
//! | `notify_incoming(app, incoming)`, `dismiss_incoming(id)` | "X wants to send …" with Accept / Decline (`App::answer_incoming`), and taking it away once answered |
//! | `notify_phone(app, n)`, `dismiss_phone(tag)` | a phone's notification ([PhoneNotification]) with Dismiss (`App::dismiss_phone_notification`), and taking it away when the phone removed it |
//! | `now_playing()`, `media_command(cmd)` | what plays on this PC ([NowPlaying]) and controlling it ([MediaCmd]): MPRIS through playerctl, the System Media Transport Controls, AppleScript (Spotify, Music) |
//! | `open_path(path)` | open a file or folder with the default app |
//! | `open_url(url)` | open a web link in the default browser |
//! | `clipboard::{watch, get, set}` | clipboard text, and pushing changes to the phone |
//! | `Input` | virtual mouse and keyboard for the Remote screen |
//! | `CameraHub`, `find_camera` | phone camera as a webcam |
//! | `MicSink` | phone mic as a microphone |
//! | `default_shell()`, `process_name(pid)` | the Shell screen |
//! | `autostart::{enabled, set}` | start with the desktop session |
//! | `setup::{checks, fix}` | permissions and system pieces the features need |
//! | `parent_process()` | the program that started Dotlink, for the log |
//! | `share_menu::set(phones)` | "Share with Dotlink" in the file manager's menu (Explorer, Finder's Quick Actions), one item per device |
//! | `battery()` | the laptop's battery: (percent, on power); None without one |
//! | `computer_name()` | the name the user gave this PC, when it isn't the hostname; else None |
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

pub(crate) mod battery;

/// The phone mic played into a virtual audio device (VB-CABLE, BlackHole).
#[cfg(any(windows, target_os = "macos"))]
pub(crate) mod cpal_sink;

use serde::{Deserialize, Serialize};

/// What plays on this PC, for the phone's Media card.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NowPlaying {
    /// The app: "spotify", "firefox", "Music"…
    pub player: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    pub position_ms: u64,
    /// 0 when unknown (a live stream).
    pub duration_ms: u64,
    pub art: Option<Art>,
    /// The output volume, 0 to 1; None when it can't be read.
    pub volume: Option<f32>,
}

/// Where a track's artwork is.
#[derive(Debug, Clone, PartialEq)]
pub enum Art {
    /// `file://` or `http(s)://`.
    Url(String),
    /// JPEG or PNG bytes.
    Bytes(Vec<u8>),
}

/// A command from the phone's Media card (`media_cmd`).
#[derive(Debug, Clone, PartialEq)]
pub enum MediaCmd {
    Play,
    Pause,
    Toggle,
    Next,
    Prev,
    /// To this position, ms.
    Seek(u64),
    /// The output volume, 0 to 1.
    Volume(f32),
}

impl MediaCmd {
    /// From a `media_cmd` message: `cmd`, with `ms` for seek and `level` (0–100) for volume.
    pub fn from_msg(v: &serde_json::Value) -> Option<Self> {
        Some(match v["cmd"].as_str()? {
            "play" => Self::Play,
            "pause" => Self::Pause,
            "toggle" => Self::Toggle,
            "next" => Self::Next,
            "prev" => Self::Prev,
            "seek" => Self::Seek(v["ms"].as_u64()?),
            "volume" => Self::Volume((v["level"].as_f64()? / 100.0).clamp(0.0, 1.0) as f32),
            _ => return None,
        })
    }
}

/// A phone's notification, as shown on this PC.
#[derive(Debug, Clone)]
pub struct PhoneNotification {
    /// Short and stable (see `server::notification_tag`): what Dismiss and removal refer to.
    pub tag: String,
    /// "<app> · <phone>".
    pub title: String,
    pub body: String,
    /// The app's icon, cached in `config::icon_dir()`.
    pub icon: Option<std::path::PathBuf>,
}

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
