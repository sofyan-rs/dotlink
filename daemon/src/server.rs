//! Shared daemon state and the HTTPS / WebSocket API the phone talks to.

use crate::config::{now_secs, random_hex, sha256_hex, Config, Device, State};
use crate::input::{Input, InputMsg};
use crate::media::{find_camera, CameraHub, MicSink};
use crate::term::Session;
use crate::{clipboard, notify};
use anyhow::{anyhow, bail, Result};
use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, FromRequestParts, Path, Query, State as AxState};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::sync::{broadcast, oneshot};

const PAIRING_TTL: Duration = Duration::from_secs(300);
const OFFER_TTL: Duration = Duration::from_secs(3600);

struct Pairing {
    secret: String,
    expires: Instant,
    done: Option<oneshot::Sender<String>>,
}

struct Offer {
    path: PathBuf,
    name: String,
    size: u64,
    created: Instant,
}

pub struct App {
    pub config: Config,
    pub fingerprint: String,
    pub hostname: String,
    pub state: Mutex<State>,
    pairing: Mutex<Option<Pairing>>,
    control_tx: broadcast::Sender<String>,
    /// Device id → number of open control connections.
    online: Mutex<HashMap<String, usize>>,
    offers: Mutex<HashMap<String, Offer>>,
    last_clip: Mutex<String>,
    input: Mutex<Option<Input>>,
    /// Device id → its shell sessions by id (the tabs in the app's Shell screen).
    terms: Mutex<HashMap<String, BTreeMap<u32, Arc<Session>>>>,
    /// Feeds "Dotlink Camera" (phone frames or the "camera off" placeholder).
    pub camera: CameraHub,
}

impl App {
    pub fn new(config: Config, state: State, fingerprint: String) -> Arc<Self> {
        let hostname = hostname::get().map(|h| h.to_string_lossy().into_owned()).unwrap_or_else(|_| "Linux".into());
        let camera = CameraHub::start(config.camera_device.clone(), config.camera_size());
        Arc::new(Self {
            config,
            fingerprint,
            hostname,
            state: Mutex::new(state),
            pairing: Mutex::new(None),
            control_tx: broadcast::channel(64).0,
            online: Mutex::new(HashMap::new()),
            offers: Mutex::new(HashMap::new()),
            last_clip: Mutex::new(String::new()),
            input: Mutex::new(None),
            terms: Mutex::new(HashMap::new()),
            camera,
        })
    }

    pub fn server_id(&self) -> String {
        self.state.lock().unwrap().server_id.clone()
    }

    /// Sends a JSON message to every connected phone.
    pub fn send_control(&self, msg: Value) {
        let _ = self.control_tx.send(msg.to_string());
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
        let hosts = local_ips().join(",");
        let uri = format!(
            "dotlink://pair?h={hosts}&p={}&fp={}&c={secret}&n={}&id={}",
            self.config.port,
            self.fingerprint,
            url_encode(&self.hostname),
            self.server_id()
        );
        (uri, rx)
    }

    /// Offers files to the connected phone, which downloads them right away.
    pub fn offer_files(&self, paths: &[PathBuf]) -> Result<usize> {
        if self.online_devices().is_empty() {
            bail!("no phone connected");
        }
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
            self.send_control(json!({ "t": "file", "id": id, "name": name, "size": meta.len() }));
            offers.insert(id, Offer { path: path.clone(), name, size: meta.len(), created: Instant::now() });
            count += 1;
        }
        Ok(count)
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
        for s in self.terms.lock().unwrap().remove(&dev.id).unwrap_or_default().into_values() {
            s.kill();
        }
        Ok(dev.name)
    }
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/pair", post(pair))
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
        if !valid {
            return (StatusCode::FORBIDDEN, "pairing code expired or wrong").into_response();
        }
        pairing.take().and_then(|mut p| p.done.take())
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
    }
    if let Some(done) = done {
        let _ = done.send(name.clone());
    }
    notify("Phone paired", &format!("{name} can now connect to this PC"));
    Json(json!({ "token": token, "server_id": app.server_id(), "server_name": app.hostname })).into_response()
}

async fn ping(AxState(app): AxState<Arc<App>>, Authed(_): Authed) -> Json<Value> {
    Json(json!({
        "name": app.hostname,
        "terminal": app.config.terminal,
        "terminal_pin": !app.config.terminal_pin.is_empty(),
    }))
}

// ---------- control: clipboard + file offers ----------

async fn ws_control(ws: WebSocketUpgrade, AxState(app): AxState<Arc<App>>, Authed(dev): Authed) -> Response {
    ws.on_upgrade(move |socket| control_session(app, dev, socket))
}

async fn control_session(app: Arc<App>, dev: Device, socket: WebSocket) {
    *app.online.lock().unwrap().entry(dev.id.clone()).or_default() += 1;
    tracing::info!("{} connected", dev.name);
    let (mut tx, mut rx) = socket.split();
    let mut events = app.control_tx.subscribe();
    let hello = json!({ "t": "hello", "name": app.hostname, "terminal": app.config.terminal });
    let _ = tx.send(Message::Text(hello.to_string().into())).await;
    loop {
        tokio::select! {
            ev = events.recv() => match ev {
                Ok(text) => if tx.send(Message::Text(text.into())).await.is_err() { break },
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            },
            msg = rx.next() => match msg {
                Some(Ok(Message::Text(text))) => {
                    let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
                    if v["t"] == "clip" {
                        if let Some(text) = v["text"].as_str() {
                            if app.remember_clip(text) {
                                tracing::info!("clipboard from {} ({} chars)", dev.name, text.chars().count());
                                if let Err(e) = clipboard::set(text).await {
                                    tracing::warn!("wl-copy: {e}");
                                }
                            }
                        }
                    }
                }
                Some(Ok(_)) => {}
                _ => break,
            }
        }
    }
    let mut online = app.online.lock().unwrap();
    if let Some(n) = online.get_mut(&dev.id) {
        *n = n.saturating_sub(1);
    }
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
    if !app.config.terminal {
        return Some((StatusCode::FORBIDDEN, "terminal is disabled in config.toml").into_response());
    }
    if !app.config.terminal_pin.is_empty() {
        let pin = headers.get("x-terminal-pin").and_then(|v| v.to_str().ok()).unwrap_or("");
        if !ct_eq(pin, &app.config.terminal_pin) {
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
                let shell = if app.config.shell.is_empty() {
                    std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into())
                } else {
                    app.config.shell.clone()
                };
                match Session::spawn(&shell, cols, rows) {
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
    if let Err(e) = find_camera(&app.config.camera_device) {
        socket.send(Message::Text(json!({ "error": format!("{e:#}") }).to_string().into())).await?;
        return Err(e);
    }
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

async fn ws_mic(ws: WebSocketUpgrade, Authed(dev): Authed) -> Response {
    ws.on_upgrade(move |mut socket| async move {
        let (rate, ch) = match socket.recv().await {
            Some(Ok(Message::Text(t))) => {
                let v: Value = serde_json::from_str(&t).unwrap_or_default();
                (v["rate"].as_u64().unwrap_or(48000) as u32, v["ch"].as_u64().unwrap_or(1) as u32)
            }
            _ => return,
        };
        let mut sink = match MicSink::start(rate, ch) {
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
                sink.write(&pcm);
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
    match save_upload(&app, &q.name, headers.get("x-sha256").and_then(|v| v.to_str().ok()), body).await {
        Ok(path) => {
            let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            notify_file(&dev.name, &name, &path);
            Json(json!({ "ok": true, "name": name })).into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

async fn save_upload(app: &App, name: &str, sha: Option<&str>, body: Body) -> Result<PathBuf> {
    let name = sanitize_name(name).ok_or_else(|| anyhow!("bad file name"))?;
    let dir = app.config.download_dir();
    tokio::fs::create_dir_all(&dir).await?;
    let dest = unique_path(&dir, &name);
    let part = dir.join(format!(".{name}.{}.part", random_hex(4)));
    let mut file = tokio::fs::File::create(&part).await?;
    let mut hasher = Sha256::new();
    let mut stream = body.into_data_stream();
    let result: Result<()> = async {
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            hasher.update(&chunk);
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        if let Some(expected) = sha {
            if !hex::encode(hasher.finalize()).eq_ignore_ascii_case(expected) {
                bail!("checksum mismatch");
            }
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

async fn download(AxState(app): AxState<Arc<App>>, Authed(_): Authed, Path(id): Path<String>) -> Response {
    let (path, name, size) = {
        let offers = app.offers.lock().unwrap();
        match offers.get(&id) {
            Some(o) => (o.path.clone(), o.name.clone(), o.size),
            None => return StatusCode::NOT_FOUND.into_response(),
        }
    };
    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(_) => return StatusCode::GONE.into_response(),
    };
    let body = Body::from_stream(tokio_util::io::ReaderStream::with_capacity(file, 256 * 1024));
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

fn notify_file(from: &str, name: &str, path: &std::path::Path) {
    let path = path.to_path_buf();
    let (from, name) = (from.to_string(), name.to_string());
    tokio::spawn(async move {
        let out = tokio::process::Command::new("notify-send")
            .args(["-a", "Dotlink", "-i", "document-save", "--action=open=Open", "--action=folder=Show in folder"])
            .arg(format!("Received from {from}"))
            .arg(&name)
            .output()
            .await;
        let target = match out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).as_deref() {
            Ok("open") => path,
            Ok("folder") => path.parent().map(|p| p.to_path_buf()).unwrap_or(path),
            _ => return,
        };
        let _ = tokio::process::Command::new("xdg-open").arg(target).spawn();
    });
}

/// Constant-time string comparison for secrets.
fn ct_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
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
