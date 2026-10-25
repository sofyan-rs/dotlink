//! Linking to another PC: this PC acts like a phone towards it. It pairs with the other PC's
//! `dotlink://pair` link (as `kind: pc`), keeps a control connection open to it, and sends and
//! receives files and the clipboard through the same API the phone uses.
//!
//! Only the PC that joined dials; the other one sees it as a paired device of kind `pc`. The
//! other PC's certificate is pinned by its SHA-256 fingerprint from the link, like the phone does.

use crate::config::{Peer, now_secs, sha256_hex, url_decode, url_encode};
use crate::events::{Event, Transfer, TransferState};
use crate::platform::{self, clipboard};
use crate::server::{App, Progress, save_stream};
use anyhow::{Context, Result, anyhow, bail};
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_util::sync::CancellationToken;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
/// A ping this often keeps NAT and firewalls from dropping an idle link and notices a dead one.
const PING_EVERY: Duration = Duration::from_secs(20);
/// Nothing at all from the other PC for this long: the link is gone.
const SILENT_LIMIT: Duration = Duration::from_secs(60);
const RETRY_MIN: Duration = Duration::from_secs(2);
const RETRY_MAX: Duration = Duration::from_secs(30);

/// The PCs this one links to, while running.
#[derive(Default)]
pub struct Peers {
    links: Mutex<HashMap<String, Link>>,
}

struct Link {
    stop: CancellationToken,
    /// Set while connected: control messages for the other PC.
    tx: Option<mpsc::UnboundedSender<String>>,
    /// Where it answered last.
    addr: Option<SocketAddr>,
    /// Its battery (laptops), while connected: (percent, on power).
    battery: Option<(u8, bool)>,
}

impl Peers {
    pub fn online(&self, id: &str) -> bool {
        self.links.lock().unwrap().get(id).is_some_and(|l| l.tx.is_some())
    }

    /// Sends a control message; false when not connected.
    pub fn send(&self, id: &str, msg: &Value) -> bool {
        let links = self.links.lock().unwrap();
        links.get(id).and_then(|l| l.tx.as_ref()).is_some_and(|tx| tx.send(msg.to_string()).is_ok())
    }

    pub fn battery(&self, id: &str) -> Option<(u8, bool)> {
        self.links.lock().unwrap().get(id).filter(|l| l.tx.is_some()).and_then(|l| l.battery)
    }

    fn set_battery(&self, id: &str, battery: (u8, bool)) {
        if let Some(link) = self.links.lock().unwrap().get_mut(id) {
            link.battery = Some(battery);
        }
    }

    fn addr(&self, id: &str) -> Option<SocketAddr> {
        self.links.lock().unwrap().get(id).and_then(|l| l.addr)
    }

    fn set_online(&self, id: &str, tx: Option<mpsc::UnboundedSender<String>>, addr: Option<SocketAddr>) {
        if let Some(link) = self.links.lock().unwrap().get_mut(id) {
            if tx.is_none() {
                link.battery = None;
            }
            link.tx = tx;
            if addr.is_some() {
                link.addr = addr;
            }
        }
    }
}

/// Starts the links to every PC in `state.json` (service start).
pub fn start_all(app: &Arc<App>) {
    let ids: Vec<String> = app.state.lock().unwrap().peers.iter().map(|p| p.id.clone()).collect();
    for id in ids {
        start(app, id);
    }
}

fn start(app: &Arc<App>, id: String) {
    let stop = CancellationToken::new();
    let old = app.peers.links.lock().unwrap().insert(id.clone(), Link { stop: stop.clone(), tx: None, addr: None, battery: None });
    if let Some(old) = old {
        old.stop.cancel();
    }
    tokio::spawn(run(app.clone(), id, stop));
}

// ---------- pairing ----------

/// What a `dotlink://pair?…` link holds.
struct PairLink {
    hosts: Vec<String>,
    port: u16,
    fingerprint: String,
    secret: String,
    name: String,
    id: String,
}

fn parse_link(link: &str) -> Result<PairLink> {
    let query = link
        .trim()
        .strip_prefix("dotlink://pair?")
        .ok_or_else(|| anyhow!("not a Dotlink pairing link (dotlink://pair?…)"))?;
    let mut params = HashMap::new();
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            params.insert(k, url_decode(v).ok_or_else(|| anyhow!("bad pairing link"))?);
        }
    }
    let mut get = |k: &str| params.remove(k).filter(|v| !v.is_empty());
    let fingerprint = get("fp").ok_or_else(|| anyhow!("the link has no certificate fingerprint"))?;
    if fingerprint.len() != 64 || !fingerprint.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("bad certificate fingerprint in the link");
    }
    Ok(PairLink {
        hosts: get("h").map(|h| h.split(',').map(str::to_string).collect()).unwrap_or_default(),
        port: get("p").and_then(|p| p.parse().ok()).ok_or_else(|| anyhow!("the link has no port"))?,
        fingerprint: fingerprint.to_lowercase(),
        secret: get("c").ok_or_else(|| anyhow!("the link has no pairing code"))?,
        name: get("n").unwrap_or_default(),
        id: get("id").unwrap_or_default(),
    })
}

/// Pairs with the PC whose `dotlink://pair` link this is and starts linking to it. Returns its
/// name.
pub async fn join(app: &Arc<App>, link: &str) -> Result<String> {
    let link = parse_link(link)?;
    if link.id == app.server_id() {
        bail!("this is this PC's own link: open it on the other PC");
    }
    if let Some(dev) = app.state.lock().unwrap().devices.iter().find(|d| d.id == link.id) {
        bail!("{} is already linked: it joined this PC", dev.name);
    }
    let tls = client_config(&link.fingerprint);
    let addrs: Vec<SocketAddr> =
        link.hosts.iter().filter_map(|h| h.parse().ok()).map(|ip| SocketAddr::new(ip, link.port)).collect();
    if addrs.is_empty() {
        bail!("the link has no address");
    }
    let body = json!({
        "secret": link.secret,
        "device_id": app.server_id(),
        "device_name": app.name(),
        "kind": "pc",
    })
    .to_string();
    let mut last_error = anyhow!("no address answered");
    for addr in &addrs {
        let req = Request::post("/pair")
            .header(hyper::header::HOST, addr.to_string())
            .header(hyper::header::CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from(body.clone())))?;
        let reply = match request(*addr, &tls, req).await {
            Ok(reply) => reply,
            Err(e) => {
                last_error = e;
                continue;
            }
        };
        let status = reply.status();
        let text = String::from_utf8_lossy(&reply.into_body().collect().await?.to_bytes()).into_owned();
        if !status.is_success() {
            bail!("{} refused: {}", if link.name.is_empty() { "the PC" } else { &link.name }, text.trim());
        }
        let v: Value = serde_json::from_str(&text).context("pairing reply")?;
        let token = v["token"].as_str().ok_or_else(|| anyhow!("pairing reply has no token"))?;
        let id = v["server_id"].as_str().filter(|s| !s.is_empty()).unwrap_or(&link.id).to_string();
        let name = v["server_name"].as_str().filter(|s| !s.is_empty()).unwrap_or(&link.name).to_string();
        save_peer(app, &id, &name, &link.fingerprint, *addr, &link.hosts, token)?;
        return Ok(name);
    }
    Err(last_error.context(format!("could not reach {}", if link.name.is_empty() { "the PC" } else { &link.name })))
}

/// Remembers a PC just joined and starts linking to it. `addr` answered; `hosts` are its other
/// addresses.
fn save_peer(app: &Arc<App>, id: &str, name: &str, fingerprint: &str, addr: SocketAddr, hosts: &[String], token: &str) -> Result<()> {
    // The address that answered first, so reconnecting tries it first.
    let first = addr.ip().to_string();
    let mut all: Vec<String> = hosts.iter().filter(|h| **h != first).cloned().collect();
    all.insert(0, first);
    {
        let mut state = app.state.lock().unwrap();
        state.peers.retain(|p| p.id != id);
        state.peers.push(Peer {
            id: id.to_string(),
            name: name.to_string(),
            fingerprint: fingerprint.to_string(),
            hosts: all,
            port: addr.port(),
            token: token.to_string(),
            paired_at: now_secs(),
            clipboard_auto: false,
        });
        state.save()?;
    }
    tracing::info!("linked to {name}");
    start(app, id.to_string());
    app.emit(Event::Devices);
    Ok(())
}

/// A Dotlink PC on the network (mDNS), for joining with a code.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Nearby {
    pub id: String,
    pub name: String,
    pub port: u16,
    /// Its addresses.
    pub hosts: Vec<String>,
    /// The first 16 hex digits of its certificate's fingerprint, as advertised.
    pub fp: String,
    /// Already linked with this PC (either way).
    pub linked: bool,
}

/// The other Dotlink PCs on the network.
pub async fn nearby(app: &App) -> Vec<Nearby> {
    let own = app.server_id();
    let mut found = scan(None).await;
    found.retain(|n| n.id != own);
    // In the order they answered they'd move around between scans.
    found.sort_by_key(|n| n.name.to_lowercase());
    let state = app.state.lock().unwrap();
    for n in &mut found {
        n.linked = state.peers.iter().any(|p| p.id == n.id) || state.devices.iter().any(|d| d.id == n.id);
    }
    found
}

/// Joins the PC `id` (from [nearby]) with the 6-digit code it shows (see `pair_code.rs`).
pub async fn join_code(app: &Arc<App>, id: &str, code: &str) -> Result<String> {
    let code = crate::pair_code::clean(code);
    if code.len() != 6 {
        bail!("the code has 6 digits");
    }
    if let Some(dev) = app.state.lock().unwrap().devices.iter().find(|d| d.id == id) {
        bail!("{} is already linked: it joined this PC", dev.name);
    }
    let pc = scan(Some(id)).await.into_iter().next().ok_or_else(|| anyhow!("that PC isn't on the network any more"))?;
    let mut last_error = anyhow!("{} has no address", pc.name);
    for host in &pc.hosts {
        let Ok(ip) = host.parse() else { continue };
        let addr = SocketAddr::new(ip, pc.port);
        match join_code_at(app, &pc, addr, &code).await {
            Ok(name) => return Ok(name),
            Err(e) if e.is::<Refused>() => return Err(e),
            Err(e) => last_error = e,
        }
    }
    Err(last_error.context(format!("could not reach {}", pc.name)))
}

/// The other PC answered and said no (wrong code, no code shown…): no use trying its other
/// addresses.
#[derive(Debug)]
struct Refused(String);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Refused {}

async fn join_code_at(app: &Arc<App>, pc: &Nearby, addr: SocketAddr, code: &str) -> Result<String> {
    // No fingerprint to pin yet: take whatever certificate it shows, then make the code prove it.
    let (tls, seen) = capturing_config();
    let stream = connect(addr, &tls).await?;
    let fingerprint = seen.lock().unwrap().clone().ok_or_else(|| anyhow!("no certificate"))?;
    if !fingerprint.starts_with(&pc.fp.to_lowercase()) {
        return Err(Refused(format!("{} answered with another certificate than it advertises", pc.name)).into());
    }
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let post = |path: &str, body: Value| {
        Request::post(path)
            .header(hyper::header::HOST, addr.to_string())
            .header(hyper::header::CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from(body.to_string())))
    };
    // Both requests on this one connection: the certificate checked above is the one the key
    // is bound to.
    let (exchange, msg) = crate::pair_code::start_joiner(code, &fingerprint);
    let body = json!({ "device_id": app.server_id(), "device_name": app.name(), "msg": hex::encode(msg) });
    let v = json_reply(&pc.name, sender.send_request(post("/pair/code", body)?).await?).await?;
    let reply = v["msg"].as_str().and_then(|m| hex::decode(m).ok()).ok_or_else(|| anyhow!("bad pairing reply"))?;
    let key = exchange.finish(&reply).ok_or_else(|| anyhow!("bad pairing reply"))?;
    sender.ready().await?;
    let body = json!({ "session": v["session"], "proof": crate::pair_code::confirm(&key, true) });
    let v = json_reply(&pc.name, sender.send_request(post("/pair/code/confirm", body)?).await?).await?;
    if v["proof"].as_str() != Some(crate::pair_code::confirm(&key, false).as_str()) {
        return Err(Refused(format!("{} couldn't prove it knows the code", pc.name)).into());
    }
    let token = v["token"].as_str().ok_or_else(|| anyhow!("pairing reply has no token"))?;
    let name = v["server_name"].as_str().filter(|s| !s.is_empty()).unwrap_or(&pc.name).to_string();
    save_peer(app, &pc.id, &name, &fingerprint, addr, &pc.hosts, token)?;
    Ok(name)
}

/// A pairing reply as JSON; a refusal (its text) as [Refused].
async fn json_reply(name: &str, reply: Response<Incoming>) -> Result<Value> {
    let status = reply.status();
    let text = String::from_utf8_lossy(&reply.into_body().collect().await?.to_bytes()).into_owned();
    if !status.is_success() {
        return Err(Refused(format!("{name}: {}", text.trim())).into());
    }
    Ok(serde_json::from_str(&text).context("pairing reply")?)
}

/// Forgets a linked PC here, and tells it to forget this one (when it can be reached).
pub fn forget(app: &Arc<App>, id: &str) -> Result<String> {
    let peer = {
        let mut state = app.state.lock().unwrap();
        let idx = state.peers.iter().position(|p| p.id == id).ok_or_else(|| anyhow!("no linked PC \"{id}\""))?;
        let peer = state.peers.remove(idx);
        state.save()?;
        peer
    };
    let addr = app.peers.addr(id);
    if let Some(link) = app.peers.links.lock().unwrap().remove(id) {
        link.stop.cancel();
    }
    app.emit(Event::Devices);
    if let Some(addr) = addr {
        let peer = peer.clone();
        tokio::spawn(async move {
            let req = authed(Request::delete("/pair"), &peer, addr).body(Full::new(Bytes::new()));
            if let Ok(req) = req {
                let _ = request(addr, &client_config(&peer.fingerprint), req).await;
            }
        });
    }
    Ok(peer.name)
}

// ---------- the link ----------

/// The other PC answered 401: it unpaired this one.
#[derive(Debug)]
struct Revoked;

impl std::fmt::Display for Revoked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("unpaired by the other PC")
    }
}

impl std::error::Error for Revoked {}

/// Keeps the link to one PC up until `stop`.
async fn run(app: Arc<App>, id: String, stop: CancellationToken) {
    let mut delay = RETRY_MIN;
    loop {
        let Some(peer) = app.state.lock().unwrap().peers.iter().find(|p| p.id == id).cloned() else { return };
        tokio::select! {
            _ = stop.cancelled() => return,
            result = session(&app, &peer) => match result {
                Ok(()) => delay = RETRY_MIN,
                Err(e) if e.is::<Revoked>() => {
                    tracing::info!("{} unpaired this PC", peer.name);
                    let _ = forget(&app, &id);
                    platform::notify("PC unlinked", &format!("{} forgot this PC", peer.name));
                    return;
                }
                Err(e) => tracing::debug!("{}: {e:#}", peer.name),
            },
        }
        app.peers.set_online(&id, None, None);
        app.emit(Event::Devices);
        tokio::select! {
            _ = stop.cancelled() => return,
            _ = tokio::time::sleep(delay) => {}
        }
        delay = (delay * 2).min(RETRY_MAX);
    }
}

/// One control connection, from connecting to its end. Ok when it was up for a while.
async fn session(app: &Arc<App>, peer: &Peer) -> Result<()> {
    let tls = client_config(&peer.fingerprint);
    let (addr, stream) = connect_any(app, peer, &tls).await?;
    let mut req = format!("wss://{addr}/ws/control").into_client_request()?;
    req.headers_mut().insert(hyper::header::AUTHORIZATION, format!("Bearer {}", peer.token).parse()?);
    let (ws, _) = match tokio_tungstenite::client_async(req, stream).await {
        Ok(ok) => ok,
        Err(tokio_tungstenite::tungstenite::Error::Http(r)) if r.status() == StatusCode::UNAUTHORIZED => {
            return Err(Revoked.into());
        }
        Err(e) => return Err(e.into()),
    };
    let (mut tx, mut rx) = ws.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
    app.peers.set_online(&peer.id, Some(out_tx), Some(addr));
    app.emit(Event::Devices);
    tracing::info!("linked to {} at {addr}", peer.name);
    let info = json!({ "t": "info", "kind": "pc", "name": app.name(), "os": std::env::consts::OS });
    tx.send(Message::text(info.to_string())).await?;
    if let Some(battery) = app.battery_msg() {
        tx.send(Message::text(battery.to_string())).await?;
    }
    let mut ping = tokio::time::interval(PING_EVERY);
    let mut last_heard = tokio::time::Instant::now();
    loop {
        tokio::select! {
            _ = ping.tick() => {
                if last_heard.elapsed() > SILENT_LIMIT {
                    bail!("no answer for {}s", SILENT_LIMIT.as_secs());
                }
                tx.send(Message::Ping(Bytes::new())).await?;
            }
            Some(text) = out_rx.recv() => tx.send(Message::text(text)).await?,
            msg = rx.next() => {
                last_heard = tokio::time::Instant::now();
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(v) = serde_json::from_str::<Value>(text.as_str()) {
                            handle(app, peer, addr, v).await;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Err(e.into()),
                }
            }
        }
    }
    tracing::info!("{} disconnected", peer.name);
    Ok(())
}

/// A control message from the other PC.
async fn handle(app: &Arc<App>, peer: &Peer, addr: SocketAddr, v: Value) {
    match v["t"].as_str() {
        Some("hello") => {
            if let Some(name) = v["name"].as_str() {
                rename(app, &peer.id, name);
            }
        }
        Some("battery") => {
            let Some(level) = v["level"].as_u64() else { return };
            let charging = v["charging"].as_bool().unwrap_or(false);
            app.peers.set_battery(&peer.id, (level.min(100) as u8, charging));
            app.emit(Event::Devices);
        }
        Some("clip") => {
            let Some(text) = v["text"].as_str() else { return };
            if !app.remember_clip(text) {
                return;
            }
            let chars = text.chars().count();
            tracing::info!("clipboard from {} ({chars} chars)", peer.name);
            match clipboard::set(text).await {
                Ok(()) => app.emit(Event::Clipboard { to_phone: false, device: peer.name.clone(), chars }),
                Err(e) => tracing::warn!("clipboard: {e}"),
            }
        }
        Some("file") => {
            let (Some(id), Some(name)) = (v["id"].as_str(), v["name"].as_str()) else { return };
            let (app, peer, id, name, size) = (app.clone(), peer.clone(), id.to_string(), name.to_string(), v["size"].as_u64());
            tokio::spawn(async move {
                if let Err(e) = download(&app, &peer, addr, &id, &name, size).await {
                    tracing::warn!("file from {}: {e:#}", peer.name);
                }
            });
        }
        _ => {}
    }
}

/// The other PC's name changed (its `hello`).
fn rename(app: &App, id: &str, name: &str) {
    let name: String = name.trim().chars().filter(|c| !c.is_control()).take(64).collect();
    let mut state = app.state.lock().unwrap();
    let Some(peer) = state.peers.iter_mut().find(|p| p.id == id) else { return };
    if name.is_empty() || peer.name == name {
        return;
    }
    peer.name = name;
    if let Err(e) = state.save() {
        tracing::warn!("state: {e:#}");
    }
    drop(state);
    app.emit(Event::Devices);
}

// ---------- files ----------

/// Downloads a file the other PC offered into the download folder.
async fn download(app: &Arc<App>, peer: &Peer, addr: SocketAddr, id: &str, name: &str, size: Option<u64>) -> Result<()> {
    let Some(name) = crate::server::sanitize_name(name) else { bail!("bad file name") };
    let mut progress = Progress::new(
        app,
        Transfer {
            id: crate::config::random_hex(12),
            outgoing: false,
            device: peer.name.clone(),
            name: name.clone(),
            size,
            done: 0,
            state: TransferState::Active,
            path: String::new(),
            started_at: now_secs(),
        },
    );
    let result = async {
        let req = authed(Request::get(format!("/download/{}", url_encode(id))), peer, addr).body(Full::new(Bytes::new()))?;
        let reply = request(addr, &client_config(&peer.fingerprint), req).await?;
        if !reply.status().is_success() {
            bail!("{} answered {}", peer.name, reply.status());
        }
        let body = reply.into_body().into_data_stream();
        save_stream(app, &name, None, body, &mut progress).await
    }
    .await;
    match result {
        Ok(path) => {
            let saved = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            progress.transfer.name = saved.clone();
            progress.transfer.path = path.to_string_lossy().into_owned();
            progress.finish(Ok(()));
            platform::notify_file(&peer.name, &saved, &path);
            Ok(())
        }
        Err(e) => {
            progress.finish(Err(format!("{e:#}")));
            Err(e)
        }
    }
}

/// Sends files to a linked PC, one after the other, in the background.
pub fn send_files(app: &Arc<App>, id: &str, paths: Vec<PathBuf>) -> Result<()> {
    let peer = app.state.lock().unwrap().peers.iter().find(|p| p.id == id).cloned().ok_or_else(|| anyhow!("no linked PC"))?;
    let addr = app.peers.addr(id).filter(|_| app.peers.online(id)).ok_or_else(|| anyhow!("{} is not connected", peer.name))?;
    let transfers: Vec<(PathBuf, Transfer)> = paths
        .into_iter()
        .map(|path| {
            let size = std::fs::metadata(&path).ok().map(|m| m.len());
            let transfer = Transfer {
                id: crate::config::random_hex(12),
                outgoing: true,
                device: peer.name.clone(),
                name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into()),
                size,
                done: 0,
                state: TransferState::Waiting,
                path: path.to_string_lossy().into_owned(),
                started_at: now_secs(),
            };
            app.put_transfer(transfer.clone());
            (path, transfer)
        })
        .collect();
    let app = app.clone();
    tokio::spawn(async move {
        for (path, mut transfer) in transfers {
            transfer.state = TransferState::Active;
            let mut progress = Progress::new(&app, transfer);
            match upload(&peer, addr, &path, &mut progress).await {
                Ok(()) => progress.finish(Ok(())),
                Err(e) => {
                    tracing::warn!("file to {}: {e:#}", peer.name);
                    progress.finish(Err(format!("{e:#}")));
                }
            }
        }
    });
    Ok(())
}

async fn upload(peer: &Peer, addr: SocketAddr, path: &PathBuf, progress: &mut Progress) -> Result<()> {
    let file = tokio::fs::File::open(path).await.with_context(|| path.display().to_string())?;
    let len = file.metadata().await?.len();
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    // Counts what has gone out through a channel: the body stream must be 'static.
    let (sent_tx, mut sent_rx) = mpsc::unbounded_channel::<u64>();
    let chunks = tokio_util::io::ReaderStream::with_capacity(file, 256 * 1024).map(move |chunk| {
        chunk.map(|c| {
            let _ = sent_tx.send(c.len() as u64);
            Frame::data(c)
        })
    });
    let req = authed(Request::post(format!("/upload?name={}", url_encode(&name))), peer, addr)
        .header(hyper::header::CONTENT_LENGTH, len)
        .body(StreamBody::new(chunks))?;
    let tls = client_config(&peer.fingerprint);
    let send = request(addr, &tls, req);
    tokio::pin!(send);
    let reply = loop {
        tokio::select! {
            reply = &mut send => break reply?,
            Some(n) = sent_rx.recv() => progress.add(n),
        }
    };
    while let Ok(n) = sent_rx.try_recv() {
        progress.add(n);
    }
    let status = reply.status();
    if !status.is_success() {
        let text = reply.into_body().collect().await.map(|b| b.to_bytes()).unwrap_or_default();
        bail!("{} refused it: {}", peer.name, String::from_utf8_lossy(&text).trim());
    }
    Ok(())
}

// ---------- connections ----------

fn authed(builder: hyper::http::request::Builder, peer: &Peer, addr: SocketAddr) -> hyper::http::request::Builder {
    builder
        .header(hyper::header::HOST, addr.to_string())
        .header(hyper::header::AUTHORIZATION, format!("Bearer {}", peer.token))
}

/// One HTTP/1.1 request on a new connection (a file per connection, like the phone).
async fn request<B>(addr: SocketAddr, tls: &Arc<ClientConfig>, req: Request<B>) -> Result<Response<Incoming>>
where
    B: hyper::body::Body + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    let stream = connect(addr, tls).await?;
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            tracing::debug!("peer connection: {e}");
        }
    });
    Ok(sender.send_request(req).await?)
}

async fn connect(addr: SocketAddr, tls: &Arc<ClientConfig>) -> Result<TlsStream<TcpStream>> {
    let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr)).await.map_err(|_| anyhow!("{addr}: timed out"))??;
    tcp.set_nodelay(true)?;
    let name = ServerName::IpAddress(addr.ip().into());
    let stream = tokio::time::timeout(CONNECT_TIMEOUT, TlsConnector::from(tls.clone()).connect(name, tcp))
        .await
        .map_err(|_| anyhow!("{addr}: TLS timed out"))??;
    Ok(stream)
}

/// Tries where the PC answered last, the addresses from pairing, then mDNS.
async fn connect_any(app: &App, peer: &Peer, tls: &Arc<ClientConfig>) -> Result<(SocketAddr, TlsStream<TcpStream>)> {
    let mut tried = Vec::new();
    let mut candidates: Vec<SocketAddr> = app.peers.addr(&peer.id).into_iter().collect();
    candidates.extend(peer.hosts.iter().filter_map(|h| h.parse().ok()).map(|ip| SocketAddr::new(ip, peer.port)));
    let mut last_error = anyhow!("no address to try");
    for round in 0..2 {
        if round == 1 {
            candidates = scan(Some(&peer.id))
                .await
                .into_iter()
                .flat_map(|n| {
                    let port = n.port;
                    n.hosts.into_iter().filter_map(move |h| h.parse().ok().map(|ip| SocketAddr::new(ip, port)))
                })
                .collect();
        }
        for addr in candidates.drain(..) {
            if tried.contains(&addr) {
                continue;
            }
            tried.push(addr);
            match connect(addr, tls).await {
                Ok(stream) => return Ok((addr, stream)),
                Err(e) => last_error = e,
            }
        }
    }
    Err(last_error)
}

/// Dotlink PCs on the network (`_dotlink._tcp`), listening for 3 s; with `id`, just that one
/// (as soon as it answers), for when its IP changed or to join it with a code.
async fn scan(id: Option<&str>) -> Vec<Nearby> {
    let id = id.map(str::to_string);
    tokio::task::spawn_blocking(move || {
        let Ok(mdns) = mdns_sd::ServiceDaemon::new() else { return Vec::new() };
        let Ok(events) = mdns.browse("_dotlink._tcp.local.") else { return Vec::new() };
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut found: Vec<Nearby> = Vec::new();
        while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
            let Ok(event) = events.recv_timeout(left) else { break };
            let mdns_sd::ServiceEvent::ServiceResolved(info) = event else { continue };
            let Some(pc_id) = info.get_property_val_str("id").map(str::to_string) else { continue };
            if id.as_ref().is_some_and(|id| *id != pc_id) {
                continue;
            }
            let instance = info.get_fullname().split("._dotlink").next().unwrap_or_default().to_string();
            let mut hosts: Vec<String> = info.get_addresses_v4().into_iter().map(|ip| ip.to_string()).collect();
            // Resolved on an interface without IPv4 (or before its address came): another
            // answer follows.
            if hosts.is_empty() {
                continue;
            }
            hosts.sort();
            let pc = Nearby {
                name: info.get_property_val_str("n").map(str::to_string).unwrap_or(instance),
                port: info.get_port(),
                fp: info.get_property_val_str("fp").unwrap_or_default().to_string(),
                hosts,
                linked: false,
                id: pc_id,
            };
            // The same PC answers once per network interface: merge its addresses.
            match found.iter_mut().find(|n| n.id == pc.id) {
                Some(n) => {
                    for h in pc.hosts {
                        if !n.hosts.contains(&h) {
                            n.hosts.push(h);
                        }
                    }
                }
                None => found.push(pc),
            }
            if id.is_some() {
                break;
            }
        }
        let _ = mdns.shutdown();
        found
    })
    .await
    .unwrap_or_default()
}

// ---------- TLS ----------

/// For joining with a code: accepts any certificate and records its fingerprint, which the
/// code exchange then binds to.
fn capturing_config() -> (Arc<ClientConfig>, Arc<Mutex<Option<String>>>) {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let seen = Arc::new(Mutex::new(None));
    let verifier = Arc::new(Pinned { fingerprint: None, seen: seen.clone(), provider: provider.clone() });
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("TLS versions")
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    (Arc::new(config), seen)
}

fn client_config(fingerprint: &str) -> Arc<ClientConfig> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let verifier = Arc::new(Pinned {
        fingerprint: Some(fingerprint.to_lowercase()),
        seen: Arc::default(),
        provider: provider.clone(),
    });
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("TLS versions")
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    Arc::new(config)
}

/// Trusts exactly the certificate whose SHA-256 is `fingerprint`: no CA, no host name. Without
/// a fingerprint it trusts any and records it in `seen` ([capturing_config]).
#[derive(Debug)]
struct Pinned {
    fingerprint: Option<String>,
    seen: Arc<Mutex<Option<String>>>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let fingerprint = sha256_hex(end_entity.as_ref());
        *self.seen.lock().unwrap() = Some(fingerprint.clone());
        if self.fingerprint.as_ref().is_none_or(|f| *f == fingerprint) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General("the PC's certificate isn't the one it was paired with".into()))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pairing_links() {
        let fp = "ab".repeat(32);
        let link = parse_link(&format!("dotlink://pair?h=192.168.1.5,10.0.0.2&p=47390&fp={fp}&c=secret&n=My%20PC&id=abc")).unwrap();
        assert_eq!(link.hosts, ["192.168.1.5", "10.0.0.2"]);
        assert_eq!(link.port, 47390);
        assert_eq!(link.name, "My PC");
        assert_eq!(link.id, "abc");
        assert!(parse_link("https://example.com").is_err());
        assert!(parse_link("dotlink://pair?h=1.2.3.4&p=1&fp=short&c=x").is_err());
    }
}
