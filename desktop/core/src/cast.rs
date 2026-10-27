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
//! Several phones can cast at once, each in its own scrcpy window; everything here is per
//! paired device id.
//!
//! Uses the PC's `scrcpy` and `adb` when installed. Otherwise it downloads the official scrcpy
//! release (which bundles adb) into Dotlink's data folder, checked against a pinned SHA-256.

use crate::events::Event;
use crate::server::{App, PhoneInfo};
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::{IpAddr, SocketAddr};
use std::collections::HashMap;
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
    /// Per device id; Idle phones aren't in it.
    sessions: Mutex<HashMap<String, Session>>,
    /// One scrcpy download at a time, when two phones start casting together.
    installing: tokio::sync::Mutex<()>,
    runs: std::sync::atomic::AtomicU64,
}

struct Session {
    state: CastState,
    /// Stops the running scrcpy.
    stop: Option<oneshot::Sender<()>>,
    /// The adb serial scrcpy runs on, so another phone of the same model doesn't take it.
    serial: Option<String>,
    /// Which scrcpy run this is: one that ends after a newer one started leaves it alone.
    run: u64,
}

impl Cast {
    pub fn state(&self, id: &str) -> CastState {
        self.sessions.lock().unwrap().get(id).map(|s| s.state.clone()).unwrap_or(CastState::Idle)
    }

    /// Every phone that isn't Idle, by device id.
    pub fn states(&self) -> HashMap<String, CastState> {
        self.sessions.lock().unwrap().iter().map(|(id, s)| (id.clone(), s.state.clone())).collect()
    }

    /// adb serials the other phones are casting on.
    fn serials_in_use(&self, except: &str) -> Vec<String> {
        let sessions = self.sessions.lock().unwrap();
        sessions.iter().filter(|(id, _)| *id != except).filter_map(|(_, s)| s.serial.clone()).collect()
    }
}

fn set(app: &App, id: &str, state: CastState) {
    {
        let mut sessions = app.cast.sessions.lock().unwrap();
        if state == CastState::Idle {
            sessions.remove(id);
        } else {
            let session =
                sessions.entry(id.to_string()).or_insert(Session { state: CastState::Idle, stop: None, serial: None, run: 0 });
            if !matches!(state, CastState::Casting { .. }) {
                session.serial = None;
            }
            session.state = state;
        }
    }
    app.emit(Event::Cast);
}

/// Sets `state` only while attempt `run` still owns the phone's cast: after Stop (or a newer
/// Cast) it's left alone. False when it no longer does.
fn update(app: &App, id: &str, run: u64, state: CastState) -> bool {
    let owned = app.cast.sessions.lock().unwrap().get(id).is_some_and(|s| s.run == run);
    if owned {
        set(app, id, state);
    }
    owned
}

/// The phone to cast: `id` (device id or name), or the first one online. Returns its device id,
/// name and what it told us about itself.
fn pick(app: &App, id: Option<&str>) -> Result<(String, String, PhoneInfo)> {
    let online = app.online_devices();
    let state = app.state.lock().unwrap();
    let dev = state
        .devices
        .iter()
        .filter(|d| d.kind == crate::config::DeviceKind::Phone && online.contains(&d.name))
        .find(|d| id.is_none_or(|id| d.id == id || d.name.eq_ignore_ascii_case(id)))
        .ok_or_else(|| match id {
            Some(id) => anyhow!("no connected phone called \"{id}\""),
            None => anyhow!("no phone connected"),
        })?;
    let info = app.phones.lock().unwrap().get(&dev.id).cloned().unwrap_or_default();
    Ok((dev.id.clone(), dev.name.clone(), info))
}

/// Starts casting. Returns once scrcpy is up ("casting"), Stop was pressed meanwhile
/// ("cancelled"), or what's missing is known (the error); the outcome is also in [Cast::state].
pub async fn start(app: &Arc<App>, id: Option<&str>) -> Result<&'static str> {
    let (id, name, info) = pick(app, id)?;
    let run = app.cast.runs.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    {
        // Check and claim under one lock: two quick clicks start one scrcpy.
        let mut sessions = app.cast.sessions.lock().unwrap();
        if sessions.get(&id).is_some_and(|s| {
            matches!(s.state, CastState::Casting { .. } | CastState::Connecting { .. } | CastState::Installing { .. })
        }) {
            bail!("already casting {name}");
        }
        sessions.insert(
            id.clone(),
            Session { state: CastState::Connecting { device: name.clone() }, stop: None, serial: None, run },
        );
    }
    app.emit(Event::Cast);
    // Stop while installing or connecting removes the session: every step below then finds
    // it gone and gives up quietly.
    const CANCELLED: Result<&str> = Ok("cancelled");
    if find_tool("scrcpy").is_none() {
        if !update(app, &id, run, CastState::Installing { device: name.clone() }) {
            return CANCELLED;
        }
        let _one = app.cast.installing.lock().await;
        // Another phone may have installed it while we waited.
        if find_tool("scrcpy").is_none()
            && let Err(e) = install().await
        {
            let error = format!("couldn't install scrcpy: {e:#}");
            return if update(app, &id, run, CastState::Failed { device: name, error }) { Err(e) } else { CANCELLED };
        }
    }
    if !update(app, &id, run, CastState::Connecting { device: name.clone() }) {
        return CANCELLED;
    }
    let taken = app.cast.serials_in_use(&id);
    let connected = connect(app, &id, &name, &info, &taken).await;
    if !update(app, &id, run, CastState::Connecting { device: name.clone() }) {
        return CANCELLED;
    }
    match connected {
        Ok((serial, usb)) => match launch(app, &id, &name, &serial, usb, run).await {
            Ok(true) => Ok("casting"),
            Ok(false) => CANCELLED,
            Err(e) => {
                let error = format!("{e:#}");
                if update(app, &id, run, CastState::Failed { device: name, error }) { Err(e) } else { CANCELLED }
            }
        },
        Err(Missing::WifiDebugging) => {
            // The phone shows a notification that opens the setting.
            app.send_control_to(Some(&id), json!({ "t": "adb_wifi" }));
            update(app, &id, run, CastState::NeedsWifiDebugging { device: name.clone() });
            bail!("turn on Wireless debugging on {name} (it shows a notification), or plug it in over USB")
        }
        Err(Missing::Pairing) => {
            update(app, &id, run, CastState::NeedsPairing { device: name.clone() });
            bail!("pair {name} once: Wireless debugging → Pair device with pairing code, then enter the code")
        }
        Err(Missing::Other(e)) => {
            let error = format!("{e:#}");
            if update(app, &id, run, CastState::Failed { device: name, error }) { Err(e) } else { CANCELLED }
        }
    }
}

/// Stops casting `id` (device id or name), or every phone for None.
pub fn stop(app: &App, id: Option<&str>) {
    let ids: Vec<String> = match id {
        None => app.cast.states().into_keys().collect(),
        Some(id) => {
            let state = app.state.lock().unwrap();
            let dev = state.devices.iter().find(|d| d.id == id || d.name.eq_ignore_ascii_case(id));
            vec![dev.map_or_else(|| id.to_string(), |d| d.id.clone())]
        }
    };
    for id in ids {
        let stop = app.cast.sessions.lock().unwrap().get_mut(&id).and_then(|s| s.stop.take());
        match stop {
            // The scrcpy task sets Idle once scrcpy is gone.
            Some(stop) => drop(stop.send(())),
            // Failed or waiting for the user: Stop dismisses it.
            None => set(app, &id, CastState::Idle),
        }
    }
}

/// Pairs this PC's adb with the phone's Wireless debugging, using the code it shows under
/// "Pair device with pairing code".
pub async fn pair(app: &Arc<App>, id: Option<&str>, code: &str) -> Result<()> {
    let code: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
    if code.len() != 6 {
        bail!("the pairing code has 6 digits");
    }
    let (id, name, info) = pick(app, id)?;
    let ip = info.ip.context("don't know the phone's address yet")?;
    let addr = browse("_adb-tls-pairing._tcp.local.", ip, Duration::from_secs(4)).await.ok_or_else(|| {
        anyhow!("can't see {name} waiting to pair: open Wireless debugging → Pair device with pairing code on it")
    })?;
    let out = adb(&["pair", &addr.to_string(), &code]).await?;
    if !out.to_lowercase().contains("success") {
        bail!("{}", last_line(&out).unwrap_or("pairing failed"));
    }
    set(app, &id, CastState::Idle);
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

/// An adb serial for the phone, and whether it's over USB. `taken`: serials other phones are
/// casting on.
async fn connect(
    app: &App,
    id: &str,
    name: &str,
    info: &PhoneInfo,
    taken: &[String],
) -> Result<(String, bool), Missing> {
    let model = adb_model(&info.model);
    let devices: Vec<AdbDevice> = adb_devices().await?.into_iter().filter(|d| !taken.contains(&d.serial)).collect();
    // Another connected phone of the same model: adb's model can't tell them apart, so only
    // the phone's IP (Wi-Fi) identifies it.
    let twin = !model.is_empty() && {
        let phones = app.phones.lock().unwrap();
        app.online_ids().iter().any(|other| other != id && phones.get(other).is_some_and(|p| adb_model(&p.model) == model))
    };
    // Matched by model; a phone that hasn't told us its model (older app) can still use the
    // only device adb sees.
    let only_one = devices.len() == 1;
    let mine = |d: &&AdbDevice| if model.is_empty() { only_one } else { d.model == model };
    if let Some(d) = devices.iter().filter(mine).filter(|_| !twin).max_by_key(|d| d.usb) {
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
        if twin {
            return Err(Missing::Other(anyhow!(
                "another phone of the same model is connected, so {name} can only cast over Wi-Fi: turn on its Wireless debugging"
            )));
        }
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
/// Runs scrcpy as attempt `run`. False when Stop came first (scrcpy is then ended at once).
async fn launch(app: &Arc<App>, id: &str, name: &str, serial: &str, usb: bool, run: u64) -> Result<bool> {
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
    // Hand over the stop under the same lock that checks for Stop, so neither is missed.
    let owned = match app.cast.sessions.lock().unwrap().get_mut(id) {
        Some(session) if session.run == run => {
            session.state = CastState::Casting { device: name.to_string(), usb };
            session.stop = Some(stop_tx);
            session.serial = Some(serial.to_string());
            true
        }
        _ => false,
    };
    if !owned {
        // kill_on_drop (and on Windows the job object) end it.
        tracing::info!("cast of {name} cancelled");
        return Ok(false);
    }
    app.emit(Event::Cast);
    tracing::info!("casting {name} ({serial})");
    let app = app.clone();
    let (id, name) = (id.to_string(), name.to_string());
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
        tracing::info!("cast of {name} ended");
        // Stopped and cast again meanwhile: the new run owns the state.
        if app.cast.sessions.lock().unwrap().get(&id).is_none_or(|s| s.run != run) {
            return;
        }
        // Closing the window is a normal end; failing within seconds is an error to show.
        let failed = status.is_some_and(|s| !s.success()) && started.elapsed() < Duration::from_secs(5);
        let error = err.lines().rev().find(|l| l.contains("ERROR")).map(|l| l.trim().to_string());
        set(
            &app,
            &id,
            match (failed, error) {
                (true, Some(error)) => CastState::Failed { device: name, error },
                (true, None) => CastState::Failed { device: name, error: "scrcpy stopped".into() },
                _ => CastState::Idle,
            },
        );
    });
    Ok(true)
}

/// Serials of the phones plugged in over USB that allow debugging from this PC.
pub(crate) async fn usb_serials() -> Result<Vec<String>> {
    Ok(usb_phones().await?.into_iter().map(|(serial, _)| serial).collect())
}

/// `(serial, model)` of the phones plugged in over USB that allow debugging from this PC.
pub(crate) async fn usb_phones() -> Result<Vec<(String, String)>> {
    Ok(adb_devices()
        .await?
        .into_iter()
        .filter(|d| d.usb && d.state == "device")
        .map(|d| (d.serial, d.model.replace('_', " ")))
        .collect())
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
            // Windows' adb prints no `usb:` field. Network serials are `ip:port` or the mDNS
            // name (`adb-…._adb-tls-connect._tcp`); anything else is plugged in.
            usb |= !serial.contains(':') && !serial.contains("._adb-tls") && !serial.starts_with("emulator-");
            Some(AdbDevice { serial, state, model, usb })
        })
        .collect())
}

/// `Build.MODEL` the way adb prints it.
fn adb_model(model: &str) -> String {
    model.trim().replace(' ', "_")
}

/// Runs adb and returns stdout + stderr.
pub(crate) async fn adb(args: &[&str]) -> Result<String> {
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
    // A Mac app opened from Finder gets a PATH without Homebrew's folders.
    if cfg!(target_os = "macos")
        && let Some(found) = ["/opt/homebrew/bin", "/usr/local/bin"]
            .iter()
            .map(|d| std::path::Path::new(d).join(&exe))
            .find(|p| p.is_file())
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
