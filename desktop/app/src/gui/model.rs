//! App-wide state the window shows. It outlives the window: closing it to the tray and
//! opening it again finds everything where it was.

use super::client::{Client, DeviceInfo, NearbyPc, PairUpdate, Status, Update};
use dotlink_core::cast::CastState;
use dotlink_core::config::Config;
use dotlink_core::events::{Event, Transfer};
use dotlink_core::platform::{self, Check};
use dotlink_core::update::{self, Release};
use futures::StreamExt;
use serde_json::json;
use gpui::{App, AppContext, Context, Entity, EventEmitter};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    /// One phone or PC ([Model::current_device]): its actions, settings and recent transfers.
    Device,
    Pair,
    Activity,
    Setup,
    Settings,
}

/// A phone's folder open on its page (Browse from PC).
pub struct PhoneFolder {
    pub device: String,
    /// `/` is the phone's internal storage.
    pub path: String,
    pub entries: Vec<dotlink_core::files::Entry>,
    pub loading: bool,
    pub error: Option<String>,
}

impl PhoneFolder {
    /// `name` inside this folder.
    pub fn child(&self, name: &str) -> String {
        format!("{}/{name}", self.path.trim_end_matches('/'))
    }

    /// The folder above, None at the top.
    pub fn parent(&self) -> Option<String> {
        let path = self.path.trim_end_matches('/');
        (!path.is_empty()).then(|| path.rsplit_once('/').map(|(up, _)| up).filter(|up| !up.is_empty()).unwrap_or("/").to_string())
    }
}

pub enum Pairing {
    Idle,
    /// `code`: what another PC types to join this one.
    Waiting { uri: String, code: String, since: Instant },
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
    /// The phone folder open on a device page.
    pub folder: Option<PhoneFolder>,
    pub checks: Vec<Check>,
    pub checks_loading: bool,
    /// Setup check being fixed right now.
    pub fixing: Option<String>,
    pub clips: VecDeque<ClipNote>,
    pub autostart: bool,
    /// A setting changed that applies after a restart.
    pub restart_needed: bool,
    pub has_tray: bool,
    /// Camera/mic asked to start, waiting for the phone: (kind, device id, when).
    pub starting: Vec<(&'static str, String, Instant)>,
    /// The device the Device page shows, as picked in the sidebar; see [Model::current_device].
    pub device: Option<String>,
    /// Other Dotlink PCs on the network (the Pair page's Join a PC), and whether a scan runs.
    pub nearby: Vec<NearbyPc>,
    pub nearby_loading: bool,
    /// A status request is on its way; `refresh_again` asks for one more after it (events come
    /// in bursts, e.g. a phone connecting sends Devices twice).
    refreshing: bool,
    refresh_again: bool,
    pub update: UpdateState,
    /// The version the last "is available" toast was about, so the periodic check says it once.
    update_announced: Option<String>,
}

pub enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    /// Downloading, with progress 0.0–1.0.
    Downloading(Release, f32),
    Failed(String),
}

/// The tray app runs for days: look for updates this often (and shortly after starting).
const UPDATE_EVERY: std::time::Duration = std::time::Duration::from_secs(12 * 60 * 60);

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
            page: Page::Device,
            pairing: Pairing::Idle,
            pairing_id: 0,
            checks: Vec::new(),
            folder: None,
            checks_loading: false,
            fixing: None,
            clips: VecDeque::new(),
            autostart: {
                // Rewrite the entries each start, so they follow the app if it moved (or were
                // written with a wrong path by an older version).
                let on = platform::autostart::enabled();
                if on && let Err(e) = platform::autostart::set(true) {
                    tracing::warn!("autostart: {e:#}");
                }
                on
            },
            restart_needed: false,
            has_tray: false,
            starting: Vec::new(),
            device: None,
            nearby: Vec::new(),
            nearby_loading: false,
            refreshing: false,
            refresh_again: false,
            update: UpdateState::Idle,
            update_announced: None,
        });
        model.update(cx, |_, cx| {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(std::time::Duration::from_secs(10)).await;
                loop {
                    if this.update(cx, |this, cx| this.check_update(false, cx)).is_err() {
                        break;
                    }
                    cx.background_executor().timer(UPDATE_EVERY).await;
                }
            })
            .detach();
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
            Update::Event(
                Event::Devices | Event::Config | Event::Streams | Event::Cast | Event::Incoming(_) | Event::IncomingDone { .. },
            ) => self.refresh(cx),
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
                        this.starting.retain(|(kind, _, _)| match *kind {
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

    /// Looks for a newer release. `manual`: the user asked, so failures show; otherwise a found
    /// update is announced once.
    pub fn check_update(&mut self, manual: bool, cx: &mut Context<Self>) {
        if matches!(self.update, UpdateState::Checking | UpdateState::Downloading(..)) {
            return;
        }
        let before = std::mem::replace(&mut self.update, UpdateState::Checking);
        if manual {
            cx.notify();
        }
        let task = cx.background_executor().spawn(async move { update::check() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.update = match result {
                    Ok(Some(release)) => {
                        if !manual && this.update_announced.as_deref() != Some(&release.version) {
                            this.update_announced = Some(release.version.clone());
                            this.notice(format!("Dotlink {} is available: Settings → About", release.version), false, cx);
                        }
                        UpdateState::Available(release)
                    }
                    Ok(None) => UpdateState::UpToDate,
                    Err(e) if manual => UpdateState::Failed(format!("{e:#}")),
                    // A quiet check failing (offline) shouldn't hide what was known.
                    Err(e) => {
                        tracing::info!("update check: {e:#}");
                        match before {
                            UpdateState::Checking | UpdateState::Downloading(..) => UpdateState::Idle,
                            other => other,
                        }
                    }
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Downloads and starts the update, then quits (the update starts Dotlink again). Without a
    /// file for this kind of install, opens the release page.
    pub fn install_update(&mut self, cx: &mut Context<Self>) {
        let UpdateState::Available(release) = &self.update else { return };
        let release = release.clone();
        let Some(asset) = release.asset.clone() else {
            cx.open_url(&release.page_url);
            return;
        };
        self.update = UpdateState::Downloading(release.clone(), 0.);
        cx.notify();
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<f32>();
        let task = cx.background_executor().spawn(async move {
            let path = update::download(&asset, |p| {
                let _ = tx.unbounded_send(p);
            })?;
            update::launch(&path)
        });
        cx.spawn(async move |this, cx| {
            while let Some(p) = rx.next().await {
                let alive = this.update(cx, |this, cx| {
                    if let UpdateState::Downloading(_, progress) = &mut this.update {
                        // Repaint per percent, not per 64 KB.
                        if (p * 100.) as u32 != (*progress * 100.) as u32 {
                            *progress = p;
                            cx.notify();
                        }
                    }
                });
                if alive.is_err() {
                    return;
                }
            }
            let result = task.await;
            this.update(cx, |this, cx| match result {
                Ok(()) => super::quit(cx),
                Err(e) => {
                    this.update = UpdateState::Available(release);
                    this.notice(format!("Update failed: {e:#}"), true, cx);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn notice(&mut self, text: impl Into<String>, error: bool, cx: &mut Context<Self>) {
        cx.emit(Notice { text: text.into(), error });
    }

    /// Connected (phones, PCs).
    pub fn online_counts(&self) -> (usize, usize) {
        let online = self.status.iter().flat_map(|s| &s.devices).filter(|d| d.online);
        online.fold((0, 0), |(phones, pcs), d| if d.is_phone() { (phones + 1, pcs) } else { (phones, pcs + 1) })
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
                        PairUpdate::Uri { uri, code } => Pairing::Waiting { uri, code, since: Instant::now() },
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

    /// Paired phones and linked PCs in the sidebar's order: online first, phones before PCs.
    pub fn devices(&self) -> Vec<&DeviceInfo> {
        let mut devices: Vec<&DeviceInfo> = self.status.iter().flat_map(|s| &s.devices).collect();
        devices.sort_by_key(|d| (!d.online, !d.is_phone()));
        devices
    }

    /// The device the Device page shows: the one picked in the sidebar while it's still paired,
    /// else the first in the sidebar.
    pub fn current_device(&self) -> Option<&DeviceInfo> {
        let devices = self.devices();
        let picked = self.device.as_deref().and_then(|id| devices.iter().find(|d| d.id == id));
        picked.or(devices.first()).copied()
    }

    /// Shows a device's page.
    pub fn open_device(&mut self, id: String, cx: &mut Context<Self>) {
        self.device = Some(id);
        self.page = Page::Device;
        cx.notify();
    }

    /// Sends files to one device.
    pub fn send_files_to(&mut self, paths: Vec<PathBuf>, id: String, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        let call = self.client.send_files(paths, Some(id));
        self.report_all(vec![call], cx);
    }

    /// Sends this PC's clipboard to one device.
    pub fn send_clipboard_to(&mut self, id: String, cx: &mut Context<Self>) {
        let call = self.client.send_clipboard(Some(id));
        self.report_all(vec![call], cx);
    }

    /// Opens the link on this PC's clipboard on one device.
    pub fn send_link_to(&mut self, id: String, cx: &mut Context<Self>) {
        let call = self.client.send_link(Some(id));
        self.report_all(vec![call], cx);
    }

    /// The tray's Send link: the clipboard's link, on every connected device.
    pub fn send_link(&mut self, cx: &mut Context<Self>) {
        let call = self.client.send_link(None);
        self.report_all(vec![call], cx);
    }

    /// Opens `path` on phone `id` in its page's Files.
    pub fn open_folder(&mut self, id: String, path: String, cx: &mut Context<Self>) {
        let kept = self.folder.as_ref().filter(|f| f.device == id && f.path == path).map(|f| f.entries.clone());
        self.folder = Some(PhoneFolder { device: id.clone(), path: path.clone(), entries: kept.unwrap_or_default(), loading: true, error: None });
        cx.notify();
        let call = self.client.fs(&id, "list", json!({ "path": path }));
        cx.spawn(async move |this, cx| {
            let result = call.await;
            this.update(cx, |this, cx| {
                // Another folder was opened meanwhile.
                let Some(f) = this.folder.as_mut().filter(|f| f.device == id && f.path == path) else { return };
                f.loading = false;
                match result {
                    Ok(v) => f.entries = serde_json::from_value(v["entries"].clone()).unwrap_or_default(),
                    Err(e) => f.error = Some(capitalize(&format!("{e:#}"))),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn close_folder(&mut self, cx: &mut Context<Self>) {
        self.folder = None;
        cx.notify();
    }

    fn reload_folder(&mut self, cx: &mut Context<Self>) {
        if let Some(f) = &self.folder {
            let (id, path) = (f.device.clone(), f.path.clone());
            self.open_folder(id, path, cx);
        }
    }

    /// Downloads `name` from the open folder into the download folder; opens it after when
    /// `open`.
    pub fn download_phone_file(&mut self, name: String, open: bool, cx: &mut Context<Self>) {
        let Some(f) = &self.folder else { return };
        let call = self.client.fs(&f.device, "get", json!({ "path": f.child(&name) }));
        self.notice(format!("Downloading {name}…"), false, cx);
        cx.spawn(async move |this, cx| {
            let result = call.await;
            this.update(cx, |this, cx| match result {
                Ok(v) => {
                    let path = PathBuf::from(v["path"].as_str().unwrap_or_default());
                    if open {
                        platform::open_path(&path);
                    } else {
                        this.notice(format!("Saved {}", path.file_name().unwrap_or_default().to_string_lossy()), false, cx);
                    }
                }
                Err(e) => this.notice(capitalize(&format!("{e:#}")), true, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Sends files into the open folder.
    pub fn upload_to_folder(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let Some(f) = &self.folder else { return };
        let paths: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        let call = self.client.fs(&f.device, "put", json!({ "path": f.path, "paths": paths }));
        self.folder_op(call, cx);
    }

    /// Deletes `name` (a folder with what's in it) in the open folder.
    pub fn delete_phone_file(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(f) = &self.folder else { return };
        let call = self.client.fs(&f.device, "rm", json!({ "path": f.child(&name) }));
        self.folder_op(call, cx);
    }

    pub fn rename_phone_file(&mut self, name: String, to: String, cx: &mut Context<Self>) {
        let Some(f) = &self.folder else { return };
        let call = self.client.fs(&f.device, "mv", json!({ "path": f.child(&name), "to": f.child(&to) }));
        self.folder_op(call, cx);
    }

    pub fn new_phone_folder(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(f) = &self.folder else { return };
        let call = self.client.fs(&f.device, "mkdir", json!({ "path": f.child(&name) }));
        self.folder_op(call, cx);
    }

    /// A change in the open folder: an error toast, then the folder again.
    fn folder_op(&mut self, call: impl std::future::Future<Output = anyhow::Result<serde_json::Value>> + 'static, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let result = call.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(v) if v["ok"].as_str().is_some_and(|ok| ok.starts_with("sending")) => {
                        this.notice(capitalize(v["ok"].as_str().unwrap_or_default()), false, cx)
                    }
                    Ok(_) => {}
                    Err(e) => this.notice(capitalize(&format!("{e:#}")), true, cx),
                }
                this.reload_folder(cx);
            })
            .ok();
        })
        .detach();
    }

    /// Shows a phone's notifications here (`option` "notifications") or copies their codes
    /// ("copy_codes").
    pub fn set_device_option(&mut self, id: String, option: &'static str, on: bool, cx: &mut Context<Self>) {
        let call = self.client.device_option(&id, option, on);
        cx.spawn(async move |this, cx| {
            let result = call.await;
            this.update(cx, |this, cx| {
                if let Err(e) = result {
                    this.notice(capitalize(&format!("{e:#}")), true, cx);
                }
                this.refresh(cx);
            })
            .ok();
        })
        .detach();
    }

    /// Dismiss on a phone's notification (macOS: from Notification Center).
    pub fn dismiss_phone_notification(&mut self, tag: String, cx: &mut Context<Self>) {
        let call = self.client.dismiss_phone_notification(&tag);
        cx.spawn(async move |_, _| {
            if let Err(e) = call.await {
                tracing::debug!("dismiss: {e:#}");
            }
        })
        .detach();
    }

    /// Lets Android 15+ show Dotlink the codes in notifications; the toast says how it went.
    pub fn allow_codes(&mut self, id: String, cx: &mut Context<Self>) {
        let call = self.client.allow_codes(&id);
        self.report_all(vec![call], cx);
    }

    /// Rings a phone, or stops it; the header shows RINGING once the phone says it rings.
    pub fn ring(&mut self, id: String, on: bool, cx: &mut Context<Self>) {
        let call = self.client.ring(&id, on);
        self.report_all(vec![call], cx);
    }

    /// One toast for several sends: the errors if any, else what was done.
    fn report_all(
        &mut self,
        calls: Vec<impl std::future::Future<Output = anyhow::Result<String>> + 'static>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let mut ok = Vec::new();
            let mut failed = Vec::new();
            for call in calls {
                match call.await {
                    Ok(msg) => ok.push(msg),
                    Err(e) => failed.push(format!("{e:#}")),
                }
            }
            this.update(cx, |this, cx| {
                if failed.is_empty() {
                    this.notice(capitalize(&ok.join("; ")), false, cx)
                } else {
                    this.notice(capitalize(&failed.join("; ")), true, cx)
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn send_files(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        // The tray's Send files: every connected device.
        let send = self.client.send_files(paths, None);
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
        let send = self.client.send_clipboard(None);
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

    /// Starts or stops the camera ("camera") or mic ("mic") of the phone `id`.
    pub fn set_stream(&mut self, kind: &'static str, on: bool, id: String, cx: &mut Context<Self>) {
        self.starting.retain(|(k, _, _)| *k != kind);
        if on {
            self.starting.push((kind, id.clone(), Instant::now()));
        }
        cx.notify();
        let call = self.client.media(kind, on, Some(id));
        cx.spawn(async move |this, cx| {
            let result = call.await;
            this.update(cx, |this, cx| {
                if let Err(e) = result {
                    this.starting.retain(|(k, _, _)| *k != kind);
                    this.notice(capitalize(&format!("{e:#}")), true, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The camera/mic was asked to start on the phone `id` a moment ago and it hasn't begun yet.
    pub fn is_starting(&self, kind: &str, id: &str) -> bool {
        self.starting.iter().any(|(k, d, at)| *k == kind && d == id && at.elapsed() < START_WAIT)
    }

    /// Opens the current pairing link on the phones on the cable; the result is a toast.
    pub fn pair_usb(&mut self, cx: &mut Context<Self>) {
        let Pairing::Waiting { uri, .. } = &self.pairing else { return };
        let call = self.client.pair_usb(uri);
        cx.spawn(async move |this, cx| {
            let result = call.await;
            this.update(cx, |this, cx| match result {
                Ok(msg) => this.notice(msg, false, cx),
                Err(e) => this.notice(capitalize(&format!("{e:#}")), true, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Looks for the other Dotlink PCs on the network.
    pub fn refresh_nearby(&mut self, cx: &mut Context<Self>) {
        if self.nearby_loading {
            return;
        }
        self.nearby_loading = true;
        cx.notify();
        let call = self.client.nearby();
        cx.spawn(async move |this, cx| {
            let result = call.await;
            this.update(cx, |this, cx| {
                this.nearby_loading = false;
                match result {
                    Ok(pcs) => this.nearby = pcs,
                    Err(e) => this.notice(capitalize(&format!("{e:#}")), true, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Links this PC to the PC `id` with the code it shows. Calls `done` once it worked.
    pub fn join_code(&mut self, id: String, code: String, done: impl FnOnce(&mut App) + 'static, cx: &mut Context<Self>) {
        let call = self.client.join_code(&id, &code);
        cx.spawn(async move |this, cx| {
            let result = call.await;
            this.update(cx, |this, cx| match result {
                Ok(msg) => {
                    this.notice(capitalize(&msg), false, cx);
                    this.page = Page::Device;
                    this.refresh(cx);
                    done(cx);
                }
                Err(e) => this.notice(capitalize(&format!("{e:#}")), true, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Links this PC to the one whose pairing link this is. Calls `done` once it worked.
    pub fn join(&mut self, link: String, done: impl FnOnce(&mut App) + 'static, cx: &mut Context<Self>) {
        let call = self.client.join(link.trim());
        cx.spawn(async move |this, cx| {
            let result = call.await;
            this.update(cx, |this, cx| match result {
                Ok(msg) => {
                    this.notice(capitalize(&msg), false, cx);
                    this.page = Page::Device;
                    this.refresh(cx);
                    done(cx);
                }
                Err(e) => this.notice(capitalize(&format!("{e:#}")), true, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Names a device on this PC; the toast says what changed.
    pub fn rename_device(&mut self, id: String, name: String, cx: &mut Context<Self>) {
        let call = self.client.rename(&id, &name);
        cx.spawn(async move |this, cx| {
            let result = call.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(msg) => this.notice(capitalize(&msg), false, cx),
                    Err(e) => this.notice(capitalize(&format!("{e:#}")), true, cx),
                }
                this.refresh(cx);
            })
            .ok();
        })
        .detach();
    }

    pub fn set_clipboard_auto(&mut self, id: String, on: bool, cx: &mut Context<Self>) {
        let call = self.client.clipboard_auto(&id, on);
        cx.spawn(async move |this, cx| {
            let result = call.await;
            this.update(cx, |this, cx| {
                if let Err(e) = result {
                    this.notice(capitalize(&format!("{e:#}")), true, cx);
                }
                this.refresh(cx);
            })
            .ok();
        })
        .detach();
    }

    /// Activity → Clear: the finished and failed transfers go; waiting and running ones stay.
    pub fn clear_transfers(&mut self, cx: &mut Context<Self>) {
        let call = self.client.clear_transfers();
        cx.spawn(async move |this, cx| {
            let result = call.await;
            this.update(cx, |this, cx| {
                if let Err(e) = result {
                    this.notice(capitalize(&format!("{e:#}")), true, cx);
                }
                this.refresh(cx);
            })
            .ok();
        })
        .detach();
    }

    /// Accept or Decline for a file waiting on it ("Ask before receiving files").
    pub fn answer_incoming(&mut self, id: String, accept: bool, cx: &mut Context<Self>) {
        let call = self.client.answer_incoming(&id, accept);
        cx.spawn(async move |this, cx| {
            let result = call.await;
            this.update(cx, |this, cx| {
                if let Err(e) = result {
                    this.notice(capitalize(&format!("{e:#}")), true, cx);
                }
                this.refresh(cx);
            })
            .ok();
        })
        .detach();
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
