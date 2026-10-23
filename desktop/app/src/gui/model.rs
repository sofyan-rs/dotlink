//! App-wide state the window shows. It outlives the window: closing it to the tray and
//! opening it again finds everything where it was.

use super::client::{Client, PairUpdate, Status, Update};
use dotlink_core::cast::CastState;
use dotlink_core::config::Config;
use dotlink_core::events::{Event, Transfer};
use dotlink_core::platform::{self, Check};
use futures::StreamExt;
use gpui::{App, AppContext, Context, Entity, EventEmitter};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Devices,
    Pair,
    Activity,
    Setup,
    Settings,
}

pub enum Pairing {
    Idle,
    Waiting { uri: String, since: Instant },
    Paired(String),
    Failed(String),
}

/// A short message for the window (shown as a toast) or the desktop (no window open).
#[derive(Clone)]
pub struct Notice {
    pub text: String,
    pub error: bool,
}

pub struct ClipNote {
    pub to_phone: bool,
    pub device: String,
    pub chars: usize,
    pub at: Instant,
}

pub struct Model {
    pub client: Client,
    /// The service runs inside this app (quitting the app stops it).
    pub owns_service: bool,
    pub connected: bool,
    pub status: Option<Status>,
    pub page: Page,
    pub pairing: Pairing,
    /// Bumped by each `start_pairing`; replies to an older one are ignored.
    pairing_id: u64,
    pub checks: Vec<Check>,
    pub checks_loading: bool,
    /// Setup check being fixed right now.
    pub fixing: Option<String>,
    pub clips: VecDeque<ClipNote>,
    pub autostart: bool,
    /// A setting changed that applies after a restart.
    pub restart_needed: bool,
    pub has_tray: bool,
    /// Camera/mic asked to start, waiting for the phone: (kind, when).
    pub starting: Vec<(&'static str, Instant)>,
    /// Device id that Send files / Send clipboard go to; None for every connected phone.
    pub send_to: Option<String>,
    /// Device id the webcam and mic start on, when several phones are connected.
    pub stream_from: Option<String>,
    /// A status request is on its way; `refresh_again` asks for one more after it (events come
    /// in bursts, e.g. a phone connecting sends Devices twice).
    refreshing: bool,
    refresh_again: bool,
}

/// [Model::fixing] while "Fix all" runs.
pub const FIX_ALL: &str = "*";

/// How long "Starting…" waits for the phone before giving up on it.
pub const START_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

impl EventEmitter<Notice> for Model {}

impl Model {
    pub fn new(client: Client, owns_service: bool, cx: &mut App) -> Entity<Self> {
        let model = cx.new(|_| Self {
            client: client.clone(),
            owns_service,
            connected: false,
            status: None,
            page: Page::Devices,
            pairing: Pairing::Idle,
            pairing_id: 0,
            checks: Vec::new(),
            checks_loading: false,
            fixing: None,
            clips: VecDeque::new(),
            autostart: {
                // Rewrite the entries each start, so they follow the app if it moved (or were
                // written with a wrong path by an older version).
                let on = platform::autostart::enabled();
                if on {
                    if let Err(e) = platform::autostart::set(true) {
                        tracing::warn!("autostart: {e:#}");
                    }
                }
                on
            },
            restart_needed: false,
            has_tray: false,
            starting: Vec::new(),
            send_to: None,
            stream_from: None,
            refreshing: false,
            refresh_again: false,
        });
        let mut updates = client.watch();
        model.update(cx, |_, cx| {
            cx.spawn(async move |this, cx| {
                while let Some(update) = updates.next().await {
                    if this.update(cx, |this, cx| this.apply(update, cx)).is_err() {
                        break;
                    }
                }
            })
            .detach();
        });
        model
    }

    fn apply(&mut self, update: Update, cx: &mut Context<Self>) {
        match update {
            Update::Connected => {
                self.connected = true;
                self.refresh(cx);
                self.refresh_checks(cx);
            }
            Update::Disconnected => {
                self.connected = false;
                cx.notify();
            }
            Update::Event(Event::Devices | Event::Config | Event::Streams | Event::Cast) => self.refresh(cx),
            Update::Event(Event::MediaPrompt { device, kind }) => {
                let what = if kind == "mic" { "microphone" } else { "webcam" };
                self.notice(format!("Tap Start in the notification on {device} to turn on the {what}"), false, cx);
            }
            Update::Event(Event::Paired { name }) => {
                self.pairing = Pairing::Paired(name);
                cx.notify();
            }
            Update::Event(Event::Transfer(t)) => self.put_transfer(t, cx),
            Update::Event(Event::Clipboard { to_phone, device, chars }) => {
                self.clips.push_front(ClipNote { to_phone, device, chars, at: Instant::now() });
                self.clips.truncate(20);
                cx.notify();
            }
        }
    }

    fn put_transfer(&mut self, t: Transfer, cx: &mut Context<Self>) {
        let Some(status) = self.status.as_mut() else { return };
        match status.transfers.iter_mut().find(|x| x.id == t.id) {
            Some(x) => *x = t,
            None => status.transfers.insert(0, t),
        }
        cx.notify();
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.refreshing {
            self.refresh_again = true;
            return;
        }
        self.refreshing = true;
        let status = self.client.status();
        cx.spawn(async move |this, cx| {
            let result = status.await;
            this.update(cx, |this, cx| {
                this.refreshing = false;
                if std::mem::take(&mut this.refresh_again) {
                    this.refresh(cx);
                }
                match result {
                    Ok(s) => {
                        // A stream that is now running is no longer "starting".
                        this.starting.retain(|(kind, _)| match *kind {
                            "camera" => s.camera.is_none(),
                            _ => s.mic.is_none(),
                        });
                        // The theme lives in config.toml, so it also follows edits made elsewhere.
                        super::theme::set_choice(super::theme::choice_from(&s.config.theme), cx);
                        this.status = Some(s)
                    }
                    Err(e) => tracing::debug!("status: {e:#}"),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn refresh_checks(&mut self, cx: &mut Context<Self>) {
        let port = self.status.as_ref().map_or(dotlink_core::config::DEFAULT_PORT, |s| s.port);
        self.checks_loading = true;
        cx.notify();
        let task = cx.background_executor().spawn(async move { platform::setup::checks(port) });
        cx.spawn(async move |this, cx| {
            let checks = task.await;
            this.update(cx, |this, cx| {
                this.checks = checks;
                this.checks_loading = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn fix(&mut self, id: String, cx: &mut Context<Self>) {
        self.fix_ids(id.clone(), vec![id], cx);
    }

    /// Everything Setup can fix, with one password prompt.
    pub fn fix_all(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<String> = self.checks.iter().filter(|c| !c.ok && c.fixable).map(|c| c.id.clone()).collect();
        if !ids.is_empty() {
            self.fix_ids(FIX_ALL.into(), ids, cx);
        }
    }

    /// `busy` is what [Self::fixing] shows meanwhile (one check's id, or [FIX_ALL]).
    fn fix_ids(&mut self, busy: String, ids: Vec<String>, cx: &mut Context<Self>) {
        let port = self.status.as_ref().map_or(dotlink_core::config::DEFAULT_PORT, |s| s.port);
        self.fixing = Some(busy);
        cx.notify();
        let (tx, rx) = futures::channel::oneshot::channel();
        // pkexec waits for the password dialog, downloads take a while; run on the tokio side.
        self.client.spawn(async move {
            let _ = tx.send(platform::setup::fix_many(&ids, port).await);
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.unwrap_or_else(|_| Err(anyhow::anyhow!("stopped")));
            this.update(cx, |this, cx| {
                this.fixing = None;
                match result {
                    Ok(()) => this.notice("Fixed", false, cx),
                    Err(e) if e.to_string() == "cancelled" => {}
                    Err(e) => this.notice(format!("{e:#}"), true, cx),
                }
                this.refresh_checks(cx);
            })
            .ok();
        })
        .detach();
    }

    pub fn notice(&mut self, text: impl Into<String>, error: bool, cx: &mut Context<Self>) {
        cx.emit(Notice { text: text.into(), error });
    }

    pub fn online_names(&self) -> Vec<String> {
        self.status
            .as_ref()
            .map(|s| s.devices.iter().filter(|d| d.online).map(|d| d.name.clone()).collect())
            .unwrap_or_default()
    }

    pub fn start_pairing(&mut self, cx: &mut Context<Self>) {
        let mut updates = self.client.pair();
        self.pairing_id += 1;
        let id = self.pairing_id;
        self.pairing = Pairing::Idle;
        cx.notify();
        cx.spawn(async move |this, cx| {
            while let Some(update) = updates.next().await {
                let alive = this.update(cx, |this, cx| {
                    // "New code" replaced this pairing: the service ends the old one with an error.
                    if this.pairing_id != id {
                        return;
                    }
                    this.pairing = match update {
                        PairUpdate::Uri(uri) => Pairing::Waiting { uri, since: Instant::now() },
                        PairUpdate::Paired(name) => {
                            this.notice(format!("Paired with {name}"), false, cx);
                            Pairing::Paired(name)
                        }
                        PairUpdate::Failed(_) if matches!(this.pairing, Pairing::Paired(_)) => return,
                        PairUpdate::Failed(e) => Pairing::Failed(e),
                    };
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// [Model::send_to] while that phone is online; otherwise every connected phone.
    pub fn send_target(&self) -> Option<String> {
        let id = self.send_to.as_ref()?;
        let online = self.status.as_ref()?.devices.iter().any(|d| &d.id == id && d.online);
        online.then(|| id.clone())
    }

    /// [Model::stream_from] while that phone is online; otherwise the first online phone.
    pub fn stream_source(&self) -> Option<String> {
        let online: Vec<&str> =
            self.status.as_ref()?.devices.iter().filter(|d| d.online).map(|d| d.id.as_str()).collect();
        let chosen = self.stream_from.as_deref().filter(|id| online.contains(id));
        chosen.or(online.first().copied()).map(str::to_string)
    }

    pub fn send_files(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        let send = self.client.send_files(paths, self.send_target());
        cx.spawn(async move |this, cx| {
            let result = send.await;
            this.update(cx, |this, cx| match result {
                Ok(msg) => this.notice(capitalize(&msg), false, cx),
                Err(e) => this.notice(capitalize(&format!("{e:#}")), true, cx),
            })
            .ok();
        })
        .detach();
    }

    pub fn send_clipboard(&mut self, cx: &mut Context<Self>) {
        let send = self.client.send_clipboard(self.send_target());
        cx.spawn(async move |this, cx| {
            let result = send.await;
            this.update(cx, |this, cx| match result {
                Ok(msg) => this.notice(capitalize(&msg), false, cx),
                Err(e) => this.notice(capitalize(&format!("{e:#}")), true, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Starts or stops casting a phone's screen (None: the first phone online). Errors that the
    /// cast state already explains (Wireless debugging off, needs pairing) aren't repeated as
    /// toasts.
    pub fn set_cast(&mut self, on: bool, id: Option<String>, cx: &mut Context<Self>) {
        let call = self.client.cast(on, id.as_deref());
        cx.spawn(async move |this, cx| {
            let result = call.await;
            this.update(cx, |this, cx| {
                this.refresh(cx);
                if let Err(e) = result {
                    let explained = this.status.as_ref().is_some_and(|s| {
                        s.casts.iter().filter(|(cast_id, _)| id.as_ref().is_none_or(|id| id == *cast_id)).any(
                            |(_, c)| matches!(c, CastState::NeedsWifiDebugging { .. } | CastState::NeedsPairing { .. }),
                        )
                    });
                    if !explained {
                        this.notice(capitalize(&format!("{e:#}")), true, cx);
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    /// Pairs with the phone's Wireless debugging, then casts it.
    pub fn pair_cast(&mut self, id: String, code: String, cx: &mut Context<Self>) {
        let call = self.client.cast_pair(&id, &code);
        cx.spawn(async move |this, cx| {
            let result = call.await;
            this.update(cx, |this, cx| match result {
                Ok(_) => {
                    this.notice("Paired. Casting…", false, cx);
                    this.set_cast(true, Some(id), cx);
                }
                Err(e) => this.notice(capitalize(&format!("{e:#}")), true, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Starts or stops the phone camera ("camera") or mic ("mic").
    pub fn set_stream(&mut self, kind: &'static str, on: bool, cx: &mut Context<Self>) {
        self.starting.retain(|(k, _)| *k != kind);
        if on {
            self.starting.push((kind, Instant::now()));
        }
        cx.notify();
        let call = self.client.media(kind, on, self.stream_source());
        cx.spawn(async move |this, cx| {
            let result = call.await;
            this.update(cx, |this, cx| {
                if let Err(e) = result {
                    this.starting.retain(|(k, _)| *k != kind);
                    this.notice(capitalize(&format!("{e:#}")), true, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The camera/mic was asked to start a moment ago and the phone hasn't begun yet.
    pub fn is_starting(&self, kind: &str) -> bool {
        self.starting.iter().any(|(k, at)| *k == kind && at.elapsed() < START_WAIT)
    }

    pub fn unpair(&mut self, id: String, cx: &mut Context<Self>) {
        let call = self.client.unpair(&id);
        cx.spawn(async move |this, cx| {
            let result = call.await;
            this.update(cx, |this, cx| match result {
                Ok(msg) => this.notice(capitalize(&msg), false, cx),
                Err(e) => this.notice(format!("{e:#}"), true, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Like [Self::save_config] without the "Saved" toast (switches that apply right away).
    pub fn save_config_quietly(&mut self, config: Config, cx: &mut Context<Self>) {
        let call = self.client.set_config(&config);
        cx.spawn(async move |this, cx| {
            let result = call.await;
            this.update(cx, |this, cx| {
                if let Err(e) = result {
                    this.notice(format!("{e:#}"), true, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn save_config(&mut self, config: Config, cx: &mut Context<Self>) {
        let call = self.client.set_config(&config);
        cx.spawn(async move |this, cx| {
            let result = call.await;
            this.update(cx, |this, cx| match result {
                Ok(restart) => {
                    this.restart_needed |= restart;
                    let msg = if restart { "Saved. Some changes apply after Dotlink restarts." } else { "Saved" };
                    this.notice(msg, false, cx);
                }
                Err(e) => this.notice(format!("{e:#}"), true, cx),
            })
            .ok();
        })
        .detach();
    }

    pub fn set_autostart(&mut self, on: bool, cx: &mut Context<Self>) {
        match platform::autostart::set(on) {
            Ok(()) => self.autostart = on,
            Err(e) => self.notice(format!("{e:#}"), true, cx),
        }
        cx.notify();
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(first) => first.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}
