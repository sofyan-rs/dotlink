//! Media control both ways (`media_state`, `media_cmd`) and pausing this PC's player during
//! the phone's calls (`call`).
//!
//! This PC's player ([platform::now_playing]) is read every couple of seconds while a phone is
//! connected and sent when it changes, with its artwork (base64 JPEG/PNG) only when the track
//! does. A phone's player arrives the same way and is kept in its [PhoneMedia] for the window,
//! its artwork in `config::config_dir()/media`.

use crate::config::sha256_hex;
use crate::events::Event;
use crate::platform::{self, Art, MediaCmd, NowPlaying};
use crate::server::App;
use anyhow::{Result, anyhow};
use base64::Engine as _;
use serde::Serialize;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// How often this PC's player is read while a phone is connected.
const POLL: Duration = Duration::from_secs(2);
/// While it plays, the position is sent again this often so the phone's bar stays true.
const RESYNC: Duration = Duration::from_secs(10);
/// Artwork larger than this isn't sent.
const MAX_ART: usize = 768 * 1024;

/// A phone's player, from its `media_state`, for its page.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PhoneMedia {
    pub player: String,
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    /// The artwork, saved here.
    pub art: Option<PathBuf>,
}

/// This PC's player as last sent, and whether a call paused it.
#[derive(Default)]
pub struct MediaState {
    /// The last `media_state` sent (with its artwork), for phones that connect later.
    last: std::sync::Mutex<Option<Value>>,
    /// A call paused this PC's player: resume it when the call ends.
    paused_for_call: std::sync::atomic::AtomicBool,
    /// Read again now (after a command), rather than at the next tick.
    poke: tokio::sync::Notify,
}

impl MediaState {
    /// The last `media_state`, for a phone that just connected.
    pub fn last(&self) -> Option<Value> {
        self.last.lock().unwrap().clone()
    }
}

/// The track, without what moves while it plays: a change here is a new `media_state`.
fn same_track(a: &NowPlaying, b: &NowPlaying) -> bool {
    (&a.player, &a.title, &a.artist, &a.album, a.duration_ms) == (&b.player, &b.title, &b.artist, &b.album, b.duration_ms)
}

/// The artwork's bytes: fetched or read once per track.
fn art_bytes(art: &Art) -> Option<Vec<u8>> {
    let bytes = match art {
        Art::Bytes(b) => b.clone(),
        Art::Url(url) if url.starts_with("file://") => {
            let path = crate::config::url_decode(url.trim_start_matches("file://"))?;
            std::fs::read(path).ok()?
        }
        Art::Url(url) if url.starts_with("http://") || url.starts_with("https://") => {
            let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(10))).build().into();
            let mut body = agent.get(url).call().ok()?.into_body();
            body.with_config().limit(MAX_ART as u64).read_to_vec().ok()?
        }
        Art::Url(_) => return None,
    };
    (bytes.len() <= MAX_ART).then_some(bytes)
}

fn state_msg(now: Option<&NowPlaying>, art: Option<&[u8]>) -> Value {
    let Some(n) = now else { return json!({ "t": "media_state", "player": Value::Null }) };
    let mut msg = json!({
        "t": "media_state",
        "player": n.player,
        "title": n.title,
        "artist": n.artist,
        "album": n.album,
        "playing": n.playing,
        "position": n.position_ms,
        "duration": n.duration_ms,
    });
    if let Some(v) = n.volume {
        msg["volume"] = json!((v * 100.0).round() as u32);
    }
    if let Some(art) = art {
        msg["art"] = json!(base64::engine::general_purpose::STANDARD.encode(art));
    }
    msg
}

/// Reads this PC's player while a phone is connected and tells the phones when it changes.
pub async fn watch(app: Arc<App>) {
    let mut last: Option<NowPlaying> = None;
    let mut art: Option<Vec<u8>> = None;
    let mut sent_at = tokio::time::Instant::now();
    loop {
        tokio::select! {
            _ = tokio::time::sleep(POLL) => {}
            _ = app.media.poke.notified() => {}
        }
        if !app.any_phone_online() {
            continue;
        }
        let now = tokio::task::spawn_blocking(platform::now_playing).await.ok().flatten();
        let new_track = match (&last, &now) {
            (Some(a), Some(b)) => !same_track(a, b),
            (None, None) => false,
            _ => true,
        };
        if new_track {
            let source = now.as_ref().and_then(|n| n.art.clone());
            art = match source {
                Some(source) => tokio::task::spawn_blocking(move || art_bytes(&source)).await.ok().flatten(),
                None => None,
            };
        }
        let changed = new_track
            || match (&last, &now) {
                (Some(a), Some(b)) => a.playing != b.playing || a.volume != b.volume || b.playing && sent_at.elapsed() >= RESYNC,
                _ => false,
            };
        // A seek: the position moved more than the time that passed.
        let jumped = match (&last, &now) {
            (Some(a), Some(b)) if !new_track => {
                let expected = a.position_ms + if a.playing { POLL.as_millis() as u64 } else { 0 };
                b.position_ms.abs_diff(expected) > 3000
            }
            _ => false,
        };
        if changed || jumped {
            let msg = state_msg(now.as_ref(), if new_track { art.as_deref() } else { None });
            // Kept with its artwork, for a phone that connects later.
            *app.media.last.lock().unwrap() = Some(state_msg(now.as_ref(), art.as_deref()));
            app.send_control_to_phones(msg);
            sent_at = tokio::time::Instant::now();
        }
        last = now;
    }
}

/// A command from a phone's Media card.
pub async fn command_from_phone(app: &App, v: &Value) {
    let Some(cmd) = MediaCmd::from_msg(v) else { return };
    match tokio::task::spawn_blocking(move || platform::media_command(&cmd)).await {
        Ok(Err(e)) => tracing::warn!("media: {e:#}"),
        Err(e) => tracing::warn!("media: {e}"),
        Ok(Ok(())) => {}
    }
    app.media.poke.notify_one();
}

/// A phone's player (`media_state`): kept for its page, the artwork saved next to the config.
pub fn state_from_phone(app: &App, device: &str, v: &Value) {
    let mut phones = app.phones.lock().unwrap();
    let phone = phones.entry(device.to_string()).or_default();
    if v["player"].is_null() {
        phone.media = None;
    } else {
        let old_art = phone.media.as_ref().and_then(|m| m.art.clone());
        let art = match v["art"].as_str().and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok()) {
            Some(bytes) => save_art(device, &bytes).or(old_art),
            None => old_art,
        };
        let text = |k: &str| v[k].as_str().unwrap_or_default().to_string();
        phone.media = Some(PhoneMedia {
            player: text("player"),
            title: text("title"),
            artist: text("artist"),
            playing: v["playing"].as_bool().unwrap_or(false),
            position_ms: v["position"].as_u64().unwrap_or(0),
            duration_ms: v["duration"].as_u64().unwrap_or(0),
            art,
        });
    }
    drop(phones);
    app.emit(Event::Devices);
}

/// Saves a phone's artwork as `media/<device>-<hash>.img` (a new name per picture, so the
/// window doesn't show a cached old one), removing the device's older ones.
fn save_art(device: &str, bytes: &[u8]) -> Option<PathBuf> {
    let dir = crate::config::config_dir().join("media");
    std::fs::create_dir_all(&dir).ok()?;
    let prefix = sha256_hex(device.as_bytes())[..12].to_string();
    let path = dir.join(format!("{prefix}-{}.img", &sha256_hex(bytes)[..12]));
    if !path.exists() {
        for old in std::fs::read_dir(&dir).ok()?.flatten() {
            if old.file_name().to_string_lossy().starts_with(&prefix) {
                let _ = std::fs::remove_file(old.path());
            }
        }
        std::fs::write(&path, bytes).ok()?;
    }
    Some(path)
}

/// Controls a phone's player from its page or `dotlink media`.
pub fn command_to_phone(app: &App, key: &str, cmd: &str, value: Option<u64>) -> Result<String> {
    let target = app.target_of(Some(key), true)?;
    let (id, name) = target.phones.into_iter().next().ok_or_else(|| anyhow!("no connected phone \"{key}\""))?;
    let mut msg = json!({ "t": "media_cmd", "cmd": cmd });
    match (cmd, value) {
        ("seek", Some(ms)) => msg["ms"] = json!(ms),
        ("volume", Some(level)) => msg["level"] = json!(level.min(100)),
        ("seek" | "volume", None) => anyhow::bail!("{cmd} needs a value"),
        _ => {}
    }
    app.send_control_to(Some(&id), msg);
    Ok(name)
}

/// The phone's call state (`call`): a ringing or answered call pauses this PC's player if it
/// plays, and the call's end resumes it, only when it was this that paused it.
pub async fn call(app: &App, state: &str) {
    use std::sync::atomic::Ordering;
    match state {
        "ringing" | "offhook" => {
            if app.media.paused_for_call.load(Ordering::Relaxed) {
                return;
            }
            let playing = tokio::task::spawn_blocking(platform::now_playing).await.ok().flatten().is_some_and(|n| n.playing);
            if playing && matches!(tokio::task::spawn_blocking(|| platform::media_command(&MediaCmd::Pause)).await, Ok(Ok(()))) {
                tracing::info!("paused the player for a call");
                app.media.paused_for_call.store(true, Ordering::Relaxed);
            }
        }
        "idle" if app.media.paused_for_call.swap(false, Ordering::Relaxed) => {
            tracing::info!("call ended: playing again");
            let _ = tokio::task::spawn_blocking(|| platform::media_command(&MediaCmd::Play)).await;
        }
        _ => {}
    }
    app.media.poke.notify_one();
}
