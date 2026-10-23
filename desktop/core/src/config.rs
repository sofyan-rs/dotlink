//! Config file, paired devices and on-disk paths.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub const DEFAULT_PORT: u16 = 47390;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// TCP port the HTTPS/WebSocket server listens on.
    pub port: u16,
    /// Where files sent from the phone are saved. `~` is expanded.
    pub download_dir: String,
    /// Push PC clipboard changes to the phone automatically.
    pub clipboard_sync: bool,
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            download_dir: "~/Downloads/Dotlink".into(),
            clipboard_sync: true,
            terminal: true,
            terminal_pin: String::new(),
            shell: String::new(),
            camera_device: String::new(),
            camera_size: "1920x1080".into(),
            cast_screen_off: false,
            cast_audio: true,
            cast_max_size: 0,
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

    pub fn download_dir(&self) -> PathBuf {
        expand_home(&self.download_dir)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    /// SHA-256 of the bearer token; the token itself is only known to the phone.
    pub token_sha256: String,
    pub paired_at: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    /// Random id of this PC, so the phone can tell PCs apart over mDNS.
    pub server_id: String,
    pub devices: Vec<Device>,
}

impl State {
    pub fn load() -> Result<Self> {
        let path = config_dir().join("state.json");
        let mut state: State = if path.exists() {
            serde_json::from_str(&std::fs::read_to_string(&path)?)?
        } else {
            State::default()
        };
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

    pub fn device_for_token(&self, token: &str) -> Option<&Device> {
        let hash = sha256_hex(token.as_bytes());
        self.devices.iter().find(|d| d.token_sha256 == hash)
    }
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| expand_home("~/.config")).join("dotlink")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
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
