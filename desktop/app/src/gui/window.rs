//! The Dotlink window: a sidebar and five pages.

use super::client::{Battery, DeviceInfo, Status};
use super::model::{Model, Notice, Page, Pairing, UpdateState};
use super::theme::{self, DISPLAY, MONO, Palette, palette};
use super::title_bar::TitleBar;
use dotlink_core::cast::CastState;
use dotlink_core::config::Config;
use dotlink_core::events::{Transfer, TransferState};
use dotlink_core::platform;
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, ClickEvent, Context, Decorations, Div, Entity, ExternalPaths, FontWeight, Hsla,
    PathPromptOptions, SharedString, Stateful, Subscription, Window, canvas, div, fill, point, px, size,
};
use gpui_component::WindowExt as _;
use gpui_component::input::{Input, InputState};
use gpui_component::notification::Notification;
use gpui_component::button::Button;
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_component::switch::Switch;
use gpui_component::{ActiveTheme as _, Icon};
use gpui_kit_assets::IconName;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Line height of a row's title, so things beside it can line up with it.
const TITLE_LINE: f32 = 22.;

/// Corner radius of controls (buttons, nav items, segmented control) and of cards: slight,
/// not pills.
const RADIUS: f32 = 8.;
const CARD_RADIUS: f32 = 12.;

/// Height of every control (pills, text fields), so they line up side by side.
const CONTROL: f32 = 38.;

pub struct MainView {
    model: Entity<Model>,
    form: SettingsForm,
    /// The 6-digit Wireless debugging pairing code, when Cast needs pairing.
    cast_code: Entity<InputState>,
    /// The pairing QR code, encoded once per URI (the page repaints every second).
    qr: Option<(String, Arc<Qr>)>,
    /// The Pair page's tab: show this PC's code (0) or join another PC (1).
    pair_tab: usize,
    /// Another PC's pairing link, pasted on the Pair page.
    join_link: Entity<InputState>,
    /// The PC picked on the Pair page's Join a PC, and the code it shows.
    join_pick: Option<String>,
    join_code: Entity<InputState>,
    _subs: Vec<Subscription>,
}

/// The Settings page's fields, filled from the service's config once it arrives.
struct SettingsForm {
    loaded: bool,
    name: Entity<InputState>,
    download_dir: Entity<InputState>,
    shell: Entity<InputState>,
    terminal_pin: Entity<InputState>,
    port: Entity<InputState>,
    camera_size: Entity<InputState>,
    cast_max_size: Entity<InputState>,
    clipboard_sync: bool,
    ask_before_receiving: bool,
    terminal: bool,
    cast_screen_off: bool,
    cast_audio: bool,
}

impl MainView {
    pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = |placeholder: &str, window: &mut Window, cx: &mut Context<Self>| {
            let placeholder = placeholder.to_string();
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        let form = SettingsForm {
            loaded: false,
            name: input("This PC's hostname", window, cx),
            download_dir: input("~/Downloads/Dotlink", window, cx),
            shell: input("Your login shell", window, cx),
            terminal_pin: cx.new(|cx| InputState::new(window, cx).placeholder("No PIN").masked(true)),
            port: input("47390", window, cx),
            camera_size: input("1920x1080", window, cx),
            cast_max_size: input("Full resolution", window, cx),
            clipboard_sync: true,
            ask_before_receiving: false,
            terminal: true,
            cast_screen_off: false,
            cast_audio: true,
        };
        let subs = vec![
            cx.observe(&model, |_, _, cx| cx.notify()),
            cx.subscribe_in(&model, window, |_, _, notice: &Notice, window, cx| {
                // Our own icon + text row: the built-in typed toast pins its icon 18px from the
                // top (tuned for a 16px rem), which sits low next to our 15px text.
                let (icon, color) = if notice.error {
                    (gpui_component::IconName::CircleX, cx.theme().danger)
                } else {
                    (gpui_component::IconName::CircleCheck, cx.theme().success)
                };
                let text = SharedString::from(notice.text.clone());
                let note = Notification::new().content(move |_, _, _| {
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_sm()
                        .child(Icon::new(icon.clone()).flex_none().text_color(color))
                        .child(text.clone())
                        .into_any_element()
                });
                window.push_notification(note, cx);
            }),
            cx.observe_window_appearance(window, |_, window, cx| theme::apply(Some(window), cx)),
            // GNOME's button layout changed (Tweaks → Window Titlebars): redraw the title bar.
            cx.observe_button_layout_changed(window, |_, _, cx| cx.notify()),
        ];
        // Ticks the pairing countdown.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let waiting = this.update(cx, |this, cx| {
                    let m = this.model.read(cx);
                    // Pairing countdown, and "Starting…" running out; only on the page showing it.
                    let ticking = match m.page {
                        Page::Pair => matches!(m.pairing, Pairing::Waiting { .. }),
                        Page::Phones => !m.starting.is_empty(),
                        _ => false,
                    };
                    if ticking {
                        cx.notify();
                    }
                });
                if waiting.is_err() {
                    break;
                }
            }
        })
        .detach();
        let cast_code = cx.new(|cx| InputState::new(window, cx).placeholder("6-digit code"));
        let join_link = cx.new(|cx| InputState::new(window, cx).placeholder("dotlink://pair?…"));
        let join_code = cx.new(|cx| InputState::new(window, cx).placeholder("6-digit code"));
        Self { model, form, cast_code, qr: None, pair_tab: 0, join_link, join_pick: None, join_code, _subs: subs }
    }

    fn go(&mut self, page: Page, cx: &mut Context<Self>) {
        self.model.update(cx, |m, cx| {
            m.page = page;
            match page {
                Page::Pair if !matches!(m.pairing, Pairing::Waiting { .. }) => m.start_pairing(cx),
                Page::Setup => m.refresh_checks(cx),
                _ => {}
            }
            cx.notify();
        });
    }

    /// Picks files and sends them to the phones (`phones`) or the PCs, as chosen on that page.
    fn pick_and_send(&mut self, phones: bool, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Send".into()),
        });
        let model = self.model.clone();
        cx.spawn(async move |_, cx| {
            if let Ok(Ok(Some(paths))) = picked.await {
                model.update(cx, |m, cx| m.send_files_to(paths, phones, cx));
            }
        })
        .detach();
    }

    /// Copies the service's config into the form the first time it is known.
    fn load_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.form.loaded {
            return;
        }
        let Some(config) = self.model.read(cx).status.as_ref().map(|s| s.config.clone()) else { return };
        self.form.loaded = true;
        let set = |state: &Entity<InputState>, value: String, window: &mut Window, cx: &mut Context<Self>| {
            state.update(cx, |s, cx| s.set_value(value, window, cx));
        };
        set(&self.form.name, config.name.clone(), window, cx);
        set(&self.form.download_dir, config.download_dir.clone(), window, cx);
        set(&self.form.shell, config.shell.clone(), window, cx);
        set(&self.form.terminal_pin, config.terminal_pin.clone(), window, cx);
        set(&self.form.port, config.port.to_string(), window, cx);
        set(&self.form.camera_size, config.camera_size.clone(), window, cx);
        if config.cast_max_size > 0 {
            set(&self.form.cast_max_size, config.cast_max_size.to_string(), window, cx);
        }
        self.form.clipboard_sync = config.clipboard_sync;
        self.form.ask_before_receiving = config.ask_before_receiving;
        self.form.terminal = config.terminal;
        self.form.cast_screen_off = config.cast_screen_off;
        self.form.cast_audio = config.cast_audio;
    }

    fn encode_qr(&mut self, cx: &mut Context<Self>) {
        let Pairing::Waiting { uri, .. } = &self.model.read(cx).pairing else { return };
        if self.qr.as_ref().is_none_or(|(cached, _)| cached != uri) {
            self.qr = Some((uri.clone(), Arc::new(Qr::new(uri))));
        }
    }

    fn save_theme(&mut self, cx: &mut Context<Self>) {
        let Some(mut config) = self.model.read(cx).status.as_ref().map(|s| s.config.clone()) else { return };
        config.theme = theme::choice_name(theme::choice()).into();
        self.model.update(cx, |m, cx| m.save_config_quietly(config, cx));
    }

    /// Switches apply as soon as they're flipped (only the text fields wait for Save).
    fn save_switches(&mut self, cx: &mut Context<Self>) {
        let Some(mut config) = self.model.read(cx).status.as_ref().map(|s| s.config.clone()) else { return };
        config.clipboard_sync = self.form.clipboard_sync;
        config.ask_before_receiving = self.form.ask_before_receiving;
        config.terminal = self.form.terminal;
        config.cast_screen_off = self.form.cast_screen_off;
        config.cast_audio = self.form.cast_audio;
        self.model.update(cx, |m, cx| m.save_config_quietly(config, cx));
    }

    fn save_form(&mut self, cx: &mut Context<Self>) {
        let Some(mut config) = self.model.read(cx).status.as_ref().map(|s| s.config.clone()) else { return };
        let text = |state: &Entity<InputState>, cx: &App| state.read(cx).value().trim().to_string();
        let port = text(&self.form.port, cx);
        match port.parse::<u16>() {
            Ok(p) if p >= 1024 => config.port = p,
            _ => {
                self.model.update(cx, |m, cx| m.notice("The port must be a number from 1024 to 65535", true, cx));
                return;
            }
        }
        config.name = text(&self.form.name, cx);
        config.download_dir = text(&self.form.download_dir, cx);
        if config.download_dir.is_empty() {
            config.download_dir = Config::default().download_dir;
        }
        config.shell = text(&self.form.shell, cx);
        config.terminal_pin = text(&self.form.terminal_pin, cx);
        config.camera_size = text(&self.form.camera_size, cx);
        let max_size = text(&self.form.cast_max_size, cx);
        match max_size.parse::<u32>() {
            _ if max_size.is_empty() => config.cast_max_size = 0,
            Ok(n) => config.cast_max_size = n,
            Err(_) => {
                self.model.update(cx, |m, cx| m.notice("The cast size is a number of pixels, like 1600", true, cx));
                return;
            }
        }
        config.clipboard_sync = self.form.clipboard_sync;
        config.ask_before_receiving = self.form.ask_before_receiving;
        config.terminal = self.form.terminal;
        config.cast_screen_off = self.form.cast_screen_off;
        config.cast_audio = self.form.cast_audio;
        self.model.update(cx, |m, cx| m.save_config(config, cx));
    }
}

impl Render for MainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.load_form(window, cx);
        self.encode_qr(cx);
        let p = palette(cx);
        let page = self.model.read(cx).page;
        let content = match page {
            Page::Phones => self.phones_page(&p, cx).into_any_element(),
            Page::Pcs => self.pcs_page(&p, cx).into_any_element(),
            Page::Pair => self.pair(&p, cx).into_any_element(),
            Page::Activity => self.activity(&p, cx).into_any_element(),
            Page::Setup => self.setup(&p, cx).into_any_element(),
            Page::Settings => self.settings(&p, cx).into_any_element(),
        };
        let body =
            div()
                .size_full()
                .flex()
                .bg(p.bg)
                .text_color(p.ink)
                .font_family(theme::SANS)
                .text_size(px(15.))
                .child(self.sidebar(&p, page, cx))
                .child(div().id("content").flex_1().min_w_0().h_full().overflow_y_scroll().child(
                    div()
                        .w_full()
                        .max_w(px(760.))
                        .mx_auto()
                        .px(px(32.))
                        .py(px(28.))
                        .flex()
                        .flex_col()
                        .gap(px(24.))
                        .children(self.incoming_banner(&p, cx))
                        .child(content),
                ));
        // GNOME (and other compositors without server-side decorations) leave the title bar to
        // the app: draw one with window controls (title_bar.rs). gpui-component's Root already
        // adds the resizable frame and its shadow. KDE and Hyprland decorate (or tile) the window.
        if matches!(window.window_decorations(), Decorations::Client { .. }) {
            div()
                .size_full()
                .flex()
                .flex_col()
                .bg(p.bg)
                // Empty: the sidebar right under it already says DOTLINK.
                .child(TitleBar::new(p))
                .child(div().flex_1().min_h(px(0.)).child(body))
                .into_any_element()
        } else {
            body.into_any_element()
        }
    }
}

/// What apps pick as the phone camera. macOS: OBS passes it on as "OBS Virtual Camera".
#[cfg(not(target_os = "macos"))]
const CAMERA_NAME: &str = "Dotlink Camera";
#[cfg(target_os = "macos")]
const CAMERA_NAME: &str = "OBS";

#[cfg(windows)]
const SETUP_INTRO: &str = "What each feature needs from this PC. Fix asks for administrator rights (UAC).";
#[cfg(target_os = "linux")]
const SETUP_INTRO: &str = "What each feature needs from this PC. Fix asks for your password once (polkit).";
#[cfg(target_os = "macos")]
const SETUP_INTRO: &str = "What each feature needs from this Mac. Fix opens System Settings where you allow it.";

// ---------- incoming files ----------

impl MainView {
    /// Files waiting for Accept or Decline ("Ask before receiving files"), above every page.
    fn incoming_banner(&self, p: &Palette, cx: &mut Context<Self>) -> Option<Div> {
        let waiting = self.model.read(cx).status.as_ref().map(|s| s.incoming.clone()).unwrap_or_default();
        if waiting.is_empty() {
            return None;
        }
        let mut list = group(p).border_1().border_color(p.accent);
        for (i, f) in waiting.into_iter().enumerate() {
            let (accept_id, decline_id) = (f.id.clone(), f.id.clone());
            let size = f.size.map(|s| format!(" · {}", human_size(s))).unwrap_or_default();
            list = list.child(
                row(p)
                    .when(i > 0, |r| r.border_t_1().border_color(p.line))
                    .child(direction(p, false))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().truncate().child(f.name.clone()))
                            .child(
                                div()
                                    .font_family(MONO)
                                    .text_size(px(13.))
                                    .text_color(p.ink_muted)
                                    .child(format!("{} wants to send it{size}", f.from)),
                            ),
                    )
                    .child(pill_icon(p, SharedString::from(format!("decline-{}", f.id)), IconName::X, "Decline", Kind::Ghost).on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            let id = decline_id.clone();
                            this.model.update(cx, |m, cx| m.answer_incoming(id, false, cx));
                        },
                    )))
                    .child(pill_icon(p, SharedString::from(format!("accept-{}", f.id)), IconName::Check, "Accept", Kind::Primary).on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            let id = accept_id.clone();
                            this.model.update(cx, |m, cx| m.answer_incoming(id, true, cx));
                        },
                    ))),
            );
        }
        Some(list)
    }
}

// ---------- sidebar ----------

impl MainView {
    fn sidebar(&self, p: &Palette, page: Page, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let connected = model.connected;
        let hostname = model.status.as_ref().map(|s| s.name.clone()).unwrap_or_default();
        let (phones, pcs) = model.online_counts();
        let nav = |id: &'static str, icon: IconName, label: &'static str, target: Page, cx: &mut Context<Self>| {
            let selected = page == target;
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(px(12.))
                .px(px(14.))
                .py(px(9.))
                .rounded(px(RADIUS))
                .cursor_pointer()
                .text_color(if selected { p.ink } else { p.ink_muted })
                .when(selected, |d| d.bg(p.nav_selected).font_weight(FontWeight::MEDIUM))
                .when(!selected, |d| d.hover(|s| s.bg(p.surface_raised).text_color(p.ink)))
                .child(Icon::new(icon).size(px(18.)))
                .child(label)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.go(target, cx)))
        };
        div()
            .w(px(236.))
            .h_full()
            .flex()
            .flex_col()
            .justify_between()
            .p(px(16.))
            .bg(p.surface)
            .border_r_1()
            .border_color(p.line)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .px(px(14.))
                            .pt(px(8.))
                            .font_family(DISPLAY)
                            .font_weight(FontWeight::BLACK)
                            .text_size(px(32.))
                            .child("DOTLINK"),
                    )
                    .child(mono_label(p, hostname.to_uppercase()).px(px(14.)).pb(px(16.)))
                    .child(nav("nav-phones", IconName::Smartphone, "Phones", Page::Phones, cx))
                    .child(nav("nav-pcs", IconName::Monitor, "PCs", Page::Pcs, cx))
                    .child(nav("nav-pair", IconName::ScanLine, "Pair", Page::Pair, cx))
                    .child(nav("nav-activity", IconName::ArrowDownUp, "Activity", Page::Activity, cx))
                    .child(nav("nav-setup", IconName::ListChecks, "Setup", Page::Setup, cx))
                    .child(nav("nav-settings", IconName::Settings2, "Settings", Page::Settings, cx)),
            )
            .child(
                div()
                    .px(px(14.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(status_dot(if connected { p.positive } else { p.accent }))
                    .child(mono_label(
                        p,
                        match (connected, phones, pcs) {
                            (false, _, _) => "SERVICE STOPPED".to_string(),
                            (true, 0, 0) => "NOTHING ONLINE".to_string(),
                            (true, phones, pcs) => {
                                let count = |n: usize, one: &str, many: &str| match n {
                                    0 => None,
                                    1 => Some(format!("1 {one}")),
                                    n => Some(format!("{n} {many}")),
                                };
                                let parts: Vec<String> =
                                    [count(phones, "PHONE", "PHONES"), count(pcs, "PC", "PCS")].into_iter().flatten().collect();
                                format!("{} ONLINE", parts.join(" · "))
                            }
                        },
                    )),
            )
    }
}

// ---------- pages ----------

impl MainView {
    /// Phones: the paired phones, sending to them, Cast and the webcam/mic (all phone-only).
    fn phones_page(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        // A copy, so `cx` is free for the listeners below.
        let Some(status) = self.model.read(cx).status.clone() else {
            return page(p, "Phones", None).child(self.not_connected(p, cx)).into_any_element();
        };
        let status = &status;
        let phone_online = status.devices.iter().any(|d| d.online && d.is_phone());
        let any_phone = status.devices.iter().any(|d| d.is_phone());
        let streams = self.streams(p, status.camera.clone(), status.mic.clone(), phone_online, cx);
        let cast = self.cast_card(p, status, cx);
        let recent = self.recent(p, status, true, cx);
        let mut phones = group(p);
        if !any_phone {
            phones = phones.child(
                row(p).child(div().flex_1().min_w_0().text_color(p.ink_muted).child("No phone paired yet.")).child(
                    pill_icon(p, "pair-empty", IconName::ScanLine, "Pair a phone", Kind::Primary).on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.pair_tab = 0;
                        this.go(Page::Pair, cx)
                    })),
                ),
            );
        }
        for (i, d) in status.devices.iter().filter(|d| d.is_phone()).enumerate() {
            let id = d.id.clone();
            let subtitle = match (d.online, d.usb) {
                (true, true) => "Online · USB".to_string(),
                (true, false) => "Online · Wi-Fi".to_string(),
                _ => format!("Paired {}", ago(d.paired_at)),
            };
            phones = phones.child(
                row(p)
                    .when(i > 0, |r| r.border_t_1().border_color(p.line))
                    .child(status_dot(if d.online { p.positive } else { p.ink_faint }))
                    .child(
                        div()
                            .flex_1().min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().font_weight(FontWeight::MEDIUM).child(d.name.clone()))
                            .child(div().text_size(px(13.)).text_color(p.ink_muted).child(subtitle)),
                    )
                    .children(d.battery.map(|b| battery(p, b)))
                    .child(pill_icon(p, SharedString::from(format!("unpair-{id}")), IconName::Trash, "Forget", Kind::Ghost).on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| {
                            let id = id.clone();
                            this.model.update(cx, |m, cx| m.unpair(id, cx));
                        }),
                    )),
            );
        }
        let addresses = status.addresses.iter().map(|a| format!("{a}:{}", status.port)).collect::<Vec<_>>().join("  ");

        page(p, "Phones", None)
            .child(section(p, "Paired", phones))
            .when(any_phone, |page| page.child(section(p, "Send to phone", self.send_zone(p, status, true, cx))))
            .when(any_phone, |page| page.child(section(p, "Cast screen", cast)))
            .when_some(streams.filter(|_| any_phone), |page, streams| page.child(section(p, "Webcam & mic", streams)))
            .when_some(recent, |page, recent| page.child(recent))
            .child(section(
                p,
                "This PC",
                group(p)
                    .child(kv(p, "Name", status.name.clone()))
                    .child(kv(p, "Address", addresses).border_t_1().border_color(p.line))
                    .child(
                        kv(p, "Fingerprint", format!("{}…", &status.fingerprint[..32.min(status.fingerprint.len())]))
                            .border_t_1()
                            .border_color(p.line),
                    ),
            ))
            .into_any_element()
    }

    /// PCs: other PCs linked with this one (files and the clipboard), and sending to them.
    fn pcs_page(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(status) = self.model.read(cx).status.clone() else {
            return page(p, "PCs", None).child(self.not_connected(p, cx)).into_any_element();
        };
        let status = &status;
        let pcs = self.pcs(p, status, cx).unwrap_or_else(|| {
            group(p).child(
                row(p)
                    .child(div().flex_1().min_w_0().text_color(p.ink_muted).child("No PC linked yet."))
                    .child(pill_icon(p, "join-empty", IconName::Link, "Join a PC", Kind::Primary).on_click(cx.listener(
                        |this, _: &ClickEvent, _, cx| {
                            this.pair_tab = 1;
                            this.model.update(cx, |m, cx| m.refresh_nearby(cx));
                            this.go(Page::Pair, cx)
                        },
                    ))),
            )
        });
        let any_pc = status.devices.iter().any(|d| !d.is_phone());
        let recent = self.recent(p, status, false, cx);
        page(p, "PCs", Some("Other PCs linked with this one send each other files and the clipboard."))
            .child(section(p, "Linked", pcs))
            .when(any_pc, |page| page.child(section(p, "Send to PC", self.send_zone(p, status, false, cx))))
            .when_some(recent, |page, recent| page.child(recent))
            .into_any_element()
    }

    /// The latest transfers with phones (`phones`) or with PCs, under the drop zone so sending
    /// shows its progress there. None when there are none.
    fn recent(&self, p: &Palette, status: &Status, phones: bool, cx: &mut Context<Self>) -> Option<Div> {
        let names: Vec<&str> = status.devices.iter().filter(|d| d.is_phone() == phones).map(|d| d.name.as_str()).collect();
        let recent: Vec<&Transfer> = status.transfers.iter().filter(|t| names.contains(&t.device.as_str())).take(3).collect();
        if recent.is_empty() {
            return None;
        }
        let mut list = group(p);
        for (i, t) in recent.into_iter().enumerate() {
            list = list.child(transfer_row(p, t).when(i > 0, |r| r.border_t_1().border_color(p.line)));
        }
        Some(section_with(
            p,
            "Recent",
            Some(
                pill(p, if phones { "see-activity-phones" } else { "see-activity-pcs" }, "See all", Kind::Ghost)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.go(Page::Activity, cx))),
            ),
            list,
        ))
    }

    /// Drop files here, Send files…, Send clipboard: to the phones (`phones`) or the PCs, with a
    /// picker when several are connected.
    fn send_zone(&self, p: &Palette, status: &Status, phones: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let online: Vec<(Option<String>, String)> = status
            .devices
            .iter()
            .filter(|d| d.online && d.is_phone() == phones)
            .map(|d| (Some(d.id.clone()), d.name.clone()))
            .collect();
        let any_online = !online.is_empty();
        let several = online.len() > 1;
        let target = several.then(|| {
            let all = if phones { "All phones" } else { "All PCs" };
            let options: Vec<_> = std::iter::once((None, all.to_string())).chain(online).collect();
            let model = self.model.clone();
            div().flex().items_center().gap(px(10.)).child(mono_label(p, "TO")).child(phone_picker(
                if phones { "send-to-phone" } else { "send-to-pc" },
                options,
                self.model.read(cx).send_target(phones),
                move |id, cx| {
                    model.update(cx, |m, cx| {
                        m.send_to = id;
                        cx.notify();
                    })
                },
            ))
        });
        let accent = p.accent;
        let hint = match (phones, several, any_online) {
            (_, true, _) => "Drop files here to send them where chosen",
            (true, _, true) => "Drop files here to send them to your phone",
            (false, _, true) => "Drop files here to send them to the linked PC",
            (true, _, false) => "Open Dotlink on your phone to send it files",
            (false, _, false) => "The linked PC isn't connected",
        };
        div()
            .id(if phones { "drop-phones" } else { "drop-pcs" })
            .flex()
            .flex_col()
            .items_center()
            .gap(px(14.))
            .py(px(28.))
            .rounded(px(CARD_RADIUS))
            .border_1()
            .border_dashed()
            .border_color(p.line_strong)
            .drag_over::<ExternalPaths>(move |s, _, _, _| s.border_color(accent).bg(accent.opacity(0.06)))
            .on_drop(cx.listener(move |this, paths: &ExternalPaths, _, cx| {
                let paths = paths.paths().to_vec();
                this.model.update(cx, |m, cx| m.send_files_to(paths, phones, cx));
            }))
            .children(target)
            .child(Icon::new(IconName::Upload).size(px(28.)).text_color(p.ink_muted))
            .child(div().text_color(p.ink_muted).child(hint))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        enabled(
                            pill_icon(p, if phones { "send-files-phones" } else { "send-files-pcs" }, IconName::Files, "Send files…", Kind::Primary),
                            any_online,
                        )
                            .when(any_online, |b| {
                                b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.pick_and_send(phones, cx)))
                            }),
                    )
                    .child(
                        enabled(
                            pill_icon(
                                p,
                                if phones { "send-clip-phones" } else { "send-clip-pcs" },
                                IconName::ClipboardCopy,
                                "Send clipboard",
                                Kind::Secondary,
                            ),
                            any_online,
                        )
                            .when(any_online, |b| {
                                b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.model.update(cx, |m, cx| m.send_clipboard_to(phones, cx))
                                }))
                            }),
                    ),
            )
    }

    /// Each phone's screen in its own scrcpy window: one row per connected phone (and any phone
    /// still casting), with its state, Cast/Stop, and pairing when it's needed.
    fn cast_card(&self, p: &Palette, status: &Status, cx: &mut Context<Self>) -> Div {
        let phones: Vec<&DeviceInfo> = status
            .devices
            .iter()
            .filter(|d| d.is_phone() && (d.online || status.casts.contains_key(&d.id)))
            .collect();
        let mut card = group(p);
        if phones.is_empty() {
            return card.child(
                row(p).child(status_dot(p.ink_faint)).child(
                    div().flex_1().min_w(px(0.)).text_color(p.ink_muted).child(
                        "Shows a connected phone's screen in a window, with mouse and keyboard. Over USB or Wi-Fi.",
                    ),
                ),
            );
        }
        // One code field: pairing is done for one phone at a time.
        let mut code_shown = false;
        for (i, d) in phones.into_iter().enumerate() {
            let cast = status.cast(&d.id);
            let (dot, detail): (Hsla, String) = match &cast {
                CastState::Idle => (p.ink_faint, "Shows its screen in a window, with mouse and keyboard.".into()),
                CastState::Installing { .. } => (p.ink, "Installing scrcpy (18 MB, once)…".into()),
                CastState::Connecting { .. } => (p.ink, "Connecting…".into()),
                CastState::Casting { usb, .. } => (
                    p.positive,
                    format!("Casting over {}. Close the window or press Stop to end.", if *usb { "USB" } else { "Wi-Fi" }),
                ),
                CastState::NeedsWifiDebugging { .. } => (
                    p.accent,
                    "Wireless debugging is off. The phone shows a notification that opens the setting; or plug it in over USB."
                        .into(),
                ),
                CastState::NeedsPairing { .. } => (
                    p.accent,
                    "Pair once: Developer options → Wireless debugging → Pair device with pairing code. Type the code here."
                        .into(),
                ),
                CastState::Failed { error, .. } => (p.accent, format!("Couldn't cast: {error}")),
            };
            let busy = matches!(cast, CastState::Connecting { .. } | CastState::Installing { .. });
            let pill_id = SharedString::from(format!("cast-{}", d.id));
            let (id, online) = (d.id.clone(), d.online);
            let button = match cast {
                // Connecting can take a while (mDNS, adb connect); Cancel ends it like Stop.
                CastState::Casting { .. } | CastState::Connecting { .. } | CastState::Installing { .. } => {
                    pill_icon(p, pill_id, IconName::Square, if busy { "Cancel" } else { "Stop" }, Kind::Ghost).on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            let id = id.clone();
                            this.model.update(cx, |m, cx| m.set_cast(false, Some(id), cx))
                        },
                    ))
                }
                ref other => {
                    let (icon, label, kind) =
                        if matches!(other, CastState::Idle) {
                            (IconName::Cast, "Cast", Kind::Primary)
                        } else {
                            (IconName::RotateCcw, "Try again", Kind::Secondary)
                        };
                    enabled(pill_icon(p, pill_id, icon, label, kind), online).when(online, |b| {
                        b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            let id = id.clone();
                            this.model.update(cx, |m, cx| m.set_cast(true, Some(id), cx))
                        }))
                    })
                }
            };
            card = card.child(
                row(p)
                    .when(i > 0, |r| r.border_t_1().border_color(p.line))
                    .child(status_dot(dot))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(div().font_weight(FontWeight::MEDIUM).child(d.name.clone()))
                            .child(div().text_size(px(13.)).text_color(p.ink_muted).child(detail)),
                    )
                    .child(button),
            );
            if matches!(cast, CastState::NeedsPairing { .. }) && !code_shown {
                code_shown = true;
                let id = d.id.clone();
                card = card.child(
                    row(p)
                        .border_t_1()
                        .border_color(p.line)
                        .child(div().w(px(200.)).child(Styled::h(Input::new(&self.cast_code), px(CONTROL))))
                        .child(pill_icon(p, "cast-pair", IconName::Link, "Pair", Kind::Primary).on_click(cx.listener(
                            move |this, _: &ClickEvent, _, cx| {
                                let code = this.cast_code.read(cx).value().to_string();
                                let id = id.clone();
                                this.model.update(cx, |m, cx| m.pair_cast(id, code, cx));
                            },
                        ))),
                );
            }
        }
        card
    }

    /// Phone camera and mic: what's streaming, and Start/Stop. None where the platform has neither.
    fn streams(
        &self,
        p: &Palette,
        camera: Option<String>,
        mic: Option<String>,
        any_online: bool,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let features = platform::features();
        let model = self.model.read(cx);
        let rows: Vec<(&'static str, &'static str, &'static str, Option<String>)> = [
            features.camera.then_some(("camera", "Webcam", CAMERA_NAME, camera)),
            features.mic.then_some(("mic", "Microphone", "Dotlink Mic", mic)),
        ]
        .into_iter()
        .flatten()
        .collect();
        if rows.is_empty() {
            return None;
        }
        let mut list = group(p);
        // Several phones online: choose the one the webcam and mic start on.
        let online: Vec<(Option<String>, String)> = model
            .status
            .iter()
            .flat_map(|s| &s.devices)
            .filter(|d| d.online && d.is_phone())
            .map(|d| (Some(d.id.clone()), d.name.clone()))
            .collect();
        let first = usize::from(online.len() > 1);
        if online.len() > 1 {
            let entity = self.model.clone();
            list = list.child(row(p).child(div().flex_1().min_w_0().child("Phone")).child(phone_picker(
                "stream-from",
                online,
                model.stream_source(),
                move |id, cx| {
                    entity.update(cx, |m, cx| {
                        m.stream_from = id;
                        cx.notify();
                    })
                },
            )));
        }
        for (i, (kind, title, device_name, from)) in rows.into_iter().enumerate() {
            let starting = from.is_none() && model.is_starting(kind);
            let (subtitle, dot) = match (&from, starting) {
                (Some(phone), _) => (format!("On · {phone} → {device_name}"), p.positive),
                (None, true) => ("Starting… check your phone".to_string(), p.ink),
                (None, false) => ("Off".to_string(), p.ink_faint),
            };
            let on = from.is_some();
            let button =
                if on {
                    pill_icon(p, SharedString::from(format!("stop-{kind}")), IconName::Square, "Stop", Kind::Ghost).on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| this.model.update(cx, |m, cx| m.set_stream(kind, false, cx)),
                    ))
                } else if starting {
                    enabled(pill(p, SharedString::from(format!("start-{kind}")), "Starting…", Kind::Primary), false)
                } else {
                    enabled(pill_icon(p, SharedString::from(format!("start-{kind}")), IconName::Play, "Start", Kind::Primary), any_online)
                        .when(any_online, |b| {
                            b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.model.update(cx, |m, cx| m.set_stream(kind, true, cx))
                            }))
                        })
                };
            // macOS: the stream for OBS, whose virtual camera apps use (platform/macos/camera.rs).
            #[cfg(target_os = "macos")]
            let copy_link = (kind == "camera").then(|| {
                let port = model.status.as_ref().map_or(dotlink_core::config::DEFAULT_PORT, |s| s.port);
                pill_icon(p, "copy-obs-link", IconName::Copy, "Copy OBS link", Kind::Ghost).on_click(cx.listener(
                    move |this, _: &ClickEvent, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(platform::camera_url(port)));
                        this.model.update(cx, |m, cx| {
                            m.notice("Copied: paste it as the URL of a Browser source in OBS", false, cx)
                        });
                    },
                ))
            });
            #[cfg(not(target_os = "macos"))]
            let copy_link: Option<Div> = None;
            list = list.child(
                row(p)
                    .when(i + first > 0, |r| r.border_t_1().border_color(p.line))
                    .child(status_dot(dot))
                    .child(
                        div()
                            .flex_1().min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().font_weight(FontWeight::MEDIUM).child(title))
                            .child(div().font_family(MONO).text_size(px(13.)).text_color(p.ink_muted).child(subtitle)),
                    )
                    .children(copy_link)
                    .child(button),
            );
        }
        Some(list)
    }

    fn pair(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let body: AnyElement = match &model.pairing {
            Pairing::Idle => div().text_color(p.ink_muted).child("Making a pairing code…").into_any_element(),
            Pairing::Waiting { uri, code, since } => {
                let left = 300u64.saturating_sub(since.elapsed().as_secs());
                let uri = uri.clone();
                let code = dotlink_core::pair_code::spaced(code);
                // Too narrow for both side by side: the steps go under the code.
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(28.))
                    .items_start()
                    .child(qr(self.qr.as_ref().map(|(_, q)| q.clone()), 260.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(240.))
                            .flex()
                            .flex_col()
                            .gap(px(14.))
                            .child(step(p, "1", "Open Dotlink on your phone."))
                            .child(step(p, "2", "Tap Pair and scan this code."))
                            .child(step(p, "3", "Keep both on the same Wi-Fi, or use a USB cable."))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .pt(px(8.))
                                    .child(mono_label(p, "ANOTHER PC: PAIR → JOIN A PC, THEN TYPE"))
                                    .child(
                                        div()
                                            .font_family(MONO)
                                            .text_size(px(28.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(code),
                                    ),
                            )
                            .child(mono_label(p, format!("EXPIRES IN {}:{:02}", left / 60, left % 60)).pt(px(8.)))
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap(px(8.))
                                    // Plugged in with USB debugging: the PC opens the link on the
                                    // phone, no scanning.
                                    .child(pill_icon(p, "pair-usb", IconName::Usb, "Pair over USB", Kind::Secondary).on_click(cx.listener(
                                        |this, _: &ClickEvent, _, cx| this.model.update(cx, |m, cx| m.pair_usb(cx)),
                                    )))
                                    .child(pill_icon(p, "copy-link", IconName::Copy, "Copy link", Kind::Secondary).on_click(cx.listener(
                                        move |this, _: &ClickEvent, _, cx| {
                                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(uri.clone()));
                                            this.model.update(cx, |m, cx| {
                                                m.notice("Link copied. Paste it in the phone app.", false, cx)
                                            });
                                        },
                                    )))
                                    .child(pill_icon(p, "new-code", IconName::RefreshCw, "New code", Kind::Ghost).on_click(cx.listener(
                                        |this, _: &ClickEvent, _, cx| {
                                            this.model.update(cx, |m, cx| m.start_pairing(cx))
                                        },
                                    ))),
                            ),
                    )
                    .into_any_element()
            }
            Pairing::Paired(name) => div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(div().font_family(DISPLAY).font_weight(FontWeight::BLACK).text_size(px(40.)).child("PAIRED"))
                .child(div().text_color(p.ink_muted).child(format!("{name} can now connect to this PC.")))
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(
                            pill(p, "paired-done", "Done", Kind::Primary)
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.go(Page::Phones, cx))),
                        )
                        .child(pill_icon(p, "pair-another", IconName::ScanLine, "Pair another", Kind::Ghost).on_click(cx.listener(
                            |this, _: &ClickEvent, _, cx| this.model.update(cx, |m, cx| m.start_pairing(cx)),
                        ))),
                )
                .into_any_element(),
            Pairing::Failed(e) => div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(div().text_color(p.accent_text).child(e.clone()))
                .child(pill_icon(p, "pair-retry", IconName::RotateCcw, "Try again", Kind::Primary).on_click(
                    cx.listener(|this, _: &ClickEvent, _, cx| this.model.update(cx, |m, cx| m.start_pairing(cx))),
                ))
                .into_any_element(),
        };
        let tab = self.pair_tab;
        let view = cx.entity().downgrade();
        let tabs = segmented(p, "pair-tab", &["THIS PC'S CODE", "JOIN A PC"], tab, move |i, cx| {
            view.update(cx, |this, cx| {
                this.pair_tab = i;
                if i == 1 {
                    this.model.update(cx, |m, cx| m.refresh_nearby(cx));
                }
                cx.notify();
            })
            .ok();
        });
        let (subtitle, body) = if tab == 0 {
            ("A phone scans the QR code; another PC types the 6-digit code. Both work once, for 5 minutes.", body)
        } else {
            ("Link this PC to another one to send files and the clipboard both ways.", self.join_form(p, cx))
        };
        page(p, "Pair", Some(subtitle)).child(div().flex().child(tabs)).child(body)
    }

    /// The Pair page's "Join a PC" tab: pick a PC on the network and type its code, or paste
    /// its link.
    fn join_form(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let model = self.model.read(cx);
        let loading = model.nearby_loading;
        let pcs = model.nearby.clone();
        let pick = self.join_pick.clone().filter(|id| pcs.iter().any(|pc| pc.id == *id && !pc.linked));
        let mut list = group(p);
        if pcs.is_empty() {
            list = list.child(row(p).child(div().flex_1().min_w_0().text_color(p.ink_muted).child(if loading {
                "Looking for PCs…"
            } else {
                "No other Dotlink PC found. Open Dotlink on it, on the same network."
            })));
        }
        for (i, pc) in pcs.iter().enumerate() {
            let (id, chosen) = (pc.id.clone(), pick.as_deref() == Some(pc.id.as_str()));
            let mut r = row(p)
                .id(SharedString::from(format!("nearby-{}", pc.id)))
                .when(i > 0, |r| r.border_t_1().border_color(p.line))
                .child(status_dot(if chosen { p.accent } else if pc.linked { p.positive } else { p.ink_faint }))
                .child(div().flex_1().min_w_0().font_weight(FontWeight::MEDIUM).child(pc.name.clone()))
                .child(div().text_size(px(13.)).text_color(p.ink_muted).child(if pc.linked {
                    "Linked"
                } else if chosen {
                    "Chosen"
                } else {
                    ""
                }));
            if !pc.linked {
                r = r.cursor_pointer().on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.join_pick = Some(id.clone());
                    cx.notify();
                }));
            }
            list = list.child(r);
        }
        let can_join = pick.is_some();
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .max_w(px(560.))
            .child(step(p, "1", "On the other PC, open Dotlink → Pair: it shows a 6-digit code."))
            .child(step(p, "2", "Pick that PC here and type its code."))
            .child(section_with(
                p,
                "PCs on this network",
                Some(enabled(pill_icon(p, "nearby-refresh", IconName::RefreshCw, if loading { "Looking…" } else { "Refresh" }, Kind::Ghost), !loading).when(
                    !loading,
                    |b| b.on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.model.update(cx, |m, cx| m.refresh_nearby(cx)))),
                )),
                list,
            ))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(div().w(px(200.)).child(Styled::h(Input::new(&self.join_code), px(CONTROL))))
                    .child(enabled(pill_icon(p, "join-code", IconName::Link, "Join", Kind::Primary), can_join).when(can_join, |b| {
                        b.on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            let (Some(id), code) = (this.join_pick.clone(), this.join_code.read(cx).value().to_string()) else {
                                return;
                            };
                            let input = this.join_code.clone();
                            let window = window.window_handle();
                            this.model.update(cx, |m, cx| {
                                m.join_code(
                                    id,
                                    code,
                                    move |cx| {
                                        let _ = window.update(cx, |_, window, cx| {
                                            input.update(cx, |state, cx| state.set_value("", window, cx));
                                        });
                                    },
                                    cx,
                                )
                            });
                        }))
                    })),
            )
            .child(div().text_size(px(13.)).text_color(p.ink_muted).child(
                "The code only works with the PC showing it: someone else on the network can't stand in for it.",
            ))
            .child(mono_label(p, "OR PASTE ITS LINK (PAIR → COPY LINK)").pt(px(10.)))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(div().flex_1().min_w_0().child(Styled::h(Input::new(&self.join_link), px(CONTROL))))
                    .child(pill(p, "join", "Join", Kind::Primary).on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        let link = this.join_link.read(cx).value().to_string();
                        if link.trim().is_empty() {
                            return;
                        }
                        let input = this.join_link.clone();
                        let window = window.window_handle();
                        this.model.update(cx, |m, cx| {
                            m.join(
                                link,
                                move |cx| {
                                    let _ = window.update(cx, |_, window, cx| {
                                        input.update(cx, |state, cx| state.set_value("", window, cx));
                                    });
                                },
                                cx,
                            )
                        });
                    }))),
            )
            .into_any_element()
    }

    /// PCs linked with this one: files and the clipboard, nothing phone-only. None when there
    /// are none.
    fn pcs(&self, p: &Palette, status: &Status, cx: &mut Context<Self>) -> Option<Div> {
        let pcs: Vec<&DeviceInfo> = status.devices.iter().filter(|d| !d.is_phone()).collect();
        if pcs.is_empty() {
            return None;
        }
        let mut list = group(p);
        for (i, d) in pcs.into_iter().enumerate() {
            let (id, auto) = (d.id.clone(), d.clipboard_auto);
            let subtitle = if d.online { "Online".to_string() } else { format!("Linked {}", ago(d.paired_at)) };
            let switch_id = SharedString::from(format!("clip-auto-{id}"));
            let toggle_id = id.clone();
            list = list.child(
                row(p)
                    .when(i > 0, |r| r.border_t_1().border_color(p.line))
                    .child(status_dot(if d.online { p.positive } else { p.ink_faint }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().font_weight(FontWeight::MEDIUM).child(d.name.clone()))
                            .child(div().text_size(px(13.)).text_color(p.ink_muted).child(subtitle)),
                    )
                    .children(d.battery.map(|b| battery(p, b)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(13.))
                            .text_color(p.ink_muted)
                            .child("Clipboard sync")
                            .child(Switch::new(switch_id).checked(auto).on_click(cx.listener(
                                move |this, checked: &bool, _, cx| {
                                    let (id, on) = (toggle_id.clone(), *checked);
                                    this.model.update(cx, |m, cx| m.set_clipboard_auto(id, on, cx));
                                },
                            ))),
                    )
                    .child(pill_icon(p, SharedString::from(format!("unpair-{id}")), IconName::Trash, "Forget", Kind::Ghost).on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| {
                            let id = id.clone();
                            this.model.update(cx, |m, cx| m.unpair(id, cx));
                        }),
                    )),
            );
        }
        Some(list)
    }

    fn activity(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let transfers = model.status.as_ref().map(|s| s.transfers.clone()).unwrap_or_default();
        let mut files = group(p);
        if transfers.is_empty() {
            files = files.child(row(p).text_color(p.ink_muted).child("Nothing sent or received yet."));
        }
        // "PC" after a linked PC's name: the names alone don't tell a phone from a PC.
        let pc_names: Vec<String> = model
            .status
            .iter()
            .flat_map(|s| &s.devices)
            .filter(|d| !d.is_phone())
            .map(|d| d.name.clone())
            .collect();
        for (i, t) in transfers.iter().enumerate() {
            let mut t = t.clone();
            if pc_names.contains(&t.device) {
                t.device = format!("{} (PC)", t.device);
            }
            files = files.child(transfer_row(p, &t).when(i > 0, |r| r.border_t_1().border_color(p.line)));
        }
        let mut clips = group(p);
        if model.clips.is_empty() {
            clips = clips.child(row(p).text_color(p.ink_muted).child("No clipboard sync since Dotlink started."));
        }
        for (i, c) in model.clips.iter().enumerate() {
            let device = if pc_names.contains(&c.device) { format!("{} (PC)", c.device) } else { c.device.clone() };
            let what = if c.to_phone { format!("To {device}") } else { format!("From {device}") };
            clips = clips.child(
                row(p)
                    .when(i > 0, |r| r.border_t_1().border_color(p.line))
                    .child(direction(p, c.to_phone))
                    .child(div().flex_1().min_w_0().child(what))
                    .child(mono_label(p, format!("{} CHARS · {}", c.chars, ago_instant(c.at)))),
            );
        }
        let download_dir = model.status.as_ref().map(|s| s.config.download_dir());
        // Only what has finished can go: waiting and running transfers stay in the list.
        let clearable = transfers.iter().any(|t| matches!(t.state, TransferState::Done | TransferState::Failed(_)));
        let actions = div()
            .flex()
            .gap(px(8.))
            .when(clearable, |d| {
                d.child(pill_icon(p, "clear-transfers", IconName::Trash, "Clear", Kind::Ghost).on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| this.model.update(cx, |m, cx| m.clear_transfers(cx)),
                )))
            })
            .children(download_dir.map(|dir| {
                pill_icon(p, "open-downloads", IconName::FolderOpen, "Open folder", Kind::Ghost).on_click(move |_, _, _| {
                    let _ = std::fs::create_dir_all(&dir);
                    platform::open_path(&dir)
                })
            }));
        page(p, "Activity", None)
            .child(section_with(p, "Files", Some(actions), files))
            .child(section(p, "Clipboard", clips))
    }

    fn setup(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let fixing = model.fixing.clone();
        // Checks grouped by feature, in the order they come: (feature, card, rows so far).
        let mut features: Vec<(String, Div, usize)> = Vec::new();
        for c in &model.checks {
            let at = match features.iter().position(|(f, _, _)| *f == c.feature) {
                Some(i) => i,
                None => {
                    features.push((c.feature.clone(), group(p), 0));
                    features.len() - 1
                }
            };
            let first = features[at].2 == 0;
            features[at].2 += 1;
            let id = c.id.clone();
            let busy = fixing.as_deref() == Some(c.id.as_str())
                || (fixing.as_deref() == Some(super::model::FIX_ALL) && !c.ok && c.fixable);
            let check_row = row(p)
                .when(!first, |r| r.border_t_1().border_color(p.line))
                // Top-aligned only with a detail line; a lone title stays centred in the row.
                .when(!c.detail.is_empty(), |r| r.items_start())
                // The dot sits in a box as tall as the title's line, so it centres on the title
                // even when a detail line follows.
                .child(div().h(px(TITLE_LINE)).flex().items_center().child(status_dot(if c.ok {
                    p.positive
                } else {
                    p.accent
                })))
                .child(
                    div()
                        .flex_1()
                        // Narrower than its text, so a long detail wraps instead of widening the card.
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(div().line_height(px(TITLE_LINE)).child(c.title.clone()))
                        .when(!c.detail.is_empty(), |d| {
                            d.child(
                                div()
                                    .font_family(MONO)
                                    .text_size(px(13.))
                                    .text_color(p.ink_muted)
                                    .child(c.detail.clone()),
                            )
                        }),
                )
                .when(c.fixable, |r| {
                    r.child(if busy {
                        enabled(pill(p, SharedString::from(format!("fix-{id}")), "Fixing…", Kind::Primary), false)
                    } else {
                        pill_icon(p, SharedString::from(format!("fix-{id}")), IconName::Wrench, "Fix", Kind::Primary).on_click(cx.listener(
                            move |this, _: &ClickEvent, _, cx| {
                                let id = id.clone();
                                this.model.update(cx, |m, cx| m.fix(id, cx));
                            },
                        ))
                    })
                });
            features[at].1 = std::mem::replace(&mut features[at].1, div()).child(check_row);
        }
        let fixable = model.checks.iter().filter(|c| !c.ok && c.fixable).count();
        let mut body =
            page(p, "Setup", Some(SETUP_INTRO))
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .when(fixable > 0, |d| {
                            d.child(if fixing.is_some() {
                                enabled(pill(p, "fix-all", "Fixing…", Kind::Primary), false)
                            } else {
                                pill_icon(p, "fix-all", IconName::Wrench, if fixable == 1 { "Fix it" } else { "Fix all" }, Kind::Primary)
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.model.update(cx, |m, cx| m.fix_all(cx))
                                    }))
                            })
                        })
                        .child(
                            pill_icon(
                                p,
                                "recheck",
                                IconName::RefreshCw,
                                if model.checks_loading { "Checking…" } else { "Check again" },
                                Kind::Secondary,
                            )
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.model.update(cx, |m, cx| m.refresh_checks(cx))
                            })),
                        ),
                );
        if model.checks.is_empty() && !model.checks_loading {
            body = body.child(div().text_color(p.ink_muted).child("Nothing to set up on this system."));
        }
        for (feature, g, _) in features {
            body = body.child(section(p, &feature, g));
        }
        body
    }

    fn settings(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let autostart = model.autostart;
        let wm_hint = platform::autostart::wm_hint();
        let restart = model.restart_needed;
        let owns = model.owns_service;
        let version = model.status.as_ref().map(|s| s.version.clone()).unwrap_or_default();
        let form = &self.form;
        let view = cx.entity().downgrade();
        let switch_row = |id: &'static str,
                          title: &'static str,
                          detail: &'static str,
                          on: bool,
                          first: bool,
                          toggle: fn(&mut MainView, bool, &mut Context<MainView>)| {
            let view = view.clone();
            row(p)
                .when(!first, |r| r.border_t_1().border_color(p.line))
                .child(
                    div()
                        .flex_1().min_w_0()
                        .flex()
                        .flex_col()
                        .child(title)
                        .child(div().text_size(px(13.)).text_color(p.ink_muted).child(detail)),
                )
                .child(Switch::new(id).checked(on).on_click(move |checked: &bool, _, cx| {
                    let checked = *checked;
                    view.update(cx, |this, cx| {
                        toggle(this, checked, cx);
                        cx.notify();
                    })
                    .ok();
                }))
        };
        let field = |title: &'static str, detail: &'static str, state: &Entity<InputState>, first: bool| {
            row(p)
                .when(!first, |r| r.border_t_1().border_color(p.line))
                .child(
                    div()
                        .w(px(236.))
                        .flex()
                        .flex_col()
                        .child(title)
                        .child(div().text_size(px(13.)).text_color(p.ink_muted).child(detail)),
                )
                .child(div().flex_1().min_w_0().child(Styled::h(Input::new(state), px(CONTROL))))
        };

        let view = cx.entity().downgrade();
        let theme_choice = theme::choice();
        page(p, "Settings", None)
            .child(section(
                p,
                "Theme",
                group(p).child(row(p).child(segmented(
                    p,
                    "theme",
                    &["SYSTEM", "LIGHT", "DARK"],
                    theme_choice as usize,
                    move |i, cx| {
                        theme::set_choice(i as u8, cx);
                        // Saved like the switches: right away, no Save needed.
                        view.update(cx, |this, cx| this.save_theme(cx)).ok();
                    },
                ))),
            ))
            .child(section(
                p,
                "General",
                group(p)
                    .child(switch_row(
                        "autostart",
                        "Start at login",
                        "Runs in the tray, without opening the window.",
                        autostart,
                        true,
                        |this, on, cx| this.model.update(cx, |m, cx| m.set_autostart(on, cx)),
                    ))
                    .when_some(wm_hint.filter(|_| autostart), |g, hint| {
                        g.child(
                            row(p).border_t_1().border_color(p.line).child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .font_family(MONO)
                                    .text_size(px(13.))
                                    .text_color(p.ink_muted)
                                    .child(hint),
                            ),
                        )
                    })
                    .child(switch_row(
                        "clipboard",
                        "Clipboard sync",
                        "Text you copy here goes to the phone right away.",
                        form.clipboard_sync,
                        false,
                        |this, on, cx| {
                            this.form.clipboard_sync = on;
                            this.save_switches(cx);
                        },
                    )),
            ))
            .child(section(
                p,
                "This PC",
                group(p).child(field("Name", "What your phones call this PC.", &form.name, true)),
            ))
            .child(section(
                p,
                "Files",
                group(p).child(field("Download folder", "Files from phones and PCs go here.", &form.download_dir, true)).child(
                    switch_row(
                        "ask-receive",
                        "Ask before receiving files",
                        "Each file waits for Accept. No answer in 2 minutes declines it.",
                        form.ask_before_receiving,
                        false,
                        |this, on, cx| {
                            this.form.ask_before_receiving = on;
                            this.save_switches(cx);
                        },
                    ),
                ),
            ))
            .child(section(
                p,
                "Shell",
                group(p)
                    .child(switch_row(
                        "terminal",
                        "Allow the phone's Shell",
                        "A paired phone can open a terminal on this PC.",
                        form.terminal,
                        true,
                        |this, on, cx| {
                            this.form.terminal = on;
                            this.save_switches(cx);
                        },
                    ))
                    .child(field("PIN", "Asked before the Shell opens.", &form.terminal_pin, false))
                    .child(field("Program", "Empty: your login shell.", &form.shell, false)),
            ))
            .child(section(
                p,
                "Cast",
                group(p)
                    .child(switch_row(
                        "cast-screen-off",
                        "Turn the phone screen off",
                        "Its picture stays on the PC; saves battery.",
                        form.cast_screen_off,
                        true,
                        |this, on, cx| {
                            this.form.cast_screen_off = on;
                            this.save_switches(cx);
                        },
                    ))
                    .child(switch_row(
                        "cast-audio",
                        "Phone sound on the PC",
                        "Android 11 or newer.",
                        form.cast_audio,
                        false,
                        |this, on, cx| {
                            this.form.cast_audio = on;
                            this.save_switches(cx);
                        },
                    ))
                    .child(field("Max size", "Longest side in pixels; empty for full.", &form.cast_max_size, false)),
            ))
            .child(section(
                p,
                "Advanced",
                group(p).child(field("Port", "Applies after a restart.", &form.port, true)).child(field(
                    "Webcam size",
                    "Fixed size of Dotlink Camera.",
                    &form.camera_size,
                    false,
                )),
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        pill_icon(p, "save", IconName::Save, "Save", Kind::Primary)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.save_form(cx))),
                    )
                    .when(restart, |d| {
                        d.child(
                            div().text_color(p.ink_muted).child("Restart Dotlink to apply the port or webcam size."),
                        )
                    }),
            )
            .child(section(
                p,
                "About",
                group(p)
                    .child(kv(p, "Version", version))
                    .child(self.update_row(p, cx))
                    .child(
                        kv(p, "Config", dotlink_core::config::config_path().display().to_string())
                            .border_t_1()
                            .border_color(p.line),
                    )
                    .child(
                        row(p)
                            .border_t_1()
                            .border_color(p.line)
                            .child(div().flex_1().min_w_0().text_color(p.ink_muted).child("Found a bug or have an idea?"))
                            .child(
                                pill_icon(p, "report-bug", IconName::Bug, "Report a bug or suggestion", Kind::Ghost)
                                    .on_click(|_, _, cx| cx.open_url("https://github.com/sofyan-rs/dotlink/issues")),
                            ),
                    )
                    .child(
                        row(p)
                            .border_t_1()
                            .border_color(p.line)
                            .child(div().flex_1().min_w_0().text_color(p.ink_muted).child("Dotlink is made by Kurodev."))
                            .child(
                                pill(p, "kurodev", "kurodev.org", Kind::Ghost)
                                    .on_click(|_, _, cx| cx.open_url("https://kurodev.org")),
                            ),
                    )
                    .child(
                        row(p)
                            .border_t_1()
                            .border_color(p.line)
                            .child(div().flex_1().min_w_0().text_color(p.ink_muted).child(if owns {
                                "Quitting stops Dotlink until you open it again."
                            } else {
                                "The service runs separately (dotlink daemon); this window only controls it."
                            }))
                            .child(
                                pill_icon(p, "quit", IconName::Power, if owns { "Quit Dotlink" } else { "Close" }, Kind::Danger)
                                    .on_click(|_, _, cx| super::quit(cx)),
                            ),
                    ),
            ))
    }

    /// Settings → About: the update check, and the update once one is found.
    fn update_row(&self, p: &Palette, cx: &mut Context<Self>) -> Div {
        let state = &self.model.read(cx).update;
        let (text, button): (String, Option<(&str, bool)>) = match state {
            UpdateState::Idle => ("Updates come from the GitHub releases.".into(), Some(("Check for updates", true))),
            UpdateState::Checking => ("Checking for updates…".into(), Some(("Check for updates", false))),
            UpdateState::UpToDate => ("Dotlink is up to date.".into(), Some(("Check again", true))),
            UpdateState::Failed(e) => (format!("Couldn't check: {e}"), Some(("Try again", true))),
            UpdateState::Available(r) => (
                format!("Dotlink {}{} is available.", r.version, if r.prerelease { " (pre-release)" } else { "" }),
                Some((if r.asset.is_some() { "Update and restart" } else { "Download" }, true)),
            ),
            UpdateState::Downloading(r, done) => {
                (format!("Downloading Dotlink {}: {}%", r.version, (done * 100.) as u32), None)
            }
        };
        let available = matches!(state, UpdateState::Available(_));
        let release_page = match state {
            UpdateState::Available(r) => Some(r.page_url.clone()),
            _ => None,
        };
        row(p)
            .border_t_1()
            .border_color(p.line)
            .child(div().flex_1().min_w_0().text_color(p.ink_muted).child(text))
            .when_some(release_page, |r, url| {
                r.child(pill_icon(p, "release-notes", IconName::Sparkles, "What's new", Kind::Ghost).on_click(move |_, _, cx| cx.open_url(&url)))
            })
            .when_some(button, |r, (label, on)| {
                r.child(enabled(
                    // Installing downloads; checking looks again.
                    pill_icon(
                        p,
                        "update",
                        if available { IconName::Download } else { IconName::RefreshCw },
                        label,
                        if available { Kind::Primary } else { Kind::Ghost },
                    )
                    .on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            this.model.update(cx, |m, cx| {
                                if available { m.install_update(cx) } else { m.check_update(true, cx) }
                            })
                        },
                    )),
                    on,
                ))
            })
    }

    fn not_connected(&self, p: &Palette, _cx: &mut Context<Self>) -> impl IntoElement {
        group(p).child(
            row(p)
                .child(status_dot(p.accent))
                .child(div().flex_1().min_w_0().text_color(p.ink_muted).child("Waiting for the Dotlink service…")),
        )
    }
}

// ---------- building blocks ----------

#[derive(Clone, Copy)]
enum Kind {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

/// Dotline button.
fn pill(p: &Palette, id: impl Into<gpui::ElementId>, label: impl Into<SharedString>, kind: Kind) -> Stateful<Div> {
    pill_with(p, id, None, label, kind)
}

/// A [pill] with a Lucide icon before its label, for actions (Send, Cast, Copy…).
fn pill_icon(
    p: &Palette,
    id: impl Into<gpui::ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
    kind: Kind,
) -> Stateful<Div> {
    pill_with(p, id, Some(icon), label, kind)
}

fn pill_with(
    p: &Palette,
    id: impl Into<gpui::ElementId>,
    icon: Option<IconName>,
    label: impl Into<SharedString>,
    kind: Kind,
) -> Stateful<Div> {
    let (bg, fg, hover): (Hsla, Hsla, Hsla) = match kind {
        Kind::Primary => (p.ink, p.on_ink, p.ink.opacity(0.85)),
        Kind::Secondary => (p.surface_raised, p.ink, p.nav_selected),
        Kind::Ghost => (gpui::transparent_black(), p.ink, p.surface_raised),
        Kind::Danger => (p.accent, p.on_accent, p.accent.opacity(0.85)),
    };
    div()
        .id(id.into())
        .flex_none()
        .h(px(CONTROL))
        .px(px(16.))
        .flex()
        .items_center()
        .rounded(px(RADIUS))
        .bg(bg)
        .text_color(fg)
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .when(matches!(kind, Kind::Ghost), |d| d.border_1().border_color(p.line_strong))
        .hover(move |s| s.bg(hover))
        .when_some(icon, |d, icon| d.gap(px(8.)).child(Icon::new(icon).size(px(16.)).text_color(fg)))
        .child(label.into())
}

/// Dotline segmented control (SegmentedControl.kt): a `surface_raised` pill well with 4 px
/// padding; the selected segment is an `ink` pill with `on_ink` text, the others muted.
fn segmented(
    p: &Palette,
    id: &'static str,
    options: &[&'static str],
    selected: usize,
    on_select: impl Fn(usize, &mut App) + 'static,
) -> Div {
    let on_select = std::rc::Rc::new(on_select);
    div().flex().gap(px(4.)).p(px(4.)).rounded(px(RADIUS)).bg(p.surface_raised).children(options.iter().enumerate().map(
        |(i, label)| {
            let on_select = on_select.clone();
            let on = i == selected;
            div()
                .id((id, i))
                .h(px(CONTROL - 8.))
                .px(px(16.))
                .flex()
                .items_center()
                // Inside the well's 4 px padding, so its corners follow the well's.
                .rounded(px(RADIUS - 4.))
                .font_family(MONO)
                .text_size(px(12.))
                .cursor_pointer()
                .when(on, |d| d.bg(p.ink).text_color(p.on_ink))
                .when(!on, |d| d.text_color(p.ink_muted).hover(|s| s.text_color(p.ink)))
                .child(*label)
                .on_click(move |_, _, cx| on_select(i, cx))
        },
    ))
}

/// A button showing the chosen phone that opens a menu of `options` (device id, or None for
/// "all"; name).
fn phone_picker(
    id: &'static str,
    options: Vec<(Option<String>, String)>,
    selected: Option<String>,
    on_select: impl Fn(Option<String>, &mut App) + 'static,
) -> impl IntoElement {
    let label = options.iter().find(|(d, _)| *d == selected).map(|(_, n)| n.clone()).unwrap_or_default();
    let on_select = std::rc::Rc::new(on_select);
    Button::new(id).outline().h(px(CONTROL)).label(label).dropdown_caret(true).dropdown_menu_with_anchor(
        gpui::Anchor::TopRight,
        move |mut menu, _, _| {
            for (device, name) in &options {
                let on_select = on_select.clone();
                let device = device.clone();
                menu = menu.item(
                    PopupMenuItem::new(name.clone())
                        .checked(device == selected)
                        .on_click(move |_, _, cx| on_select(device.clone(), cx)),
                );
            }
            menu.min_w(px(200.))
        },
    )
}

/// Greys out a pill that can't be used right now.
fn enabled(button: Stateful<Div>, on: bool) -> Stateful<Div> {
    if on { button } else { button.opacity(0.4).cursor_default() }
}

fn page(p: &Palette, title: &str, subtitle: Option<&str>) -> Div {
    div().flex().flex_col().gap(px(24.)).child(
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(div().text_size(px(28.)).font_weight(FontWeight::SEMIBOLD).child(title.to_string()))
            .when_some(subtitle, |d, s| d.child(div().text_color(p.ink_muted).child(s.to_string()))),
    )
}

fn section(p: &Palette, label: &str, body: impl IntoElement) -> Div {
    section_with::<Stateful<Div>>(p, label, None, body)
}

fn section_with<A: IntoElement>(p: &Palette, label: &str, action: Option<A>, body: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .px(px(4.))
                .child(mono_label(p, label.to_uppercase()))
                .children(action),
        )
        .child(body)
}

/// A rounded card holding rows (Dotline list group).
fn group(p: &Palette) -> Div {
    div().flex().flex_col().rounded(px(CARD_RADIUS)).bg(p.surface).border_1().border_color(p.line).overflow_hidden()
}

fn row(_p: &Palette) -> Div {
    div().flex().items_center().gap(px(12.)).px(px(16.)).py(px(12.)).min_h(px(52.))
}

fn kv(p: &Palette, key: &str, value: String) -> Div {
    row(p)
        .child(div().w(px(120.)).text_color(p.ink_muted).child(key.to_string()))
        .child(div().flex_1().min_w_0().font_family(MONO).text_size(px(14.)).child(value))
}

fn mono_label(p: &Palette, text: impl Into<SharedString>) -> Div {
    div().font_family(MONO).text_size(px(12.)).text_color(p.ink_faint).child(text.into())
}

/// Dotline dot meter: `count` dots, the first `lit` in ink, the rest faint (DotMeter.kt).
fn dot_meter(p: &Palette, count: usize, lit: usize) -> Div {
    let unlit = p.line_strong.opacity(0.5);
    div()
        .flex_none()
        .flex()
        .gap(px(4.))
        .children((0..count).map(|i| div().size(px(6.)).rounded(px(3.)).bg(if i < lit { p.ink } else { unlit })))
}

/// The trailing value of a list row ("Done", "Failed").
fn row_value(text: &'static str, color: Hsla) -> Div {
    div().flex_none().font_family(MONO).text_size(px(13.)).text_color(color).child(text)
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// ↑ to the phone, ↓ from it.
fn direction(p: &Palette, to_phone: bool) -> Div {
    div()
        .flex_none()
        .size(px(28.))
        .rounded(px(14.))
        .bg(p.surface_raised)
        .flex()
        .items_center()
        .justify_center()
        .font_family(MONO)
        .text_size(px(15.))
        .text_color(if to_phone { p.ink } else { p.positive })
        .child(if to_phone { "↑" } else { "↓" })
}

/// The phone's battery: a small battery filled to its level, the percentage, and "Charging".
/// Green while charging, red when low (15 % or less) and not charging.
fn battery(p: &Palette, b: Battery) -> Div {
    let colour = if b.charging {
        p.positive
    } else if b.level <= 15 {
        p.accent
    } else {
        p.ink
    };
    // 20 × 10 body with a 1 px outline and 1 px inner gap, so the bar is 16 px at 100 %.
    let fill = 16. * b.level.min(100) as f32 / 100.;
    let glyph = div()
        .flex()
        .items_center()
        .child(
            div()
                .w(px(20.))
                .h(px(10.))
                .rounded(px(2.5))
                .border_1()
                .border_color(colour)
                .p(px(1.))
                .child(div().h_full().w(px(fill)).rounded(px(1.)).bg(colour)),
        )
        // The terminal.
        .child(div().w(px(2.)).h(px(4.)).rounded_r(px(1.)).bg(colour));
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(glyph)
        .child(
            div()
                .font_family(MONO)
                .text_size(px(13.))
                .text_color(if b.charging { p.positive } else { p.ink_muted })
                .child(if b.charging { format!("{}% · Charging", b.level) } else { format!("{}%", b.level) }),
        )
}

fn status_dot(color: Hsla) -> Div {
    div().flex_none().size(px(8.)).rounded(px(4.)).bg(color)
}

fn step(p: &Palette, n: &str, text: &str) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .child(
            div()
                .flex_none()
                .size(px(26.))
                .rounded(px(13.))
                .border_1()
                .border_color(p.line_strong)
                .flex()
                .items_center()
                .justify_center()
                .font_family(MONO)
                .text_size(px(13.))
                .child(n.to_string()),
        )
        .child(div().flex_1().min_w_0().child(text.to_string()))
}

fn transfer_row(p: &Palette, t: &Transfer) -> Div {
    let path = PathBuf::from(&t.path);
    let who = if t.device.is_empty() { "your phone".to_string() } else { t.device.clone() };
    let (line, color) = match &t.state {
        TransferState::Waiting => (format!("Waiting for {who}"), p.ink_muted),
        TransferState::Active => (
            format!(
                "{} {who} · {}",
                if t.outgoing { "Sending to" } else { "Receiving from" },
                progress_text(t.done, t.size)
            ),
            p.ink_muted,
        ),
        TransferState::Done => (
            format!("{} {who} · {}", if t.outgoing { "Sent to" } else { "From" }, human_size(t.size.unwrap_or(t.done))),
            p.ink_muted,
        ),
        TransferState::Failed(e) => (capitalize(e), p.accent_text),
    };
    // Like the phone app: 8 dots fill up while it runs, then "Done" or "Failed".
    let lit = match (&t.state, t.size) {
        (TransferState::Active, Some(size)) if size > 0 => Some((t.done * 8 / size).min(8) as usize),
        (TransferState::Active | TransferState::Waiting, _) => Some(0),
        _ => None,
    };
    let done_incoming = t.state == TransferState::Done && !t.outgoing && !t.path.is_empty();
    let id = t.id.clone();
    row(p)
        .child(direction(p, t.outgoing))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(div().truncate().child(t.name.clone()))
                .child(div().font_family(MONO).text_size(px(13.)).text_color(color).child(line)),
        )
        .when_some(lit, |r, lit| r.child(dot_meter(p, 8, lit)))
        .when(t.state == TransferState::Done && !done_incoming, |r| r.child(row_value("Done", p.positive)))
        .when(matches!(t.state, TransferState::Failed(_)), |r| r.child(row_value("Failed", p.accent_text)))
        .when(done_incoming, |r| {
            let open = path.clone();
            r.child(
                pill_icon(p, SharedString::from(format!("open-{id}")), IconName::ExternalLink, "Open", Kind::Secondary)
                    .on_click(move |_, _, _| platform::open_path(&open)),
            )
            .child(
                pill_icon(p, SharedString::from(format!("show-{id}")), IconName::Folder, "Show", Kind::Ghost)
                    .on_click(move |_, _, _| platform::show_in_folder(&path)),
            )
        })
}

/// A QR code's dark modules, as horizontal runs per row: (row, first column, length).
pub struct Qr {
    width: usize,
    runs: Vec<(usize, usize, usize)>,
}

impl Qr {
    fn new(data: &str) -> Self {
        let Ok(code) = qrcode::QrCode::new(data.as_bytes()) else { return Self { width: 0, runs: Vec::new() } };
        let width = code.width();
        let dark: Vec<bool> = code.to_colors().into_iter().map(|m| m == qrcode::Color::Dark).collect();
        let mut runs = Vec::new();
        for (y, row) in dark.chunks(width).enumerate() {
            let mut x = 0;
            while x < width {
                if !row[x] {
                    x += 1;
                    continue;
                }
                let start = x;
                while x < width && row[x] {
                    x += 1;
                }
                runs.push((y, start, x - start));
            }
        }
        Self { width, runs }
    }
}

/// The pairing QR code, dark modules on a white tile (scanners want that, also in dark mode).
fn qr(code: Option<Arc<Qr>>, side: f32) -> impl IntoElement {
    div().flex_none().size(px(side)).rounded(px(CARD_RADIUS)).bg(gpui::white()).p(px(14.)).child(
        canvas(
            |_, _, _| {},
            move |bounds: Bounds<gpui::Pixels>, _, window, _| {
                let Some(code) = code.as_ref().filter(|c| c.width > 0) else { return };
                let cell = bounds.size.width / code.width as f32;
                for &(y, x, len) in &code.runs {
                    let origin = point(bounds.origin.x + cell * x as f32, bounds.origin.y + cell * y as f32);
                    // A hair of overlap so neighbouring modules don't show seams.
                    let size = size(cell * len as f32 + px(0.5), cell + px(0.5));
                    window.paint_quad(fill(Bounds::new(origin, size), gpui::black()));
                }
            },
        )
        .size_full(),
    )
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1000. && unit < UNITS.len() - 1 {
        v /= 1000.;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{v:.1} {}", UNITS[unit]) }
}

fn progress_text(done: u64, size: Option<u64>) -> String {
    match size {
        Some(size) if size > 0 => format!("{} of {}", human_size(done), human_size(size)),
        _ => human_size(done),
    }
}

/// "just now", "5 min ago", "3 days ago" for a unix time.
fn ago(unix: u64) -> String {
    let secs = dotlink_core::config::now_secs().saturating_sub(unix);
    ago_secs(secs)
}

fn ago_instant(at: std::time::Instant) -> String {
    ago_secs(at.elapsed().as_secs()).to_uppercase()
}

fn ago_secs(secs: u64) -> String {
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86400 => format!("{} h ago", secs / 3600),
        86400..172800 => "yesterday".into(),
        _ => format!("{} days ago", secs / 86400),
    }
}
