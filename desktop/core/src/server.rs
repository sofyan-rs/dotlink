//! Shared daemon state and the HTTPS / WebSocket API the phone talks to.

use crate::config::{now_secs, random_hex, sha256_hex, url_encode, Config, Device, State};
use crate::events::{Event, Transfer, TransferState, UiRequest};
use crate::input::InputMsg;
use crate::platform::{self, clipboard, find_camera, notify, CameraHub, Input, MicSink};
use crate::term::Session;
use anyhow::{anyhow, bail, Result};
use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, DefaultBodyLimit, FromRequestParts, Path, Query, State as AxState};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::sync::{broadcast, mpsc, oneshot, Notify};

const PAIRING_TTL: Duration = Duration::from_secs(300);
const OFFER_TTL: Duration = Duration::from_secs(3600);
/// Transfers kept for the app's list.
const TRANSFER_HISTORY: usize = 100;

struct Pairing {
    secret: String,
    expires: Instant,
    done: Option<oneshot::Sender<String>>,
}

/// What a connected phone told us about itself (its `info` message), for casting.
#[derive(Debug, Clone, Default)]
pub struct PhoneInfo {
    /// `Build.MODEL`, the same as `model:` in `adb devices -l` (spaces as `_`).
    pub model: String,
    /// Wireless debugging is on.
    pub adb_wifi: bool,
    /// Developer options are on.
    pub dev_options: bool,
    /// Where its control connection comes from (its Wi-Fi address; USB links don't change it).
    pub ip: Option<IpAddr>,
    /// Control connections over the USB cable (`adb reverse`, seen as 127.0.0.1).
    pub usb: usize,
    /// Battery percent and whether it's charging, from its `battery` message (newer apps).
    pub battery: Option<(u8, bool)>,
}

/// Where a file or clipboard goes: one phone, or every connected phone.
pub struct Target {
    /// None for every connected phone.
    pub id: Option<String>,
    /// The phones' names, comma-separated.
    pub names: String,
    /// The phones: (device id, name).
    pub phones: Vec<(String, String)>,
}

/// Transfer id of an offer: one transfer per phone when it went to every phone.
fn transfer_id(offer: &str, every_phone: bool, device: &str) -> String {
    if every_phone { format!("{offer}/{device}") } else { offer.to_string() }
}

struct Offer {
    /// Only this device may download it; None for every phone.
    device: Option<String>,
    path: PathBuf,
    name: String,
    size: u64,
    created: Instant,
}

pub struct App {
    config: Mutex<Config>,
    /// The port actually listened on (a changed `config.port` applies after a restart).
    pub port: u16,
    pub fingerprint: String,
    pub hostname: String,
    pub state: Mutex<State>,
    pairing: Mutex<Option<Pairing>>,
    /// One-time codes handed out over USB (`POST /pair/usb`), one per request, so several
    /// phones can pair at once without touching the code the window shows: (secret, expiry).
    usb_pairings: Mutex<Vec<(String, Instant)>>,
    /// Messages for the phones: (device id, or None for every phone; JSON text).
    control_tx: broadcast::Sender<(Option<String>, String)>,
    /// Device ids just unpaired: their control connections close, so the phone reconnects,
    /// is refused (401) and forgets this PC right away.
    revoked_tx: broadcast::Sender<String>,
    /// Device id → number of open control connections.
    online: Mutex<HashMap<String, usize>>,
    offers: Mutex<HashMap<String, Offer>>,
    last_clip: Mutex<String>,
    input: Mutex<Option<Input>>,
    /// Device id → its shell sessions by id (the tabs in the app's Shell screen).
    terms: Mutex<HashMap<String, BTreeMap<u32, Arc<Session>>>>,
    /// Feeds "Dotlink Camera" (phone frames or the "camera off" placeholder).
    pub camera: CameraHub,
    /// For the desktop window and `dotlink watch`-style clients.
    pub events: broadcast::Sender<Event>,
    transfers: Mutex<VecDeque<Transfer>>,
    /// Device id → what the phone told us about itself, and where it connects from.
    pub phones: Mutex<HashMap<String, PhoneInfo>>,
    /// Casting the phone screen with scrcpy.
    pub cast: crate::cast::Cast,
    /// "camera" / "mic" → the phone streaming it right now.
    streams: Mutex<BTreeMap<&'static str, String>>,
    /// The window of the app this runs in; None when headless (`dotlink daemon`).
    ui: Mutex<Option<mpsc::UnboundedSender<UiRequest>>>,
    /// Stops the service (`dotlink quit`, Quit in the tray).
    pub shutdown: Notify,
}

impl App {
    pub fn new(config: Config, state: State, fingerprint: String) -> Arc<Self> {
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_else(|_| std::env::consts::OS.into());
        let camera = CameraHub::start(config.camera_device.clone(), config.camera_size());
        update_share_menu(&state);
        Arc::new(Self {
            port: config.port,
            config: Mutex::new(config),
            fingerprint,
            hostname,
            state: Mutex::new(state),
            pairing: Mutex::new(None),
            usb_pairings: Mutex::new(Vec::new()),
            control_tx: broadcast::channel(64).0,
            revoked_tx: broadcast::channel(16).0,
            online: Mutex::new(HashMap::new()),
            offers: Mutex::new(HashMap::new()),
            last_clip: Mutex::new(String::new()),
            input: Mutex::new(None),
            terms: Mutex::new(HashMap::new()),
            camera,
            events: broadcast::channel(256).0,
            transfers: Mutex::new(VecDeque::new()),
            streams: Mutex::new(BTreeMap::new()),
            phones: Mutex::new(HashMap::new()),
            cast: crate::cast::Cast::default(),
            ui: Mutex::new(None),
            shutdown: Notify::new(),
        })
    }

    pub fn config(&self) -> Config {
        self.config.lock().unwrap().clone()
    }

    /// What the phones call this PC: `config.name`, or the hostname.
    pub fn name(&self) -> String {
        let name = self.config.lock().unwrap().name.trim().to_string();
        if name.is_empty() { self.hostname.clone() } else { name }
    }

    fn hello(&self) -> Value {
        // `os` ("linux", "windows", "macos") lets the phone label the keys (Super, Win, Cmd).
        json!({ "t": "hello", "name": self.name(), "terminal": self.config().terminal, "os": std::env::consts::OS })
    }

    /// Saves and applies a new config. Returns true when some of it applies only after a
    /// restart (port, camera).
    pub fn set_config(&self, new: Config) -> Result<bool> {
        new.save()?;
        let mut config = self.config.lock().unwrap();
        let restart = new.port != self.port
            || new.camera_device != config.camera_device
            || new.camera_size != config.camera_size;
        let renamed = new.name != config.name;
        *config = new;
        drop(config);
        if renamed {
            // The phones take the new name from hello.
            self.send_control(self.hello());
        }
        self.emit(Event::Config);
        Ok(restart)
    }

    pub fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }

    /// Lets the service talk to the window it runs with.
    pub fn attach_ui(&self, tx: mpsc::UnboundedSender<UiRequest>) {
        *self.ui.lock().unwrap() = Some(tx);
    }

    /// False when there is no window (headless).
    pub fn request_ui(&self, req: UiRequest) -> bool {
        self.ui.lock().unwrap().as_ref().is_some_and(|tx| tx.send(req).is_ok())
    }

    /// Adds or updates a transfer in the history and tells the window.
    fn put_transfer(&self, t: Transfer) {
        let mut list = self.transfers.lock().unwrap();
        match list.iter_mut().find(|x| x.id == t.id) {
            Some(x) => *x = t.clone(),
            None => {
                list.push_front(t.clone());
                list.truncate(TRANSFER_HISTORY);
            }
        }
        drop(list);
        self.emit(Event::Transfer(t));
    }

    pub fn devices_json(&self) -> Vec<Value> {
        let online = self.online.lock().unwrap();
        let state = self.state.lock().unwrap();
        let phones = self.phones.lock().unwrap();
        state
            .devices
            .iter()
            .map(|d| {
                json!({
                    "id": d.id,
                    "name": d.name,
                    "online": online.get(&d.id).copied().unwrap_or(0) > 0,
                    "paired_at": d.paired_at,
                    "model": phones.get(&d.id).map(|p| p.model.clone()),
                    "adb_wifi": phones.get(&d.id).map(|p| p.adb_wifi),
                    "usb": phones.get(&d.id).is_some_and(|p| p.usb > 0),
                    // Only while connected: an old reading would look current.
                    "battery": phones
                        .get(&d.id)
                        .and_then(|p| p.battery)
                        .filter(|_| online.get(&d.id).copied().unwrap_or(0) > 0)
                        .map(|(level, charging)| json!({ "level": level, "charging": charging })),
                })
            })
            .collect()
    }

    /// Everything the window shows, in one reply.
    pub fn status(&self) -> Value {
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "hostname": self.hostname,
            "name": self.name(),
            "port": self.port,
            "fingerprint": self.fingerprint,
            "addresses": local_ips(),
            "devices": self.devices_json(),
            "transfers": *self.transfers.lock().unwrap(),
            "config": self.config(),
            "camera": self.streams.lock().unwrap().get("camera"),
            "mic": self.streams.lock().unwrap().get("mic"),
            "has_ui": self.ui.lock().unwrap().is_some(),
            "casts": self.cast.states(),
        })
    }

    /// Text copied on this PC: send it to the phones (when clipboard sync is on).
    pub fn clip_from_desktop(&self, text: String) {
        if text.is_empty() || !self.config.lock().unwrap().clipboard_sync || !self.remember_clip(&text) {
            return;
        }
        let online = self.online_devices();
        if online.is_empty() {
            return;
        }
        tracing::info!("clipboard to phone ({} chars)", text.chars().count());
        let chars = text.chars().count();
        self.send_control(json!({ "t": "clip", "text": text }));
        self.emit(Event::Clipboard { to_phone: true, device: online.join(", "), chars });
    }

    /// Asks the phones to start or stop streaming the camera or mic (`kind`: "camera" | "mic").
    /// Starting may need a tap on the phone when its app is in the background; the phone then
    /// answers with a `media` message, which becomes [Event::MediaPrompt].
    /// There is one Dotlink Camera and one Dotlink Mic, so starting needs one phone (`to`: its
    /// name or id), unless only one is connected. Stopping goes to every phone. Returns the
    /// phones asked.
    pub fn request_stream(&self, kind: &str, on: bool, to: Option<&str>) -> Result<String> {
        if !matches!(kind, "camera" | "mic") {
            bail!("unknown stream \"{kind}\"");
        }
        if !on {
            let target = self.target(None)?;
            self.send_control(json!({ "t": kind, "on": false }));
            return Ok(target.names);
        }
        let target = self.target(to)?;
        if target.id.is_none() && target.names.contains(", ") {
            bail!("several phones are connected ({}), choose one", target.names);
        }
        if let Some(from) = self.streams.lock().unwrap().get(kind) {
            bail!("{from} is already streaming it");
        }
        // With one phone online `target.id` is None and the message reaches just that phone.
        self.send_control_to(target.id.as_deref(), json!({ "t": kind, "on": true }));
        Ok(target.names)
    }

    /// Sends the current PC clipboard now, whatever the sync setting says.
    /// `to`: a device name or id, or None for every connected phone.
    pub async fn send_clipboard(&self, to: Option<&str>) -> Result<()> {
        let target = self.target(to)?;
        let Some(text) = clipboard::get().await.filter(|t| !t.is_empty()) else {
            bail!("clipboard is empty or not text");
        };
        self.remember_clip(&text);
        let chars = text.chars().count();
        self.send_control_to(target.id.as_deref(), json!({ "t": "clip", "text": text }));
        self.emit(Event::Clipboard { to_phone: true, device: target.names, chars });
        Ok(())
    }

    /// Resolves `to` (a device name or id) to an online phone, or every online phone for None.
    pub fn target(&self, to: Option<&str>) -> Result<Target> {
        let online = self.online.lock().unwrap();
        let state = self.state.lock().unwrap();
        let is_online = |d: &Device| online.get(&d.id).copied().unwrap_or(0) > 0;
        match to {
            Some(key) => {
                let dev = state
                    .devices
                    .iter()
                    .find(|d| d.id == key || d.name.eq_ignore_ascii_case(key))
                    .ok_or_else(|| anyhow!("no paired device \"{key}\""))?;
                if !is_online(dev) {
                    bail!("{} is not connected", dev.name);
                }
                Ok(Target {
                    id: Some(dev.id.clone()),
                    names: dev.name.clone(),
                    phones: vec![(dev.id.clone(), dev.name.clone())],
                })
            }
            None => {
                let phones: Vec<(String, String)> =
                    state.devices.iter().filter(|d| is_online(d)).map(|d| (d.id.clone(), d.name.clone())).collect();
                if phones.is_empty() {
                    bail!("no phone connected");
                }
                let names = phones.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>().join(", ");
                Ok(Target { id: None, names, phones })
            }
        }
    }

    pub fn server_id(&self) -> String {
        self.state.lock().unwrap().server_id.clone()
    }

    /// Sends a JSON message to every connected phone.
    pub fn send_control(&self, msg: Value) {
        self.send_control_to(None, msg);
    }

    /// Sends a JSON message to one phone (by device id), or to every phone for None.
    pub fn send_control_to(&self, device: Option<&str>, msg: Value) {
        let _ = self.control_tx.send((device.map(str::to_string), msg.to_string()));
    }

    /// Device ids of the phones connected right now.
    pub fn online_ids(&self) -> Vec<String> {
        self.online.lock().unwrap().iter().filter(|(_, n)| **n > 0).map(|(id, _)| id.clone()).collect()
    }

    pub fn online_devices(&self) -> Vec<String> {
        let online = self.online.lock().unwrap();
        let state = self.state.lock().unwrap();
        state.devices.iter().filter(|d| online.get(&d.id).copied().unwrap_or(0) > 0).map(|d| d.name.clone()).collect()
    }

    /// Records `text` as the latest clipboard; false if it is unchanged (stops echo loops).
    pub fn remember_clip(&self, text: &str) -> bool {
        let mut last = self.last_clip.lock().unwrap();
        if *last == text {
            return false;
        }
        *last = text.to_string();
        true
    }

    /// Starts a pairing window. Returns the `dotlink://` URI for the QR code and a receiver
    /// that yields the phone's name once it pairs.
    pub fn start_pairing(&self) -> (String, oneshot::Receiver<String>) {
        let secret = random_hex(16);
        let (tx, rx) = oneshot::channel();
        *self.pairing.lock().unwrap() =
            Some(Pairing { secret: secret.clone(), expires: Instant::now() + PAIRING_TTL, done: Some(tx) });
        (self.pairing_uri(&secret), rx)
    }

    /// A one-time code for a phone asking over USB, and its link. Separate from the window's
    /// code, so it stays valid, and one per phone, so phones pairing at once don't use up each
    /// other's.
    fn usb_pairing_uri(&self) -> Option<String> {
        let mut codes = self.usb_pairings.lock().unwrap();
        codes.retain(|(_, expires)| *expires > Instant::now());
        // A bound on what a flood of requests can pile up; plenty for the phones on a hub.
        if codes.len() >= 16 {
            return None;
        }
        let secret = random_hex(16);
        codes.push((secret.clone(), Instant::now() + PAIRING_TTL));
        Some(self.pairing_uri(&secret))
    }

    fn pairing_uri(&self, secret: &str) -> String {
        format!(
            "dotlink://pair?h={}&p={}&fp={}&c={secret}&n={}&id={}",
            local_ips().join(","),
            self.port,
            self.fingerprint,
            url_encode(&self.name()),
            self.server_id()
        )
    }

    /// Offers files to a phone (`to`: its name or id) or, for None, to every connected phone.
    /// The phone downloads them right away. Returns the count and the phones' names.
    pub fn offer_files(&self, paths: &[PathBuf], to: Option<&str>) -> Result<(usize, String)> {
        let target = self.target(to)?;
        let mut offers = self.offers.lock().unwrap();
        offers.retain(|_, o| o.created.elapsed() < OFFER_TTL);
        let mut count = 0;
        for path in paths {
            let meta = std::fs::metadata(path).map_err(|e| anyhow!("{}: {e}", path.display()))?;
            if !meta.is_file() {
                bail!("{}: only regular files can be sent", path.display());
            }
            let id = random_hex(12);
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
            self.send_control_to(target.id.as_deref(), json!({ "t": "file", "id": id, "name": name, "size": meta.len() }));
            for (device, phone) in &target.phones {
                self.put_transfer(Transfer {
                    id: transfer_id(&id, target.id.is_none(), device),
                    outgoing: true,
                    device: phone.clone(),
                    name: name.clone(),
                    size: Some(meta.len()),
                    done: 0,
                    state: TransferState::Waiting,
                    path: path.to_string_lossy().into_owned(),
                    started_at: now_secs(),
                });
            }
            offers.insert(
                id,
                Offer { device: target.id.clone(), path: path.clone(), name, size: meta.len(), created: Instant::now() },
            );
            count += 1;
        }
        Ok((count, target.names))
    }

    /// Saves a phone's new name (from its `info` message).
    pub fn rename_device(&self, id: &str, name: &str) {
        let name: String = name.trim().chars().filter(|c| !c.is_control()).take(64).collect();
        if name.is_empty() {
            return;
        }
        let mut state = self.state.lock().unwrap();
        let Some(dev) = state.devices.iter_mut().find(|d| d.id == id) else { return };
        if dev.name == name {
            return;
        }
        tracing::info!("{} is now called {name}", dev.name);
        dev.name = name;
        if let Err(e) = state.save() {
            tracing::warn!("state: {e:#}");
        }
        update_share_menu(&state);
    }

    pub fn unpair(&self, key: &str) -> Result<String> {
        let mut state = self.state.lock().unwrap();
        let idx = state
            .devices
            .iter()
            .position(|d| d.id == key || d.name.eq_ignore_ascii_case(key))
            .ok_or_else(|| anyhow!("no paired device \"{key}\""))?;
        let dev = state.devices.remove(idx);
        state.save()?;
        update_share_menu(&state);
        drop(state);
        for s in self.terms.lock().unwrap().remove(&dev.id).unwrap_or_default().into_values() {
            s.kill();
        }
        let _ = self.revoked_tx.send(dev.id.clone());
        self.emit(Event::Devices);
        Ok(dev.name)
    }
}

/// The file manager's "Share with Dotlink" items follow the paired phones.
fn update_share_menu(state: &State) {
    let phones: Vec<_> = state.devices.iter().map(|d| (d.id.clone(), d.name.clone())).collect();
    platform::share_menu::set(&phones);
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/pair", post(pair))
        .route("/pair/usb", get(usb_pair_probe).post(usb_pair_request))
        .route("/ping", get(ping))
        .route("/ws/control", get(ws_control))
        .route("/ws/input", get(ws_input))
        .route("/ws/term", get(ws_term))
        .route("/terms", get(list_terms))
        .route("/terms/{id}", axum::routing::delete(close_term))
        .route("/ws/camera", get(ws_camera))
        .route("/ws/mic", get(ws_mic))
        .route("/upload", post(upload).layer(DefaultBodyLimit::disable()))
        .route("/download/{id}", get(download))
        .with_state(app)
}

// ---------- auth ----------

/// A request from a paired phone (valid `Authorization: Bearer <token>`).
struct Authed(Device);

impl FromRequestParts<Arc<App>> for Authed {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, app: &Arc<App>) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or(StatusCode::UNAUTHORIZED)?;
        let state = app.state.lock().unwrap();
        state.device_for_token(token).cloned().map(Authed).ok_or(StatusCode::UNAUTHORIZED)
    }
}

// ---------- pairing ----------

#[derive(Deserialize)]
struct PairReq {
    secret: String,
    device_id: String,
    device_name: String,
}

async fn pair(AxState(app): AxState<Arc<App>>, Json(req): Json<PairReq>) -> Response {
    let done = {
        let mut pairing = app.pairing.lock().unwrap();
        let valid = pairing.as_ref().is_some_and(|p| p.expires > Instant::now() && ct_eq(&p.secret, &req.secret));
        if valid {
            pairing.take().and_then(|mut p| p.done.take())
        } else {
            // A code handed out over USB: used once, like the window's.
            let mut codes = app.usb_pairings.lock().unwrap();
            let now = Instant::now();
            let Some(i) = codes.iter().position(|(c, expires)| *expires > now && ct_eq(c, &req.secret)) else {
                return (StatusCode::FORBIDDEN, "pairing code expired or wrong").into_response();
            };
            codes.swap_remove(i);
            None
        }
    };
    let token = random_hex(32);
    let name: String = req.device_name.chars().take(64).collect();
    {
        let mut state = app.state.lock().unwrap();
        // Re-pairing the same phone replaces its old token.
        state.devices.retain(|d| d.id != req.device_id);
        state.devices.push(Device {
            id: req.device_id.chars().take(64).collect(),
            name: name.clone(),
            token_sha256: sha256_hex(token.as_bytes()),
            paired_at: now_secs(),
        });
        if let Err(e) = state.save() {
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
        update_share_menu(&state);
    }
    if let Some(done) = done {
        let _ = done.send(name.clone());
    }
    app.emit(Event::Paired { name: name.clone() });
    app.emit(Event::Devices);
    notify("Phone paired", &format!("{name} can now connect to this PC"));
    Json(json!({ "token": token, "server_id": app.server_id(), "server_name": app.name() })).into_response()
}

/// Through `adb reverse` the phone connects from this PC's loopback.
fn from_cable(peer: &SocketAddr) -> bool {
    match peer.ip() {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(v6.is_loopback(), |v4| v4.is_loopback()),
        v4 => v4.is_loopback(),
    }
}

/// The phone's pair screen asks, over the cable, whether a Dotlink PC is on the other end.
async fn usb_pair_probe(AxState(app): AxState<Arc<App>>, ConnectInfo(peer): ConnectInfo<SocketAddr>) -> Response {
    if !from_cable(&peer) {
        return StatusCode::NOT_FOUND.into_response();
    }
    Json(json!({ "name": app.name(), "id": app.server_id() })).into_response()
}

#[derive(Deserialize)]
struct UsbPairReq {
    /// Echoed back in the link (`r=`), so the phone knows it asked and pairs without a tap.
    nonce: String,
}

/// The phone asks to pair over the cable. The secret never goes back on this connection
/// (any app on the phone can reach `127.0.0.1` while the cable is in): the PC opens the link
/// in Dotlink through adb, which only works for phones that allowed this PC's USB debugging.
async fn usb_pair_request(
    AxState(app): AxState<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(req): Json<UsbPairReq>,
) -> Response {
    if !from_cable(&peer) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !(1..=64).contains(&req.nonce.len()) || !req.nonce.chars().all(|c| c.is_ascii_alphanumeric()) {
        return (StatusCode::BAD_REQUEST, "bad nonce").into_response();
    }
    let Some(uri) = app.usb_pairing_uri() else {
        return (StatusCode::TOO_MANY_REQUESTS, "Too many pairing requests, wait a moment").into_response();
    };
    let uri = format!("{uri}&r={}", req.nonce);
    match crate::usb::open_pairing(&uri, true).await {
        Ok(phones) => {
            tracing::info!("usb: pairing link opened on {} at the phone's request", phones.join(", "));
            Json(json!({ "ok": true })).into_response()
        }
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, format!("{e:#}")).into_response(),
    }
}

async fn ping(AxState(app): AxState<Arc<App>>, Authed(_): Authed) -> Json<Value> {
    let config = app.config();
    Json(json!({
        "name": app.name(),
        "terminal": config.terminal,
        "terminal_pin": !config.terminal_pin.is_empty(),
    }))
}

// ---------- control: clipboard + file offers ----------

async fn ws_control(
    ws: WebSocketUpgrade,
    AxState(app): AxState<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Authed(dev): Authed,
) -> Response {
    ws.on_upgrade(move |socket| control_session(app, dev, peer, socket))
}

async fn control_session(app: Arc<App>, dev: Device, peer: SocketAddr, socket: WebSocket) {
    // IPv4-mapped IPv6 addresses show up as plain IPv4 elsewhere (mDNS, adb).
    let ip = match peer.ip() {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(IpAddr::V6(v6)),
        v4 => v4,
    };
    // Through `adb reverse` the phone connects from this PC's loopback.
    let usb = ip.is_loopback();
    {
        let mut phones = app.phones.lock().unwrap();
        let phone = phones.entry(dev.id.clone()).or_default();
        if usb {
            phone.usb += 1;
        } else {
            phone.ip = Some(ip);
        }
    }
    *app.online.lock().unwrap().entry(dev.id.clone()).or_default() += 1;
    app.emit(Event::Devices);
    tracing::info!("{} connected{}", dev.name, if usb { " over USB" } else { "" });
    let (mut tx, mut rx) = socket.split();
    let mut events = app.control_tx.subscribe();
    let mut revoked = app.revoked_tx.subscribe();
    let hello = app.hello();
    let _ = tx.send(Message::Text(hello.to_string().into())).await;
    loop {
        tokio::select! {
            id = revoked.recv() => match id {
                Ok(id) if id == dev.id => break,
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            },
            ev = events.recv() => match ev {
                Ok((Some(to), _)) if to != dev.id => continue,
                Ok((_, text)) => if tx.send(Message::Text(text.into())).await.is_err() { break },
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            },
            msg = rx.next() => match msg {
                Some(Ok(Message::Text(text))) => {
                    let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
                    // The phone can't start the camera/mic from the background: it asks the
                    // user with a notification and tells us so.
                    if v["t"] == "info" {
                        let mut phones = app.phones.lock().unwrap();
                        let info = phones.entry(dev.id.clone()).or_default();
                        info.model = v["model"].as_str().unwrap_or("").to_string();
                        info.adb_wifi = v["adb_wifi"].as_bool().unwrap_or(false);
                        info.dev_options = v["dev_options"].as_bool().unwrap_or(false);
                        drop(phones);
                        // The phone's name, as set in its app.
                        if let Some(name) = v["name"].as_str() {
                            app.rename_device(&dev.id, name);
                        }
                        app.emit(Event::Devices);
                    }
                    if v["t"] == "battery"
                        && let Some(level) = v["level"].as_u64()
                    {
                        let charging = v["charging"].as_bool().unwrap_or(false);
                        app.phones.lock().unwrap().entry(dev.id.clone()).or_default().battery =
                            Some((level.min(100) as u8, charging));
                        app.emit(Event::Devices);
                    }
                    if v["t"] == "media" && v["state"] == "needs_tap" {
                        let kind = v["kind"].as_str().unwrap_or("camera").to_string();
                        app.emit(Event::MediaPrompt { device: dev.name.clone(), kind });
                    }
                    if v["t"] == "clip"
                        && let Some(text) = v["text"].as_str()
                        && app.remember_clip(text)
                    {
                        let chars = text.chars().count();
                        tracing::info!("clipboard from {} ({chars} chars)", dev.name);
                        match clipboard::set(text).await {
                            Ok(()) => app.emit(Event::Clipboard { to_phone: false, device: dev.name.clone(), chars }),
                            Err(e) => tracing::warn!("clipboard: {e}"),
                        }
                    }
                }
                Some(Ok(_)) => {}
                _ => break,
            }
        }
    }
    if let Some(n) = app.online.lock().unwrap().get_mut(&dev.id) {
        *n = n.saturating_sub(1);
    }
    if usb && let Some(phone) = app.phones.lock().unwrap().get_mut(&dev.id) {
        phone.usb = phone.usb.saturating_sub(1);
    }
    app.emit(Event::Devices);
    tracing::info!("{} disconnected", dev.name);
}

// ---------- remote input ----------

async fn ws_input(ws: WebSocketUpgrade, AxState(app): AxState<Arc<App>>, Authed(_): Authed) -> Response {
    ws.on_upgrade(move |mut socket| async move {
        while let Some(Ok(msg)) = socket.recv().await {
            let Message::Text(text) = msg else { continue };
            let Ok(ev) = serde_json::from_str::<InputMsg>(&text) else { continue };
            // The uinput devices are created on first use and kept for the daemon's lifetime.
            let result = (|| -> Result<()> {
                let mut input = app.input.lock().unwrap();
                if input.is_none() {
                    *input = Some(Input::new()?);
                }
                input.as_mut().unwrap().handle(ev)
            })();
            if let Err(e) = result {
                tracing::error!("input: {e:#}");
                let _ = socket.send(Message::Text(json!({ "error": format!("{e:#}") }).to_string().into())).await;
            }
        }
    })
}

// ---------- terminal ----------

#[derive(Deserialize)]
struct TermQuery {
    cols: Option<u16>,
    rows: Option<u16>,
    /// Session to attach to; without it, the first one (or a new one if there is none).
    id: Option<u32>,
    /// Start a new session instead of attaching.
    #[serde(default)]
    new: bool,
}

/// Rejects the request when the terminal is disabled or the PIN is wrong.
fn check_terminal(app: &App, headers: &HeaderMap) -> Option<Response> {
    let config = app.config();
    if !config.terminal {
        return Some((StatusCode::FORBIDDEN, "the shell is turned off in Dotlink's settings").into_response());
    }
    if !config.terminal_pin.is_empty() {
        let pin = headers.get("x-terminal-pin").and_then(|v| v.to_str().ok()).unwrap_or("");
        if !ct_eq(pin, &config.terminal_pin) {
            return Some((StatusCode::FORBIDDEN, "wrong terminal PIN").into_response());
        }
    }
    None
}

async fn list_terms(AxState(app): AxState<Arc<App>>, Authed(dev): Authed, headers: HeaderMap) -> Response {
    if let Some(denied) = check_terminal(&app, &headers) {
        return denied;
    }
    let mut terms = app.terms.lock().unwrap();
    let sessions = terms.entry(dev.id).or_default();
    sessions.retain(|_, s| s.alive());
    let list: Vec<Value> = sessions.iter().map(|(id, s)| json!({ "id": id, "title": s.title() })).collect();
    Json(json!({ "sessions": list })).into_response()
}

async fn close_term(
    AxState(app): AxState<Arc<App>>,
    Authed(dev): Authed,
    Path(id): Path<u32>,
    headers: HeaderMap,
) -> Response {
    if let Some(denied) = check_terminal(&app, &headers) {
        return denied;
    }
    if let Some(s) = app.terms.lock().unwrap().get_mut(&dev.id).and_then(|m| m.remove(&id)) {
        s.kill();
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn ws_term(
    ws: WebSocketUpgrade,
    AxState(app): AxState<Arc<App>>,
    Authed(dev): Authed,
    Query(q): Query<TermQuery>,
    headers: HeaderMap,
) -> Response {
    if let Some(denied) = check_terminal(&app, &headers) {
        return denied;
    }
    let (cols, rows) = (q.cols.unwrap_or(80).max(10), q.rows.unwrap_or(24).max(4));
    let (id, session) = {
        let mut terms = app.terms.lock().unwrap();
        let sessions = terms.entry(dev.id.clone()).or_default();
        sessions.retain(|_, s| s.alive());
        let existing = if q.new {
            None
        } else {
            match q.id {
                Some(id) => sessions.get(&id).map(|s| (id, s.clone())),
                None => sessions.iter().next().map(|(id, s)| (*id, s.clone())),
            }
        };
        match existing {
            Some(found) => found,
            None => {
                let configured = app.config().shell;
                let (shell, args) =
                    if configured.is_empty() { platform::default_shell() } else { (configured, Vec::new()) };
                match Session::spawn(&shell, &args, cols, rows) {
                    Ok(s) => {
                        let id = sessions.keys().next_back().map_or(1, |last| last + 1);
                        sessions.insert(id, s.clone());
                        (id, s)
                    }
                    Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
                }
            }
        }
    };
    session.resize(cols, rows);
    ws.on_upgrade(move |socket| term_session(id, session, socket))
}

async fn term_session(id: u32, session: Arc<Session>, socket: WebSocket) {
    let (mut tx, mut rx) = socket.split();
    // Tell the app which session it got, then replay that session's screen.
    if tx.send(Message::Text(json!({ "t": "session", "id": id }).to_string().into())).await.is_err() {
        return;
    }
    let (backlog, mut output) = session.attach();
    if !backlog.is_empty() && tx.send(Message::Binary(backlog.into())).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            out = output.recv() => match out {
                Ok(chunk) if chunk.is_empty() => {
                    let _ = tx.send(Message::Text(json!({ "t": "exit" }).to_string().into())).await;
                    break;
                }
                Ok(chunk) => if tx.send(Message::Binary(chunk.into())).await.is_err() { break },
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            },
            msg = rx.next() => match msg {
                Some(Ok(Message::Binary(data))) => session.write(&data),
                Some(Ok(Message::Text(text))) => {
                    let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
                    if v["t"] == "resize" {
                        let cols = v["cols"].as_u64().unwrap_or(80) as u16;
                        let rows = v["rows"].as_u64().unwrap_or(24) as u16;
                        session.resize(cols.max(10), rows.max(4));
                    }
                }
                Some(Ok(_)) => {}
                _ => break,
            }
        }
    }
}

// ---------- camera ----------

async fn ws_camera(ws: WebSocketUpgrade, AxState(app): AxState<Arc<App>>, Authed(dev): Authed) -> Response {
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = camera_session(app, &dev, socket).await {
            tracing::warn!("camera: {e:#}");
        }
    })
}

async fn camera_session(app: Arc<App>, dev: &Device, mut socket: WebSocket) -> Result<()> {
    let (w, h) = match socket.recv().await {
        Some(Ok(Message::Text(t))) => {
            let v: Value = serde_json::from_str(&t)?;
            (v["w"].as_u64().unwrap_or(1280) as u32, v["h"].as_u64().unwrap_or(720) as u32)
        }
        _ => return Ok(()),
    };
    let streaming = find_camera(&app.config().camera_device).and_then(|_| Streaming::claim(&app, "camera", &dev.name));
    let _streaming = match streaming {
        Ok(s) => s,
        Err(e) => {
            socket.send(Message::Text(json!({ "error": format!("{e:#}") }).to_string().into())).await?;
            return Err(e);
        }
    };
    socket.send(Message::Text(json!({ "ok": true }).to_string().into())).await?;
    tracing::info!("camera from {} started ({w}x{h})", dev.name);
    while let Some(Ok(msg)) = socket.recv().await {
        if let Message::Binary(jpeg) = msg {
            app.camera.send(jpeg);
        }
    }
    tracing::info!("camera from {} stopped", dev.name);
    Ok(())
}

// ---------- microphone ----------

async fn ws_mic(ws: WebSocketUpgrade, AxState(app): AxState<Arc<App>>, Authed(dev): Authed) -> Response {
    ws.on_upgrade(move |mut socket| async move {
        let (rate, ch) = match socket.recv().await {
            Some(Ok(Message::Text(t))) => {
                let v: Value = serde_json::from_str(&t).unwrap_or_default();
                (v["rate"].as_u64().unwrap_or(48000) as u32, v["ch"].as_u64().unwrap_or(1) as u32)
            }
            _ => return,
        };
        let started = Streaming::claim(&app, "mic", &dev.name).and_then(|s| Ok((s, MicSink::start(rate, ch)?)));
        let (_streaming, mut sink) = match started {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("mic: {e:#}");
                let _ = socket.send(Message::Text(json!({ "error": format!("{e:#}") }).to_string().into())).await;
                return;
            }
        };
        let _ = socket.send(Message::Text(json!({ "ok": true }).to_string().into())).await;
        tracing::info!("mic from {} started", dev.name);
        while let Some(Ok(msg)) = socket.recv().await {
            if let Message::Binary(pcm) = msg {
                sink.write(pcm);
            }
        }
        tracing::info!("mic from {} stopped", dev.name);
    })
}

// ---------- files ----------

#[derive(Deserialize)]
struct UploadQuery {
    name: String,
}

async fn upload(
    AxState(app): AxState<Arc<App>>,
    Authed(dev): Authed,
    Query(q): Query<UploadQuery>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let Some(name) = sanitize_name(&q.name) else {
        return (StatusCode::BAD_REQUEST, "bad file name").into_response();
    };
    let size = headers.get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok());
    let mut progress = Progress::new(
        &app,
        Transfer {
            id: random_hex(12),
            outgoing: false,
            device: dev.name.clone(),
            name: name.clone(),
            size,
            done: 0,
            state: TransferState::Active,
            path: String::new(),
            started_at: now_secs(),
        },
    );
    let sha = headers.get("x-sha256").and_then(|v| v.to_str().ok());
    match save_upload(&app, &name, sha, body, &mut progress).await {
        Ok(path) => {
            let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            progress.transfer.name = name.clone();
            progress.transfer.path = path.to_string_lossy().into_owned();
            progress.finish(Ok(()));
            platform::notify_file(&dev.name, &name, &path);
            Json(json!({ "ok": true, "name": name })).into_response()
        }
        Err(e) => {
            progress.finish(Err(e.to_string()));
            (StatusCode::BAD_REQUEST, e.to_string()).into_response()
        }
    }
}

async fn save_upload(app: &App, name: &str, sha: Option<&str>, body: Body, progress: &mut Progress) -> Result<PathBuf> {
    let dir = app.config().download_dir();
    tokio::fs::create_dir_all(&dir).await?;
    let dest = unique_path(&dir, name);
    let part = dir.join(format!(".{name}.{}.part", random_hex(4)));
    // Each write to a tokio File is a trip to the blocking pool; the body arrives in ~16 KB
    // TLS records, so batch them.
    let mut file = tokio::io::BufWriter::with_capacity(1024 * 1024, tokio::fs::File::create(&part).await?);
    let mut hasher = Sha256::new();
    let mut stream = body.into_data_stream();
    let result: Result<()> = async {
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            hasher.update(&chunk);
            file.write_all(&chunk).await?;
            progress.add(chunk.len() as u64);
        }
        file.flush().await?;
        if let Some(expected) = sha
            && !hex::encode(hasher.finalize()).eq_ignore_ascii_case(expected)
        {
            bail!("checksum mismatch");
        }
        Ok(())
    }
    .await;
    if let Err(e) = result {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(e);
    }
    tokio::fs::rename(&part, &dest).await?;
    Ok(dest)
}

async fn download(AxState(app): AxState<Arc<App>>, Authed(dev): Authed, Path(id): Path<String>) -> Response {
    let (path, name, size, tid) = {
        let offers = app.offers.lock().unwrap();
        match offers.get(&id) {
            Some(o) if o.device.as_ref().is_none_or(|d| *d == dev.id) => {
                (o.path.clone(), o.name.clone(), o.size, transfer_id(&id, o.device.is_none(), &dev.id))
            }
            _ => return StatusCode::NOT_FOUND.into_response(),
        }
    };
    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(_) => return StatusCode::GONE.into_response(),
    };
    let mut transfer = app.transfers.lock().unwrap().iter().find(|t| t.id == tid).cloned().unwrap_or(Transfer {
        id: tid.clone(),
        outgoing: true,
        device: String::new(),
        name: name.clone(),
        size: Some(size),
        done: 0,
        state: TransferState::Active,
        path: path.to_string_lossy().into_owned(),
        started_at: now_secs(),
    });
    transfer.device = dev.name;
    transfer.done = 0;
    transfer.state = TransferState::Active;
    // Counts what the phone has read; the progress is dropped (→ failed) if it hangs up early.
    let progress = Progress::new(&app, transfer);
    let chunks = tokio_util::io::ReaderStream::with_capacity(file, 256 * 1024);
    let stream = futures::stream::unfold((chunks, progress), |(mut chunks, mut progress)| async move {
        match chunks.next().await {
            Some(Ok(chunk)) => {
                progress.add(chunk.len() as u64);
                // hyper stops reading once Content-Length bytes went out, so the end of the
                // stream is never seen: the last byte is the end.
                if progress.transfer.size.is_some_and(|size| progress.transfer.done >= size) {
                    progress.finish(Ok(()));
                }
                Some((Ok(chunk), (chunks, progress)))
            }
            Some(Err(e)) => {
                progress.finish(Err(e.to_string()));
                Some((Err(e), (chunks, progress)))
            }
            None => {
                progress.finish(Ok(()));
                None
            }
        }
    });
    let body = Body::from_stream(stream);
    (
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (header::CONTENT_LENGTH, size.to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}\"", name.replace('"', "'"))),
        ],
        body,
    )
        .into_response()
}

// ---------- helpers ----------

fn sanitize_name(name: &str) -> Option<String> {
    let base = name.rsplit(['/', '\\']).next()?.trim();
    let clean: String = base.chars().filter(|c| !c.is_control()).collect();
    let clean = clean.trim_start_matches('.').to_string();
    (!clean.is_empty() && clean.len() <= 255).then_some(clean)
}

fn unique_path(dir: &std::path::Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (1..)
        .map(|i| dir.join(format!("{stem} ({i}){ext}")))
        .find(|p| !p.exists())
        .unwrap()
}

/// Marks a camera/mic stream as running for the window while it lives.
struct Streaming {
    app: Arc<App>,
    kind: &'static str,
}

impl Streaming {
    /// Marks `device` as streaming `kind`; fails while another phone streams it (they would
    /// share one Dotlink Camera / Mic).
    fn claim(app: &Arc<App>, kind: &'static str, device: &str) -> Result<Self> {
        let mut streams = app.streams.lock().unwrap();
        if let Some(from) = streams.get(kind) {
            bail!("{from} is already streaming the {}", if kind == "mic" { "microphone" } else { "webcam" });
        }
        streams.insert(kind, device.to_string());
        drop(streams);
        app.emit(Event::Streams);
        Ok(Self { app: app.clone(), kind })
    }
}

impl Drop for Streaming {
    fn drop(&mut self) {
        self.app.streams.lock().unwrap().remove(self.kind);
        self.app.emit(Event::Streams);
    }
}

/// A transfer in progress. Reports to the window at most 4 times a second; dropping it
/// before `finish` marks the transfer failed (the phone went away).
struct Progress {
    app: Arc<App>,
    transfer: Transfer,
    last_report: Instant,
    finished: bool,
}

impl Progress {
    fn new(app: &Arc<App>, transfer: Transfer) -> Self {
        app.put_transfer(transfer.clone());
        Self { app: app.clone(), transfer, last_report: Instant::now(), finished: false }
    }

    fn add(&mut self, bytes: u64) {
        self.transfer.done += bytes;
        if self.last_report.elapsed() >= Duration::from_millis(250) {
            self.last_report = Instant::now();
            self.app.put_transfer(self.transfer.clone());
        }
    }

    fn finish(&mut self, result: std::result::Result<(), String>) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.transfer.state = match result {
            Ok(()) => TransferState::Done,
            Err(e) => TransferState::Failed(e),
        };
        self.app.put_transfer(self.transfer.clone());
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.finish(Err("interrupted".into()));
    }
}

/// Constant-time string comparison for secrets.
fn ct_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// LAN IPv4 addresses, skipping loopback and container/VM bridges.
pub fn local_ips() -> Vec<String> {
    let skip = ["docker", "br-", "virbr", "veth", "podman", "cni", "flannel", "tailscale", "lo"];
    if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback() && i.ip().is_ipv4() && !skip.iter().any(|s| i.name.starts_with(s)))
        .map(|i| i.ip().to_string())
        .collect()
}
