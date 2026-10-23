//! Cast: the phone's screen in a window on the PC, through scrcpy over ADB (USB or Wi-Fi).
//!
//! scrcpy does the streaming and control; this finds the phone for it:
//! 1. Plugged in over USB (and allowed): use that.
//! 2. Otherwise Wireless debugging (Android 11+): look up its port over mDNS
//!    (`_adb-tls-connect._tcp`, matched to the IP the phone's Dotlink connection comes from),
//!    `adb connect`, then start scrcpy.
//! 3. Wireless debugging off: ask the phone to show a notification that opens its setting.
//! 4. Not paired with this PC's adb yet: pair once with the 6-digit code from the phone
//!    (the pairing port is found over mDNS too, `_adb-tls-pairing._tcp`).
//!
//! Uses the PC's `scrcpy` and `adb` when installed. Otherwise it downloads the official scrcpy
//! release (which bundles adb) into Dotlink's data folder, checked against a pinned SHA-256.

use crate::events::Event;
use crate::server::{App, PhoneInfo};
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::oneshot;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CastState {
    Idle,
    /// Downloading scrcpy (not installed on this PC).
    Installing {
        device: String,
    },
    Connecting {
        device: String,
    },
    Casting {
        device: String,
        usb: bool,
    },
    /// Wireless debugging is off on the phone and it isn't plugged in.
    NeedsWifiDebugging {
        device: String,
    },
    /// The phone's Wireless debugging doesn't know this PC yet.
    NeedsPairing {
        device: String,
    },
    Failed {
        device: String,
        error: String,
    },
}

#[derive(Default)]
pub struct Cast {
    state: Mutex<Option<CastState>>,
    /// Stops the running scrcpy.
    stop: Mutex<Option<oneshot::Sender<()>>>,
}

impl Cast {
    pub fn state(&self) -> CastState {
        self.state.lock().unwrap().clone().unwrap_or(CastState::Idle)
    }
}

fn set(app: &App, state: CastState) {
    *app.cast.state.lock().unwrap() = Some(state);
    app.emit(Event::Cast);
}

/// The phone to cast: `id` (device id or name), or the first one online.
fn pick(app: &App, id: Option<&str>) -> Result<(String, PhoneInfo)> {
    let online = app.online_devices();
    let state = app.state.lock().unwrap();
    let dev = state
        .devices
        .iter()
        .filter(|d| online.contains(&d.name))
        .find(|d| id.is_none_or(|id| d.id == id || d.name.eq_ignore_ascii_case(id)))
        .ok_or_else(|| anyhow!("no phone connected"))?;
    let info = app.phones.lock().unwrap().get(&dev.id).cloned().unwrap_or_default();
    Ok((dev.name.clone(), info))
}

/// Starts casting. Returns once scrcpy is up (or what's missing is known); the outcome is
/// also in [Cast::state].
pub async fn start(app: &Arc<App>, id: Option<&str>) -> Result<()> {
    if matches!(
        app.cast.state(),
        CastState::Casting { .. } | CastState::Connecting { .. } | CastState::Installing { .. }
    ) {
        bail!("already casting");
    }
    let (name, info) = pick(app, id)?;
    if find_tool("scrcpy").is_none() {
        set(app, CastState::Installing { device: name.clone() });
        if let Err(e) = install().await {
            set(app, CastState::Failed { device: name, error: format!("couldn't install scrcpy: {e:#}") });
            return Err(e);
        }
    }
    set(app, CastState::Connecting { device: name.clone() });
    match connect(&name, &info).await {
        Ok((serial, usb)) => match launch(app, &name, &serial, usb).await {
            Ok(()) => Ok(()),
            Err(e) => {
                set(app, CastState::Failed { device: name, error: format!("{e:#}") });
                Err(e)
            }
        },
        Err(Missing::WifiDebugging) => {
            // The phone shows a notification that opens the setting.
            app.send_control(json!({ "t": "adb_wifi" }));
            set(app, CastState::NeedsWifiDebugging { device: name.clone() });
            bail!("turn on Wireless debugging on {name} (it shows a notification), or plug it in over USB")
        }
        Err(Missing::Pairing) => {
            set(app, CastState::NeedsPairing { device: name.clone() });
            bail!("pair {name} once: Wireless debugging → Pair device with pairing code, then enter the code")
        }
        Err(Missing::Other(e)) => {
            set(app, CastState::Failed { device: name, error: format!("{e:#}") });
            Err(e)
        }
    }
}

pub fn stop(app: &App) {
    if let Some(stop) = app.cast.stop.lock().unwrap().take() {
        let _ = stop.send(());
    }
    if !matches!(app.cast.state(), CastState::Casting { .. }) {
        set(app, CastState::Idle);
    }
}

/// Pairs this PC's adb with the phone's Wireless debugging, using the code it shows under
/// "Pair device with pairing code".
pub async fn pair(app: &Arc<App>, id: Option<&str>, code: &str) -> Result<()> {
    let code: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
    if code.len() != 6 {
        bail!("the pairing code has 6 digits");
    }
    let (name, info) = pick(app, id)?;
    let ip = info.ip.context("don't know the phone's address yet")?;
    let addr = browse("_adb-tls-pairing._tcp.local.", ip, Duration::from_secs(4)).await.ok_or_else(|| {
        anyhow!("can't see {name} waiting to pair: open Wireless debugging → Pair device with pairing code on it")
    })?;
    let out = adb(&["pair", &addr.to_string(), &code]).await?;
    if !out.to_lowercase().contains("success") {
        bail!("{}", last_line(&out).unwrap_or("pairing failed"));
    }
    set(app, CastState::Idle);
    Ok(())
}

enum Missing {
    WifiDebugging,
    Pairing,
    Other(anyhow::Error),
}

impl From<anyhow::Error> for Missing {
    fn from(e: anyhow::Error) -> Self {
        Missing::Other(e)
    }
}

/// An adb serial for the phone, and whether it's over USB.
async fn connect(name: &str, info: &PhoneInfo) -> Result<(String, bool), Missing> {
    let model = adb_model(&info.model);
    let devices = adb_devices().await?;
    // Matched by model; a phone that hasn't told us its model (older app) can still use the
    // only device adb sees.
    let only_one = devices.len() == 1;
    let mine = |d: &&AdbDevice| if model.is_empty() { only_one } else { d.model == model };
    if let Some(d) = devices.iter().filter(mine).max_by_key(|d| d.usb) {
        match d.state.as_str() {
            "device" => return Ok((d.serial.clone(), d.usb)),
            "unauthorized" => {
                return Err(Missing::Other(anyhow!("allow USB debugging on {name} (a dialog on the phone)")));
            }
            _ => {}
        }
    }
    let Some(ip) = info.ip else { return Err(Missing::Other(anyhow!("don't know the phone's address yet"))) };
    // Already connected over Wi-Fi under its ip:port serial (model unknown, old app).
    if let Some(d) = devices.iter().find(|d| d.state == "device" && d.serial.starts_with(&format!("{ip}:"))) {
        return Ok((d.serial.clone(), false));
    }
    let Some(addr) = browse("_adb-tls-connect._tcp.local.", ip, Duration::from_secs(3)).await else {
        // Not advertised: off, unless the phone says it's on (then mDNS is blocked).
        if info.adb_wifi {
            return Err(Missing::Other(anyhow!(
                "Wireless debugging is on but {name} can't be found on the network (mDNS, UDP 5353 blocked?)"
            )));
        }
        return Err(Missing::WifiDebugging);
    };
    let out = adb(&["connect", &addr.to_string()]).await?;
    let lower = out.to_lowercase();
    if lower.contains("connected to") {
        return Ok((addr.to_string(), false));
    }
    // An unpaired PC is refused at the TLS handshake.
    if lower.contains("authenticat") || lower.contains("failed to connect") || lower.contains("refused") {
        return Err(Missing::Pairing);
    }
    Err(Missing::Other(anyhow!("{}", last_line(&out).unwrap_or("adb connect failed"))))
}

/// Runs scrcpy for `serial` until it exits or [stop] is called.
async fn launch(app: &Arc<App>, name: &str, serial: &str, usb: bool) -> Result<()> {
    let config = app.config();
    let scrcpy = find_tool("scrcpy").context("scrcpy is not installed (see Setup)")?;
    let mut cmd = command(scrcpy);
    cmd.arg("--serial").arg(serial).arg("--window-title").arg(format!("{name} · Dotlink"));
    if config.cast_screen_off {
        cmd.arg("--turn-screen-off");
    }
    if !config.cast_audio {
        cmd.arg("--no-audio");
    }
    if config.cast_max_size > 0 {
        cmd.arg("--max-size").arg(config.cast_max_size.to_string());
    }
    if usb {
        cmd.arg("--stay-awake");
    }
    // The same adb as ours: two adb versions keep killing each other's server.
    if let Some(adb) = find_tool("adb") {
        cmd.env("ADB", adb);
    }
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true).spawn()?;
    let mut stderr = child.stderr.take().unwrap();
    // Ends scrcpy's whole process tree with the cast (see ProcessTree).
    #[cfg(windows)]
    let tree = child.id().and_then(crate::platform::ProcessTree::new);
    let (stop_tx, stop_rx) = oneshot::channel();
    *app.cast.stop.lock().unwrap() = Some(stop_tx);
    set(app, CastState::Casting { device: name.to_string(), usb });
    tracing::info!("casting {name} ({serial})");
    let app = app.clone();
    let name = name.to_string();
    tokio::spawn(async move {
        let started = Instant::now();
        let mut err = String::new();
        let status = tokio::select! {
            status = async {
                let _ = stderr.read_to_string(&mut err).await;
                child.wait().await
            } => status.ok(),
            _ = stop_rx => {
                let _ = child.kill().await;
                None
            }
        };
        #[cfg(windows)]
        drop(tree);
        app.cast.stop.lock().unwrap().take();
        tracing::info!("cast of {name} ended");
        // Closing the window is a normal end; failing within seconds is an error to show.
        let failed = status.is_some_and(|s| !s.success()) && started.elapsed() < Duration::from_secs(5);
        let error = err.lines().rev().find(|l| l.contains("ERROR")).map(|l| l.trim().to_string());
        set(
            &app,
            match (failed, error) {
                (true, Some(error)) => CastState::Failed { device: name, error },
                (true, None) => CastState::Failed { device: name, error: "scrcpy stopped".into() },
                _ => CastState::Idle,
            },
        );
    });
    Ok(())
}

struct AdbDevice {
    serial: String,
    /// "device", "unauthorized", "offline"…
    state: String,
    model: String,
    usb: bool,
}

/// `adb devices -l`: `SERIAL  device usb:1-4 product:X model:RMX5085 device:Y transport_id:1`.
async fn adb_devices() -> Result<Vec<AdbDevice>> {
    let out = adb(&["devices", "-l"]).await?;
    Ok(out
        .lines()
        .skip_while(|l| !l.starts_with("List of devices"))
        .skip(1)
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let serial = parts.next()?.to_string();
            let state = parts.next()?.to_string();
            let mut model = String::new();
            let mut usb = false;
            for p in parts {
                if let Some(m) = p.strip_prefix("model:") {
                    model = m.to_string();
                }
                usb |= p.starts_with("usb:");
            }
            Some(AdbDevice { serial, state, model, usb })
        })
        .collect())
}

/// `Build.MODEL` the way adb prints it.
fn adb_model(model: &str) -> String {
    model.trim().replace(' ', "_")
}

/// Runs adb and returns stdout + stderr.
async fn adb(args: &[&str]) -> Result<String> {
    let adb = find_tool("adb").context("adb is not installed (see Setup)")?;
    let out = tokio::time::timeout(Duration::from_secs(20), command(adb).args(args).stdin(Stdio::null()).output())
        .await
        .context("adb took too long")??;
    Ok(format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

/// A command for adb or scrcpy. On Windows without a console window: they are console
/// programs (scrcpy opens its own window besides).
fn command(program: PathBuf) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd
}

fn last_line(s: &str) -> Option<&str> {
    s.lines().rev().map(str::trim).find(|l| !l.is_empty())
}

/// An mDNS service of type `ty` on the host `ip`, within `wait`.
async fn browse(ty: &'static str, ip: IpAddr, wait: Duration) -> Option<SocketAddr> {
    tokio::task::spawn_blocking(move || {
        let mdns = mdns_sd::ServiceDaemon::new().ok()?;
        let rx = mdns.browse(ty).ok()?;
        let deadline = Instant::now() + wait;
        let found = loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break None;
            }
            match rx.recv_timeout(left) {
                Ok(mdns_sd::ServiceEvent::ServiceResolved(info)) => {
                    if info.get_addresses().iter().any(|a| a.to_ip_addr() == ip) {
                        break Some(SocketAddr::new(ip, info.get_port()));
                    }
                }
                Ok(_) => {}
                Err(_) => break None,
            }
        };
        let _ = mdns.shutdown();
        found
    })
    .await
    .ok()
    .flatten()
}

// ---------- scrcpy download ----------

/// The scrcpy release Dotlink downloads, and the SHA-256 of each build (from the release's
/// SHA256SUMS.txt). To update: bump the version and copy the new sums.
const SCRCPY_VERSION: &str = "v5.0.1";

/// (asset name, sha256) for this OS and CPU; None where scrcpy publishes no build.
fn scrcpy_asset() -> Option<(String, &'static str)> {
    let (target, sha) = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => ("linux-x86_64", "9f969d30cc574816077edecda65719c1c70b0aee1df1ba5fa00779ac07b8fd6e"),
        ("macos", "aarch64") => ("macos-aarch64", "33611e51977a8289e2e124b220f727895181ef2eb9060e0987a4dc2db9cf0d6b"),
        ("macos", "x86_64") => ("macos-x86_64", "31a5467a9e907f162b093267cbbab1276b15c6e709c2ea8159566a8d165beb50"),
        ("windows", "x86_64") => ("win64", "b12a2c4ee8be317422451fc7dcf8ee20a71b5ea7ef9ad73ddd825a25316227a5"),
        ("windows", "aarch64") => ("winarm64", "16c3fc2068df64946f670e3504b4752073ebae47a416a45b46e79a5ab19c689b"),
        _ => return None,
    };
    let ext = if cfg!(windows) { "zip" } else { "tar.gz" };
    Some((format!("scrcpy-{target}-{SCRCPY_VERSION}.{ext}"), sha))
}

/// Where the downloaded scrcpy (and its adb) live.
fn managed_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("dotlink/scrcpy").join(SCRCPY_VERSION)
}

/// Dotlink can download scrcpy on this OS.
pub fn can_install() -> bool {
    scrcpy_asset().is_some()
}

/// Downloads and unpacks scrcpy into [managed_dir], checking its SHA-256.
pub async fn install() -> Result<()> {
    let (asset, sha) = scrcpy_asset()
        .context("scrcpy has no ready-made build for this system; install it with your package manager")?;
    let url = format!("https://github.com/Genymobile/scrcpy/releases/download/{SCRCPY_VERSION}/{asset}");
    tracing::info!("downloading {url}");
    tokio::task::spawn_blocking(move || -> Result<()> {
        use sha2::{Digest, Sha256};
        use std::io::Read;
        let agent: ureq::Agent =
            ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(300))).build().into();
        let mut body = agent.get(&url).call().context("download")?.into_body();
        let mut data = Vec::new();
        body.as_reader().take(100 << 20).read_to_end(&mut data)?;
        let got = hex::encode(Sha256::digest(&data));
        if got != sha {
            bail!("the download doesn't match its checksum (got {got})");
        }
        let dir = managed_dir();
        let tmp = dir.with_extension("part");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp)?;
        if asset.ends_with(".zip") {
            zip::ZipArchive::new(std::io::Cursor::new(&data[..]))?.extract(&tmp)?;
        } else {
            tar::Archive::new(flate2::read::GzDecoder::new(&data[..])).unpack(&tmp)?;
        }
        // The archive holds one folder (scrcpy-linux-x86_64-v5.0.1/); keep its contents.
        let scrcpy = if cfg!(windows) { "scrcpy.exe" } else { "scrcpy" };
        let inner = std::fs::read_dir(&tmp)?
            .flatten()
            .map(|e| e.path())
            .find(|p| p.join(scrcpy).is_file())
            .context("unexpected archive layout")?;
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::rename(&inner, &dir)?;
        let _ = std::fs::remove_dir_all(&tmp);
        Ok(())
    })
    .await??;
    tracing::info!("scrcpy installed in {}", managed_dir().display());
    Ok(())
}

/// `name` in PATH, the Android SDK's platform-tools (adb), or the scrcpy Dotlink downloaded
/// (scrcpy and its adb).
pub fn find_tool(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    if let Some(found) = std::env::var_os("PATH")
        .and_then(|paths| std::env::split_paths(&paths).map(|d| d.join(&exe)).find(|p| p.is_file()))
    {
        return Some(found);
    }
    if name == "adb" {
        // Android Studio's default SDK location on each OS.
        let sdk = std::env::var_os("ANDROID_HOME").map(PathBuf::from).or_else(|| {
            if cfg!(windows) {
                dirs::data_local_dir().map(|d| d.join("Android/Sdk"))
            } else if cfg!(target_os = "macos") {
                dirs::home_dir().map(|h| h.join("Library/Android/sdk"))
            } else {
                dirs::home_dir().map(|h| h.join("Android/Sdk"))
            }
        });
        if let Some(adb) = sdk.map(|s| s.join("platform-tools").join(&exe)).filter(|p| p.is_file()) {
            return Some(adb);
        }
    }
    Some(managed_dir().join(&exe)).filter(|p| p.is_file())
}
