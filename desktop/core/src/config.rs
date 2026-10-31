//! Config file, paired devices and on-disk paths.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub const DEFAULT_PORT: u16 = 47390;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Name the phones show for this PC; empty means the hostname.
    pub name: String,
    /// TCP port the HTTPS/WebSocket server listens on.
    pub port: u16,
    /// Where files sent from the phone are saved. `~` is expanded.
    pub download_dir: String,
    /// The old switch for every phone at once; clipboard sync is per device now
    /// (`Device::clipboard_sync`). Read so `false` can be carried over, never written.
    #[serde(skip_serializing)]
    pub clipboard_sync: bool,
    /// Ask (notification, window) before taking a file from a phone or PC; off: take it.
    pub ask_before_receiving: bool,
    /// Allow the phone to open a shell on this PC.
    pub terminal: bool,
    /// When non-empty, the phone must send this PIN to open the terminal.
    pub terminal_pin: String,
    /// Shell for the terminal; empty means `$SHELL`.
    pub shell: String,
    /// v4l2loopback device for the phone camera; empty means "find the one named Dotlink Camera".
    pub camera_device: String,
    /// Fixed size of "Dotlink Camera". Phone frames are scaled into it (portrait gets side
    /// bars); it never changes while running, because apps reading the device can't follow.
    pub camera_size: String,
    /// Cast: turn the phone's screen off while its picture is on the PC.
    pub cast_screen_off: bool,
    /// Cast: play the phone's sound on the PC (Android 11+).
    pub cast_audio: bool,
    /// Cast: longest side of the picture in pixels; 0 keeps the phone's resolution.
    pub cast_max_size: u32,
    /// The desktop window's look: "system" (follow the desktop), "light" or "dark".
    pub theme: String,
    /// Link phones plugged in over USB (with USB debugging allowed) through the cable
    /// (`adb reverse`) instead of Wi-Fi.
    pub usb_link: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            name: String::new(),
            port: DEFAULT_PORT,
            download_dir: "~/Downloads/Dotlink".into(),
            clipboard_sync: true,
            ask_before_receiving: false,
            terminal: true,
            terminal_pin: String::new(),
            shell: String::new(),
            camera_device: String::new(),
            camera_size: "1920x1080".into(),
            cast_screen_off: false,
            cast_audio: true,
            cast_max_size: 0,
            theme: "system".into(),
            usb_link: true,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path();
        if !path.exists() {
            Config::default().save()?;
        }
        let text = std::fs::read_to_string(&path)?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(config_dir())?;
        std::fs::write(config_path(), toml::to_string_pretty(self)?)?;
        Ok(())
    }

    /// `camera_size` as (width, height); 1920×1080 if it doesn't parse.
    pub fn camera_size(&self) -> (u32, u32) {
        self.camera_size
            .split_once('x')
            .and_then(|(w, h)| Some((w.trim().parse().ok()?, h.trim().parse().ok()?)))
            .filter(|&(w, h): &(u32, u32)| w >= 160 && h >= 120)
            .unwrap_or((1920, 1080))
    }

    /// `~/Downloads` means the system's Downloads folder, which the user may have moved
    /// (Windows known folder, `XDG_DOWNLOAD_DIR`).
    pub fn download_dir(&self) -> PathBuf {
        let p = self.download_dir.replace('\\', "/");
        match p.strip_prefix("~/Downloads").filter(|rest| rest.is_empty() || rest.starts_with('/')) {
            Some(rest) => match dirs::download_dir() {
                Some(dir) => match rest.trim_start_matches('/') {
                    "" => dir,
                    rest => dir.join(rest),
                },
                None => expand_home(&p),
            },
            None => expand_home(&self.download_dir),
        }
    }
}

/// What a paired device is. A PC linked to another PC gets files and the clipboard only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    /// Also what devices paired before kinds existed are.
    #[default]
    Phone,
    Pc,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: DeviceKind,
    /// Send it this PC's clipboard changes as they happen. None until set: on for a phone, off
    /// for a PC (two PCs copying would keep overwriting each other). See [`Device::sends_clipboard`].
    #[serde(default)]
    pub clipboard_sync: Option<bool>,
    /// The older per-PC setting, written `false` for every phone too: only a PC's is carried
    /// over to `clipboard_sync` (in [`State::load`]), and it's never written again.
    #[serde(default, skip_serializing)]
    pub clipboard_auto: bool,
    /// Named on this PC (`dotlink rename`): the name the phone sends no longer replaces it.
    #[serde(default)]
    pub renamed: bool,
    /// SHA-256 of the bearer token; the token itself is only known to the phone.
    pub token_sha256: String,
    pub paired_at: u64,
}

impl Device {
    /// Whether this PC's clipboard changes go to it as they happen.
    pub fn sends_clipboard(&self) -> bool {
        self.clipboard_sync.unwrap_or(self.kind == DeviceKind::Phone)
    }
}

/// A PC this one linked to with its pairing link (`peer.rs`): this PC connects to it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    /// Its `server_id`.
    pub id: String,
    pub name: String,
    /// SHA-256 of its certificate, pinned.
    pub fingerprint: String,
    /// Its addresses, the one that answered last first.
    pub hosts: Vec<String>,
    pub port: u16,
    /// What this PC presents to it.
    pub token: String,
    pub paired_at: u64,
    /// Send it this PC's clipboard changes as they happen.
    #[serde(default)]
    pub clipboard_auto: bool,
    /// Named on this PC (`dotlink rename`): the name it sends no longer replaces it.
    #[serde(default)]
    pub renamed: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    /// Random id of this PC, so the phone can tell PCs apart over mDNS.
    pub server_id: String,
    /// Phones and PCs that paired with this one.
    pub devices: Vec<Device>,
    /// PCs this one linked to.
    #[serde(default)]
    pub peers: Vec<Peer>,
}

impl State {
    pub fn load() -> Result<Self> {
        let path = config_dir().join("state.json");
        let mut state: State = if path.exists() {
            serde_json::from_str(&std::fs::read_to_string(&path)?)?
        } else {
            State::default()
        };
        state.carry_over_clipboard_auto();
        if state.server_id.is_empty() {
            state.server_id = random_hex(8);
            state.save()?;
        }
        Ok(state)
    }

    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(config_dir())?;
        let path = config_dir().join("state.json");
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        set_private(&tmp);
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    /// A PC that had the older `clipboard_auto` on keeps getting the clipboard.
    fn carry_over_clipboard_auto(&mut self) {
        for d in &mut self.devices {
            if d.kind == DeviceKind::Pc && d.clipboard_sync.is_none() && d.clipboard_auto {
                d.clipboard_sync = Some(true);
            }
        }
    }

    /// Clipboard sync was turned off for every phone at once (`clipboard_sync = false`): keep
    /// it off for each device, now that it's set per device.
    pub fn clipboard_off_everywhere(&mut self) {
        for d in &mut self.devices {
            d.clipboard_sync = Some(false);
        }
        for p in &mut self.peers {
            p.clipboard_auto = false;
        }
    }

    pub fn device_for_token(&self, token: &str) -> Option<&Device> {
        let hash = sha256_hex(token.as_bytes());
        self.devices.iter().find(|d| d.token_sha256 == hash)
    }
}

/// `DOTLINK_CONFIG_DIR`, when set, replaces the config folder: a second instance next to the
/// usual one (testing two PCs on one machine).
fn config_dir_override() -> Option<PathBuf> {
    std::env::var_os("DOTLINK_CONFIG_DIR").filter(|v| !v.is_empty()).map(PathBuf::from)
}

pub fn config_dir() -> PathBuf {
    config_dir_override().unwrap_or_else(|| dirs::config_dir().unwrap_or_else(|| expand_home("~/.config")).join("dotlink"))
}

/// Empty for the usual instance. One started with `DOTLINK_CONFIG_DIR` gets `-<hash of the
/// folder>`, so its control socket and mDNS name don't clash with the usual one's.
pub fn instance_suffix() -> String {
    match config_dir_override() {
        Some(dir) => format!("-{}", &sha256_hex(dir.to_string_lossy().as_bytes())[..8]),
        None => String::new(),
    }
}

/// The log every launch appends to (a GUI program has no terminal to print to). At 1 MB it
/// moves to `dotlink.old.log` and starts again.
pub fn log_path() -> PathBuf {
    config_dir().join("dotlink.log")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(default_runtime_dir)
}

#[cfg(not(target_os = "macos"))]
fn default_runtime_dir() -> PathBuf {
    std::env::temp_dir()
}

/// The per-user temp folder (`/var/folders/…/T/`, mode 0700). `$TMPDIR` usually holds it, but
/// a process started by launchd (login items) may not have it set and get the shared `/tmp`,
/// where the CLI in Terminal wouldn't look; confstr gives the same folder either way.
#[cfg(target_os = "macos")]
fn default_runtime_dir() -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    let mut buf = vec![0u8; libc::PATH_MAX as usize];
    // SAFETY: confstr writes at most `buf.len()` bytes, NUL included, and returns the length it
    // needs (0 on error).
    let n = unsafe { libc::confstr(libc::_CS_DARWIN_USER_TEMP_DIR, buf.as_mut_ptr().cast(), buf.len()) };
    if n == 0 || n > buf.len() {
        return std::env::temp_dir();
    }
    PathBuf::from(std::ffi::OsStr::from_bytes(&buf[..n - 1]))
}

pub fn expand_home(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().unwrap_or_default().join(rest),
        None => PathBuf::from(p),
    }
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

/// Percent-encodes everything but `A-Z a-z 0-9 - _ .`.
pub fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Undoes [url_encode] (any `%XX`); None for a bad escape or text that isn't UTF-8.
pub fn url_decode(s: &str) -> Option<String> {
    let mut out = Vec::with_capacity(s.len());
    let mut bytes = s.bytes();
    while let Some(b) = bytes.next() {
        if b == b'%' {
            let hex = [bytes.next()?, bytes.next()?];
            out.push(u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?);
        } else {
            out.push(b);
        }
    }
    String::from_utf8(out).ok()
}

pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("system random number generator");
    hex::encode(buf)
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Makes a file readable by the current user only (secrets: key, tokens).
pub fn set_private(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    // TODO(windows): files under %APPDATA% are already private to the user; nothing to do
    // unless the config dir is moved somewhere shared.
    #[cfg(not(unix))]
    let _ = path;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Older versions wrote `clipboard_auto: false` for every phone: phones keep syncing, and a
    /// PC that had it on keeps it.
    #[test]
    fn clipboard_auto_carries_over() {
        let mut state: State = serde_json::from_str(
            r#"{"server_id":"x","devices":[
                {"id":"a","name":"Phone","kind":"phone","clipboard_auto":false,"token_sha256":"","paired_at":0},
                {"id":"b","name":"PC on","kind":"pc","clipboard_auto":true,"token_sha256":"","paired_at":0},
                {"id":"c","name":"PC off","kind":"pc","clipboard_auto":false,"token_sha256":"","paired_at":0}]}"#,
        )
        .unwrap();
        state.carry_over_clipboard_auto();
        let on: Vec<bool> = state.devices.iter().map(Device::sends_clipboard).collect();
        assert_eq!(on, [true, true, false]);
        assert!(!serde_json::to_string(&state).unwrap().contains("clipboard_auto"));
    }

    #[test]
    fn devices_paired_before_kinds_are_phones() {
        let dev: Device =
            serde_json::from_str(r#"{"id":"a","name":"Pixel","token_sha256":"00","paired_at":1}"#).unwrap();
        assert_eq!(dev.kind, DeviceKind::Phone);
        let dev: Device =
            serde_json::from_str(r#"{"id":"b","name":"Desk","kind":"pc","token_sha256":"00","paired_at":1}"#).unwrap();
        assert_eq!(dev.kind, DeviceKind::Pc);
    }

    #[test]
    fn url_encoding_round_trips() {
        let name = "Résumé (final) & co/../x.pdf";
        assert_eq!(url_decode(&url_encode(name)).as_deref(), Some(name));
        assert_eq!(url_decode("a%2"), None);
        assert_eq!(url_decode("%FF"), None);
    }

    #[test]
    fn download_dir_follows_system_downloads() {
        let Some(downloads) = dirs::download_dir() else { return };
        let dir = |s: &str| Config { download_dir: s.into(), ..Config::default() }.download_dir();
        assert_eq!(dir("~/Downloads/Dotlink"), downloads.join("Dotlink"));
        assert_eq!(dir("~/Downloads"), downloads);
        assert_eq!(dir("~/DownloadsX"), dirs::home_dir().unwrap().join("DownloadsX"));
    }
}
