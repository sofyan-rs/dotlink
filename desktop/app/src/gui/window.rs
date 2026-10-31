//! The Dotlink window: a sidebar listing the devices, a page per device, and Pair, Activity,
//! Setup and Settings.

use super::client::{DeviceInfo, Status};
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
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::notification::Notification;
use gpui_component::button::{Button, ButtonVariants as _};
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
    /// The device whose name is being edited on its page, and the field.
    renaming: Option<String>,
    rename_input: Entity<InputState>,
    /// The device Forget was picked for, waiting on its confirmation: (id, name, a phone).
    forgetting: Option<(String, String, bool)>,
    /// Browse from PC: the field for renaming an entry or naming a new folder, and which.
    file_input: Entity<InputState>,
    file_edit: Option<FileEdit>,
    /// An entry of the open phone folder waiting for "Delete?".
    file_deleting: Option<String>,
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
                        Page::Device => !m.starting.is_empty(),
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
        let rename_input = cx.new(|cx| InputState::new(window, cx).placeholder("Name"));
        let mut subs = subs;
        // Enter or clicking elsewhere saves the new name (Esc cancels, on the field).
        subs.push(cx.subscribe_in(&rename_input, window, |this, _, event: &InputEvent, _, cx| {
            if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                this.save_rename(cx);
            }
        }));
        let file_input = cx.new(|cx| InputState::new(window, cx).placeholder("Name"));
        subs.push(cx.subscribe_in(&file_input, window, |this, _, event: &InputEvent, _, cx| match event {
            InputEvent::PressEnter { .. } => this.save_file_edit(cx),
            InputEvent::Blur => {
                this.file_edit = None;
                cx.notify();
            }
            _ => {}
        }));
        Self {
            model,
            form,
            cast_code,
            qr: None,
            pair_tab: 0,
            join_link,
            join_pick: None,
            join_code,
            renaming: None,
            rename_input,
            forgetting: None,
            file_input,
            file_edit: None,
            file_deleting: None,
            _subs: subs,
        }
    }

    /// Turns the device's name on its page into a field.
    fn start_rename(&mut self, id: String, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.rename_input.update(cx, |s, cx| s.set_value(name, window, cx));
        self.rename_input.update(cx, |s, cx| s.focus(window, cx));
        self.renaming = Some(id);
        cx.notify();
    }

    fn save_rename(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.renaming.take() else { return };
        let name = self.rename_input.read(cx).value().trim().to_string();
        if !name.is_empty() {
            self.model.update(cx, |m, cx| m.rename_device(id, name, cx));
        }
        cx.notify();
    }

    /// Renaming an entry of the open phone folder, or naming a new folder there.
    fn start_file_edit(&mut self, edit: FileEdit, window: &mut Window, cx: &mut Context<Self>) {
        let value = match &edit {
            FileEdit::Rename(name) => name.clone(),
            FileEdit::NewFolder => String::new(),
        };
        self.file_input.update(cx, |s, cx| s.set_value(value, window, cx));
        self.file_input.update(cx, |s, cx| s.focus(window, cx));
        self.file_edit = Some(edit);
        cx.notify();
    }

    fn save_file_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.file_edit.take() else { return };
        let name = self.file_input.read(cx).value().trim().to_string();
        if name.is_empty() || name.contains('/') {
            cx.notify();
            return;
        }
        self.model.update(cx, |m, cx| match edit {
            FileEdit::Rename(old) if old != name => m.rename_phone_file(old, name, cx),
            FileEdit::Rename(_) => {}
            FileEdit::NewFolder => m.new_phone_folder(name, cx),
        });
        cx.notify();
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

    /// Picks files and sends them to the device `id`.
    fn pick_and_send(&mut self, id: String, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Send".into()),
        });
        let model = self.model.clone();
        cx.spawn(async move |_, cx| {
            if let Ok(Ok(Some(paths))) = picked.await {
                model.update(cx, |m, cx| m.send_files_to(paths, id, cx));
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
            Page::Device => self.device_page(&p, cx),
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
        let body = div().size_full().relative().child(body).children(self.confirm_forget(&p, cx)).children(self.confirm_delete_file(&p, cx));
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

/// What the field in a phone folder is for.
#[derive(Clone)]
enum FileEdit {
    Rename(String),
    NewFolder,
}

// ---------- confirming Forget ----------

impl MainView {
    /// "Delete <name>?" for an entry of the open phone folder: it can't be undone.
    fn confirm_delete_file(&self, p: &Palette, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let name = self.file_deleting.clone()?;
        let cancel = |this: &mut Self, cx: &mut Context<Self>| {
            this.file_deleting = None;
            cx.notify();
        };
        Some(
            div()
                .id("delete-backdrop")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::black().opacity(0.6))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| cancel(this, cx)))
                .child(
                    div()
                        .id("delete-dialog")
                        .w(px(400.))
                        .p(px(24.))
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .rounded(px(CARD_RADIUS))
                        .bg(p.surface)
                        .border_1()
                        .border_color(p.line_strong)
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).child(format!("Delete {name}?")))
                        .child(div().text_color(p.ink_muted).child("It's deleted on the phone, with what's in it for a folder. This can't be undone."))
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .pt(px(12.))
                                .child(pill(p, "delete-cancel", "Cancel", Kind::Secondary).flex_1().justify_center().on_click(cx.listener(
                                    move |this, _: &ClickEvent, _, cx| cancel(this, cx),
                                )))
                                .child(pill(p, "delete-confirm", "Delete", Kind::Danger).flex_1().justify_center().on_click(cx.listener(
                                    move |this, _: &ClickEvent, _, cx| {
                                        let name = name.clone();
                                        this.file_deleting = None;
                                        this.model.update(cx, |m, cx| m.delete_phone_file(name, cx));
                                    },
                                ))),
                        ),
                ),
        )
    }

    /// Device page → Now playing: the phone's player, with its artwork and controls.
    fn now_playing(&self, p: &Palette, d: &DeviceInfo, cx: &mut Context<Self>) -> Option<Div> {
        let m = d.media.as_ref()?;
        let control = |id: &'static str, icon: IconName, action: &'static str, cx: &mut Context<Self>| {
            let device = d.id.clone();
            div()
                .id(id)
                .size(px(CONTROL))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .hover(|s| s.bg(p.ink.opacity(0.06)))
                .child(Icon::new(icon).size(px(18.)).text_color(p.ink))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let device = device.clone();
                    this.model.update(cx, |m, cx| m.phone_media(device, action, cx))
                }))
        };
        let art: AnyElement = match &m.art {
            Some(path) => gpui::img(path.clone()).size(px(56.)).rounded(px(8.)).into_any_element(),
            None => div()
                .size(px(56.))
                .rounded(px(8.))
                .bg(p.ink.opacity(0.06))
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(IconName::Music).size(px(22.)).text_color(p.ink_muted))
                .into_any_element(),
        };
        let time = |ms: u64| format!("{}:{:02}", ms / 60_000, ms / 1000 % 60);
        let detail = if m.duration_ms > 0 {
            format!("{} · {} / {}", m.player.to_uppercase(), time(m.position_ms), time(m.duration_ms))
        } else {
            m.player.to_uppercase()
        };
        Some(
            group(p).child(
                row(p)
                    .gap(px(14.))
                    .child(art)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(div().truncate().child(m.title.clone()))
                            .child(div().truncate().text_size(px(13.)).text_color(p.ink_muted).child(m.artist.clone()))
                            .child(div().font_family(MONO).text_size(px(11.)).text_color(p.ink_muted).child(detail)),
                    )
                    .child(control("media-prev", IconName::SkipBack, "prev", cx))
                    .child(control("media-toggle", if m.playing { IconName::Pause } else { IconName::Play }, "toggle", cx))
                    .child(control("media-next", IconName::SkipForward, "next", cx)),
            ),
        )
    }

    /// Device page → Files (a connected phone): its storage, folders first. Click opens a
    /// folder, double-click downloads and opens a file, ⋯ has Download, Rename and Delete;
    /// files dropped on the list go into the open folder.
    fn files_section(&self, p: &Palette, d: &DeviceInfo, cx: &mut Context<Self>) -> Div {
        let folder = self.model.read(cx).folder.as_ref().filter(|f| f.device == d.id);
        let Some(folder) = folder else {
            let id = d.id.clone();
            return group(p).child(
                row(p)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(format!("{}'s storage", d.name))
                            .child(div().text_size(px(13.)).text_color(p.ink_muted).child("Browse, download, upload, rename and delete. Needs Browse from PC on in the phone app.")),
                    )
                    .child(pill_icon(p, "files-open", IconName::FolderOpen, "Browse", Kind::Secondary).on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            let id = id.clone();
                            this.model.update(cx, |m, cx| m.open_folder(id, "/".into(), cx))
                        },
                    ))),
            );
        };
        let (id, parent) = (d.id.clone(), folder.parent());
        let icon_button = |id: &'static str, icon: IconName| {
            div()
                .id(id)
                .size(px(CONTROL))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .cursor_pointer()
                .hover(|s| s.bg(p.ink.opacity(0.06)))
                .child(Icon::new(icon).size(px(16.)).text_color(p.ink_muted))
        };
        let up_id = id.clone();
        let refresh = (id.clone(), folder.path.clone());
        let toolbar = row(p)
            .gap(px(4.))
            .child(enabled(icon_button("files-up", IconName::ArrowUp), parent.is_some()).when_some(parent, |b, up| {
                b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let (id, up) = (up_id.clone(), up.clone());
                    this.model.update(cx, |m, cx| m.open_folder(id, up, cx))
                }))
            }))
            .child(div().flex_1().min_w_0().truncate().font_family(MONO).text_size(px(13.)).child(if folder.loading {
                format!("{}  …", folder.path)
            } else {
                folder.path.clone()
            }))
            .child(icon_button("files-new", IconName::FolderPlus).on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                this.start_file_edit(FileEdit::NewFolder, window, cx)
            })))
            .child(icon_button("files-refresh", IconName::RefreshCw).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                let (id, path) = refresh.clone();
                this.model.update(cx, |m, cx| m.open_folder(id, path, cx))
            })))
            .child(icon_button("files-close", IconName::X).on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.model.update(cx, |m, cx| m.close_folder(cx))
            })));
        let edit = self.file_edit.as_ref().map(|e| {
            let label = match e {
                FileEdit::Rename(name) => format!("RENAME {}", name.to_uppercase()),
                FileEdit::NewFolder => "NEW FOLDER".into(),
            };
            row(p)
                .border_t_1()
                .border_color(p.line)
                .flex_col()
                .items_start()
                .gap(px(6.))
                .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, _, cx| {
                    if e.keystroke.key == "escape" {
                        this.file_edit = None;
                        cx.stop_propagation();
                        cx.notify();
                    }
                }))
                .child(mono_label(p, format!("{label} · ENTER TO SAVE · ESC TO CANCEL")))
                .child(div().w_full().child(Input::new(&self.file_input)))
        });
        let error = folder.error.clone().map(|e| row(p).border_t_1().border_color(p.line).child(div().text_color(p.accent_text).child(e)));
        let empty = (!folder.loading && folder.error.is_none() && folder.entries.is_empty())
            .then(|| row(p).border_t_1().border_color(p.line).child(div().text_color(p.ink_muted).child("Empty folder. Drop files here to send them into it.")));
        let view = cx.entity().downgrade();
        let rows = folder.entries.iter().enumerate().map(|(i, e)| {
            let (name, dir) = (e.name.clone(), e.dir);
            let detail = if dir { format!("Folder · {}", ago(e.mtime)) } else { format!("{} · {}", human_size(e.size), ago(e.mtime)) };
            let open_id = id.clone();
            let child = folder.child(&name);
            let click_name = name.clone();
            let menu_view = view.clone();
            let menu_name = name.clone();
            row(p)
                .id(("phone-file", i))
                .border_t_1()
                .border_color(p.line)
                .cursor_pointer()
                .hover(|s| s.bg(p.ink.opacity(0.04)))
                .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                    if dir {
                        let (id, path) = (open_id.clone(), child.clone());
                        this.model.update(cx, |m, cx| m.open_folder(id, path, cx));
                    } else if e.click_count() >= 2 {
                        let name = click_name.clone();
                        this.model.update(cx, |m, cx| m.download_phone_file(name, true, cx));
                    }
                }))
                .child(Icon::new(if dir { IconName::Folder } else { IconName::File }).size(px(18.)).text_color(p.ink_muted))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().truncate().child(name.clone()))
                        .child(div().text_size(px(12.)).font_family(MONO).text_color(p.ink_muted).child(detail)),
                )
                .child(
                    Button::new(("phone-file-more", i)).ghost().icon(Icon::new(IconName::Ellipsis)).dropdown_menu_with_anchor(
                        gpui::Anchor::TopRight,
                        move |menu, _, _| {
                            let (dl_view, rn_view, rm_view) = (menu_view.clone(), menu_view.clone(), menu_view.clone());
                            let (dl_name, rn_name, rm_name) = (menu_name.clone(), menu_name.clone(), menu_name.clone());
                            let menu = if dir {
                                menu
                            } else {
                                menu.item(PopupMenuItem::new("Download").on_click(move |_, _, cx| {
                                    let name = dl_name.clone();
                                    dl_view.update(cx, |this, cx| this.model.update(cx, |m, cx| m.download_phone_file(name, false, cx))).ok();
                                }))
                            };
                            menu.item(PopupMenuItem::new("Rename").on_click(move |_, window, cx| {
                                let name = rn_name.clone();
                                rn_view.update(cx, |this, cx| this.start_file_edit(FileEdit::Rename(name), window, cx)).ok();
                            }))
                            .item(PopupMenuItem::new("Delete").on_click(move |_, _, cx| {
                                let name = rm_name.clone();
                                rm_view
                                    .update(cx, |this, cx| {
                                        this.file_deleting = Some(name);
                                        cx.notify();
                                    })
                                    .ok();
                            }))
                            .min_w(px(140.))
                        },
                    ),
                )
        });
        let tint = p.ink.opacity(0.04);
        let list = div()
            .id("phone-files")
            .max_h(px(440.))
            .overflow_y_scroll()
            .drag_over::<ExternalPaths>(move |s, _, _, _| s.bg(tint))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                let paths = paths.paths().to_vec();
                this.model.update(cx, |m, cx| m.upload_to_folder(paths, cx));
            }))
            .children(empty)
            .children(rows);
        group(p).child(toolbar).children(edit).children(error).child(list)
    }

    /// "Forget <name>?" over the window, like the phone app's dialog: the backdrop or Esc
    /// cancels.
    fn confirm_forget(&self, p: &Palette, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let (id, name, phone) = self.forgetting.clone()?;
        let body = if phone {
            "It stops connecting to this PC right away. To link it again, pair it from Pair."
        } else {
            "The two PCs stop sending each other files and the clipboard. Join again to link them."
        };
        let cancel = |this: &mut Self, cx: &mut Context<Self>| {
            this.forgetting = None;
            cx.notify();
        };
        Some(
            div()
                .id("forget-backdrop")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::black().opacity(0.6))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| cancel(this, cx)))
                .on_key_down(cx.listener(move |this, e: &gpui::KeyDownEvent, _, cx| {
                    if e.keystroke.key == "escape" {
                        cancel(this, cx);
                    }
                }))
                .child(
                    div()
                        .id("forget-dialog")
                        .w(px(400.))
                        .p(px(24.))
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .rounded(px(CARD_RADIUS))
                        .bg(p.surface)
                        .border_1()
                        .border_color(p.line_strong)
                        // Clicks inside the card don't reach the backdrop.
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).child(format!("Forget {name}?")))
                        .child(div().text_color(p.ink_muted).child(body))
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .pt(px(12.))
                                .child(pill(p, "forget-cancel", "Cancel", Kind::Secondary).flex_1().justify_center().on_click(cx.listener(
                                    move |this, _: &ClickEvent, _, cx| cancel(this, cx),
                                )))
                                .child(pill(p, "forget-confirm", "Forget", Kind::Danger).flex_1().justify_center().on_click(cx.listener(
                                    move |this, _: &ClickEvent, _, cx| {
                                        let id = id.clone();
                                        this.forgetting = None;
                                        this.model.update(cx, |m, cx| m.unpair(id, cx));
                                    },
                                ))),
                        ),
                ),
        )
    }
}

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
                            .child(div().truncate().child(f.names()))
                            .child(
                                div()
                                    .font_family(MONO)
                                    .text_size(px(13.))
                                    .text_color(p.ink_muted)
                                    .child(format!("{} wants to send {}{size}", f.from, if f.count > 1 { "them" } else { "it" })),
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
    /// DOTLINK, the paired phones and linked PCs (each opens its page), Pair, then the pages
    /// about this PC.
    fn sidebar(&self, p: &Palette, page: Page, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let connected = model.connected;
        let hostname = model.status.as_ref().map(|s| s.name.clone()).unwrap_or_default();
        let (phones, pcs) = model.online_counts();
        let current = model.current_device().filter(|_| page == Page::Device).map(|d| d.id.clone());
        let devices: Vec<DeviceInfo> = model.devices().into_iter().cloned().collect();
        let mut list = div().flex().flex_col().gap(px(4.));
        for d in devices {
            let selected = current.as_deref() == Some(d.id.as_str());
            let id = d.id.clone();
            list = list.child(
                nav_item(p, SharedString::from(format!("nav-device-{}", d.id)), selected)
                    // The dot in an icon-wide box, so names line up with the labels below.
                    .child(div().w(px(18.)).flex().justify_center().child(status_dot(if d.online { p.positive } else { p.ink_faint })))
                    .child(div().flex_1().min_w_0().truncate().child(d.name.clone()))
                    .children(d.battery.map(|b| mono_label(p, format!("{}%", b.level))))
                    .child(Icon::new(device_icon(&d)).size(px(16.)).text_color(p.ink_faint))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        let id = id.clone();
                        this.model.update(cx, |m, cx| m.open_device(id, cx));
                    })),
            );
        }
        let nav = |id: &'static str, icon: IconName, label: &'static str, target: Page, cx: &mut Context<Self>| {
            nav_item(p, id, page == target)
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
                    .child(mono_label(p, "DEVICES").px(px(14.)).pb(px(4.)))
                    .child(list)
                    .child(nav("nav-pair", IconName::Plus, "Pair", Page::Pair, cx))
                    .child(div().h(px(1.)).mx(px(14.)).my(px(8.)).bg(p.line))
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
    /// One phone or PC, picked in the sidebar: what can be done with it (tiles), its settings
    /// and the latest transfers with it.
    fn device_page(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let (status, device) = {
            let m = self.model.read(cx);
            (m.status.clone(), m.current_device().cloned())
        };
        let Some(status) = status else {
            return page(p, "Devices", None).child(self.not_connected(p, cx)).into_any_element();
        };
        let Some(d) = device else {
            return self.no_devices(p, cx).into_any_element();
        };
        // Sending: a drop area with its two buttons, right under the header.
        let (files_id, clip_id, link_id, drop_id) = (d.id.clone(), d.id.clone(), d.id.clone(), d.id.clone());
        let (online, name) = (d.online, d.name.clone());
        let tint = p.ink.opacity(0.04);
        let strong = p.ink_muted;
        let send = div()
            .id("send-drop")
            .flex()
            .flex_col()
            .items_center()
            .gap(px(14.))
            .px(px(16.))
            .py(px(36.))
            .rounded(px(CARD_RADIUS))
            .border_1()
            .border_dashed()
            .border_color(p.line_strong)
            .drag_over::<ExternalPaths>(move |s, _, _, _| s.bg(tint).border_color(strong))
            .on_drop(cx.listener(move |this, paths: &ExternalPaths, _, cx| {
                let paths = paths.paths().to_vec();
                let (id, name) = (drop_id.clone(), name.clone());
                this.model.update(cx, |m, cx| {
                    if online {
                        m.send_files_to(paths, id, cx)
                    } else {
                        m.notice(format!("{name} isn't connected"), true, cx)
                    }
                });
            }))
            .child(Icon::new(IconName::Upload).size(px(28.)).text_color(p.ink_muted))
            .child(div().text_color(p.ink_muted).child(if online {
                format!("Drop files to send them to {}", d.name)
            } else {
                format!("{} isn't connected", d.name)
            }))
            .child(div().flex().gap(px(8.)).child(enabled(pill_icon(p, "send-files", IconName::Files, "Send files…", Kind::Primary), online).when(online, |b| {
                b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.pick_and_send(files_id.clone(), cx)))
            }))
            .child(enabled(pill_icon(p, "send-clip", IconName::ClipboardCopy, "Send clipboard", Kind::Secondary), online).when(
                online,
                |b| {
                    b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        let id = clip_id.clone();
                        this.model.update(cx, |m, cx| m.send_clipboard_to(id, cx))
                    }))
                },
            ))
            .child(enabled(pill_icon(p, "send-link", IconName::ExternalLink, "Open copied link", Kind::Secondary), online).when(
                online,
                |b| {
                    b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        let id = link_id.clone();
                        this.model.update(cx, |m, cx| m.send_link_to(id, cx))
                    }))
                },
            )));

        // A phone's screen, camera and mic: rows that start and stop, with what each needs
        // under them.
        let streams = d.is_phone().then(|| {
            let mut rows: Vec<Div> = Vec::new();
            let (cast, note) = self.cast_row(p, &status, &d, cx);
            rows.push(cast);
            rows.extend(note);
            let features = platform::features();
            if features.camera {
                rows.push(self.stream_row(p, &status, &d, "camera", cx));
                // Only while this phone's camera is on: that's when OBS needs the link.
                if status.camera.as_deref() == Some(d.name.as_str()) {
                    rows.extend(self.obs_note(p, cx));
                }
            }
            if features.mic {
                rows.push(self.stream_row(p, &status, &d, "mic", cx));
            }
            rows.into_iter().enumerate().fold(group(p), |g, (i, r)| g.child(r.when(i > 0, |r| r.border_t_1().border_color(p.line))))
        });

        let clip_id = d.id.clone();
        let mut sync = group(p).child(
            row(p)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child("Clipboard sync")
                        .child(div().text_size(px(13.)).text_color(p.ink_muted).child(format!("Text you copy here goes to {} right away.", d.name))),
                )
                .child(Switch::new("clip-auto").checked(d.clipboard_auto).on_click(cx.listener(move |this, checked: &bool, _, cx| {
                    let (id, on) = (clip_id.clone(), *checked);
                    this.model.update(cx, |m, cx| m.set_clipboard_auto(id, on, cx));
                }))),
        );
        // A phone's notifications, and the codes in them.
        if d.is_phone() {
            let switch_row = |key: &'static str, title: &str, detail: String, checked: bool, cx: &mut Context<Self>| {
                let id = d.id.clone();
                row(p)
                    .border_t_1()
                    .border_color(p.line)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(title.to_string())
                            .child(div().text_size(px(13.)).text_color(p.ink_muted).child(detail)),
                    )
                    .child(Switch::new(key).checked(checked).on_click(cx.listener(move |this, checked: &bool, _, cx| {
                        let (id, on) = (id.clone(), *checked);
                        this.model.update(cx, |m, cx| m.set_device_option(id, key, on, cx));
                    })))
            };
            sync = sync.child(switch_row(
                "notifications",
                "Notifications",
                format!("{}'s notifications show here; Dismiss dismisses them there too. Turn on Show notifications on PC in the phone app.", d.name),
                d.notifications,
                cx,
            ));
            if d.notifications {
                sync = sync.child(switch_row(
                    "copy_codes",
                    "Copy verification codes",
                    "A code in a notification (an SMS, a bank) goes to this PC's clipboard.".into(),
                    d.copy_codes,
                    cx,
                ));
            }
            // Android 15+ hides codes from apps reading notifications until allowed over adb.
            if d.notifications && d.copy_codes && d.online && d.sdk.unwrap_or(0) >= 35 && d.sensitive_notifications == Some(false) {
                let allow_id = d.id.clone();
                sync = sync.child(
                    row(p)
                        .border_t_1()
                        .border_color(p.line)
                        .child(div().flex_1().min_w_0().text_size(px(13.)).text_color(p.ink_muted).child(format!(
                            "Android 15 hides codes in notifications from Dotlink. Allow it once with {} on USB or Wireless debugging.",
                            d.name
                        )))
                        .child(pill(p, "allow-codes", "Allow", Kind::Secondary).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            let id = allow_id.clone();
                            this.model.update(cx, |m, cx| m.allow_codes(id, cx))
                        }))),
                );
            }
        }

        let recent: Vec<&Transfer> = status.transfers.iter().filter(|t| t.device == d.name).take(3).collect();
        let recent = (!recent.is_empty()).then(|| {
            let list = recent
                .into_iter()
                .enumerate()
                .fold(group(p), |g, (i, t)| g.child(transfer_row(p, t).when(i > 0, |r| r.border_t_1().border_color(p.line))));
            section_with(
                p,
                "Recent",
                Some(pill(p, "see-activity", "See all", Kind::Ghost).on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.go(Page::Activity, cx)))),
                list,
            )
        });

        div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(div().flex().flex_col().gap(px(20.)).child(self.device_header(p, &d, cx)).child(send))
            .children(streams.map(|g| section(p, "Screen, camera & mic", g)))
            .children(self.now_playing(p, &d, cx).map(|g| section(p, "Now playing", g)))
            .children((d.is_phone() && d.online).then(|| section(p, "Files", self.files_section(p, &d, cx))))
            .child(section(p, "Sync", sync))
            .children(recent)
            .into_any_element()
    }

    /// The device's name in the display face (a field while renaming), then one mono line: how
    /// it's connected and its battery as a dot meter. ⋯ holds Rename and Forget.
    fn device_header(&self, p: &Palette, d: &DeviceInfo, cx: &mut Context<Self>) -> Div {
        let connection = match (d.online, d.is_phone(), d.usb) {
            (true, true, true) => "ONLINE · USB".to_string(),
            (true, true, false) => "ONLINE · WI-FI".to_string(),
            (true, false, _) => "ONLINE".to_string(),
            (false, true, _) => format!("OFFLINE · PAIRED {}", ago(d.paired_at).to_uppercase()),
            (false, false, _) => format!("OFFLINE · LINKED {}", ago(d.paired_at).to_uppercase()),
        };
        let renaming = self.renaming.as_deref() == Some(d.id.as_str());
        let title: AnyElement = if renaming {
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, _, cx| {
                    if e.keystroke.key == "escape" {
                        this.renaming = None;
                        cx.stop_propagation();
                        cx.notify();
                    }
                }))
                .child(
                    div().max_w(px(420.)).border_b_1().border_color(p.line_strong).child(
                        // The plain face while typing; the dot face is for reading.
                        Input::new(&self.rename_input)
                            .appearance(false)
                            .px(px(0.))
                            .py(px(0.))
                            .text_size(px(28.))
                            .line_height(px(40.))
                            .h(px(48.)),
                    ),
                )
                .into_any_element()
        } else {
            div()
                .min_w_0()
                .truncate()
                .child(page_title(&d.name))
                .into_any_element()
        };
        let battery = d.battery.map(|b| {
            let colour = if b.charging {
                p.positive
            } else if b.level <= 15 {
                p.accent_text
            } else {
                p.ink_muted
            };
            let lit = (b.level.min(100) as usize).div_ceil(10);
            let unlit = p.line_strong.opacity(0.5);
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(div().flex().gap(px(3.)).children((0..10).map(|i| div().size(px(5.)).rounded(px(2.5)).bg(if i < lit { colour } else { unlit }))))
                .child(div().text_color(colour).child(format!("{}%{}", b.level, if b.charging { " · CHARGING" } else { "" })))
        });
        let (view, id, name, phone) = (cx.entity().downgrade(), d.id.clone(), d.name.clone(), d.is_phone());
        let can_ring = phone && d.online && !d.ringing;
        let (ringing, stop_id) = (d.online && d.ringing, d.id.clone());
        let red = p.accent_text;
        let more = Button::new("device-more")
            .outline()
            .h(px(CONTROL))
            .w(px(CONTROL))
            .icon(Icon::new(IconName::Ellipsis))
            .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, _| {
                let (rename_view, forget_view, id, name) = (view.clone(), view.clone(), id.clone(), name.clone());
                let (forget_id, forget_name) = (id.clone(), name.clone());
                let (ring_view, ring_id) = (view.clone(), id.clone());
                let menu = if can_ring {
                    menu.item(PopupMenuItem::new("Ring").on_click(move |_, _, cx| {
                        let id = ring_id.clone();
                        ring_view.update(cx, |this, cx| this.model.update(cx, |m, cx| m.ring(id, true, cx))).ok();
                    }))
                } else {
                    menu
                };
                menu.item(PopupMenuItem::new("Rename").on_click(move |_, window, cx| {
                    let (id, name) = (id.clone(), name.clone());
                    rename_view.update(cx, |this, cx| this.start_rename(id, name, window, cx)).ok();
                }))
                // Red, and asks first: it can't be undone from here.
                .item(PopupMenuItem::element(move |_, _| div().text_color(red).child("Forget")).on_click(move |_, _, cx| {
                    let target = (forget_id.clone(), forget_name.clone(), phone);
                    forget_view
                        .update(cx, |this, cx| {
                            this.forgetting = Some(target);
                            cx.notify();
                        })
                        .ok();
                }))
                .min_w(px(160.))
            });
        div()
            .flex()
            .items_start()
            .gap(px(14.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(title)
                    .when(renaming, |c| c.child(mono_label(p, "ENTER TO SAVE · ESC TO CANCEL")))
                    .when(!renaming, |c| c.child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(16.))
                            .font_family(MONO)
                            .text_size(px(12.))
                            .text_color(p.ink_muted)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(status_dot(if d.online { p.positive } else { p.ink_faint }))
                                    .child(connection),
                            )
                            .children(battery)
                            .when(ringing, |c| c.child(div().text_color(red).child("RINGING"))),
                    )),
            )
            .when(!renaming && ringing, |d| {
                d.child(pill(p, "stop-ring", "Stop", Kind::Secondary).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let id = stop_id.clone();
                    this.model.update(cx, |m, cx| m.ring(id, false, cx))
                })))
            })
            .when(!renaming, |d| d.child(more))
    }

    /// Cast's row: what it does or how it's going, and Start / Stop. Plus a row under it when
    /// it needs something (Wireless debugging, a pairing code) or failed.
    fn cast_row(&self, p: &Palette, status: &Status, d: &DeviceInfo, cx: &mut Context<Self>) -> (Div, Option<Div>) {
        let cast = status.cast(&d.id);
        let (detail, live) = match &cast {
            CastState::Idle => (describe(p, "Its screen in a window, with mouse and keyboard."), false),
            CastState::Installing { .. } => (state("INSTALLING SCRCPY…", p.ink), false),
            CastState::Connecting { .. } => (state("CONNECTING…", p.ink), false),
            CastState::Casting { usb, .. } => (state(if *usb { "LIVE · USB" } else { "LIVE · WI-FI" }, p.positive), true),
            CastState::NeedsWifiDebugging { .. } => (state("TURN ON WIRELESS DEBUGGING", p.accent_text), false),
            CastState::NeedsPairing { .. } => (state("ENTER THE PAIRING CODE", p.accent_text), false),
            CastState::Failed { .. } => (state("FAILED", p.accent_text), false),
        };
        let (id, online) = (d.id.clone(), d.online);
        let button = match cast {
            // Connecting can take a while (mDNS, adb connect); Cancel ends it like Stop.
            CastState::Casting { .. } | CastState::Connecting { .. } | CastState::Installing { .. } => {
                let label = if live { "Stop" } else { "Cancel" };
                pill_icon(p, "cast", IconName::Square, label, Kind::Ghost).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let id = id.clone();
                    this.model.update(cx, |m, cx| m.set_cast(false, Some(id), cx))
                }))
            }
            ref other => {
                let (icon, label, kind) = if matches!(other, CastState::Idle) {
                    (IconName::Cast, "Start", Kind::Secondary)
                } else {
                    (IconName::RotateCcw, "Try again", Kind::Secondary)
                };
                enabled(pill_icon(p, "cast", icon, label, kind), online).when(online, |b| {
                    b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        let id = id.clone();
                        this.model.update(cx, |m, cx| m.set_cast(true, Some(id), cx))
                    }))
                })
            }
        };
        let row = feature_row(p, IconName::Cast, "Cast screen", detail, live, button);
        let detail = match &cast {
            CastState::NeedsWifiDebugging { .. } => {
                "Wireless debugging is off. The phone shows a notification that opens the setting; or plug it in over USB.".to_string()
            }
            CastState::NeedsPairing { .. } => {
                "Pair once: Developer options → Wireless debugging → Pair device with pairing code. Type the code here.".to_string()
            }
            CastState::Failed { error, .. } => format!("Couldn't cast: {error}"),
            _ => return (row, None),
        };
        let pairing = matches!(cast, CastState::NeedsPairing { .. });
        let id = d.id.clone();
        let note = row_note(p, detail).when(pairing, |r| {
            r.child(div().w(px(160.)).child(Styled::h(Input::new(&self.cast_code), px(CONTROL)))).child(
                pill_icon(p, "cast-pair", IconName::Link, "Pair", Kind::Primary).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let code = this.cast_code.read(cx).value().to_string();
                    let id = id.clone();
                    this.model.update(cx, |m, cx| m.pair_cast(id, code, cx));
                })),
            )
        });
        (row, Some(note))
    }

    /// Webcam's or Microphone's row (`kind`: "camera" | "mic"). One phone streams each at a
    /// time, so another phone's stream shows here as "in use".
    fn stream_row(&self, p: &Palette, status: &Status, d: &DeviceInfo, kind: &'static str, cx: &mut Context<Self>) -> Div {
        let (title, icon, sink, what, from) = if kind == "camera" {
            ("Webcam", IconName::Camera, CAMERA_NAME, "The phone camera as a webcam in any app.", &status.camera)
        } else {
            ("Microphone", IconName::Mic, "Dotlink Mic", "The phone mic as a microphone on this PC.", &status.mic)
        };
        let starting = from.is_none() && self.model.read(cx).is_starting(kind, &d.id);
        let live = from.as_deref() == Some(d.name.as_str());
        let detail = match from {
            Some(_) if live => state(&format!("LIVE · {}", sink.to_uppercase()), p.positive),
            Some(name) => state(&format!("IN USE BY {}", name.to_uppercase()), p.ink_muted),
            None if starting => state("STARTING… CHECK THE PHONE", p.ink),
            None => describe(p, what),
        };
        let id = d.id.clone();
        let button = if live {
            pill_icon(p, SharedString::from(format!("stop-{kind}")), IconName::Square, "Stop", Kind::Ghost)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.model.update(cx, |m, cx| m.set_stream(kind, false, id.clone(), cx))))
        } else if starting {
            enabled(pill(p, SharedString::from(format!("start-{kind}")), "Starting…", Kind::Secondary), false)
        } else {
            let usable = d.online && from.is_none();
            enabled(pill_icon(p, SharedString::from(format!("start-{kind}")), IconName::Play, "Start", Kind::Secondary), usable).when(usable, |b| {
                b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.model.update(cx, |m, cx| m.set_stream(kind, true, id.clone(), cx))))
            })
        };
        feature_row(p, icon, title, detail, live, button)
    }

    /// macOS: the webcam reaches apps through OBS, which shows this link (platform/macos/camera.rs).
    #[cfg(target_os = "macos")]
    fn obs_note(&self, p: &Palette, cx: &mut Context<Self>) -> Option<Div> {
        let port = self.model.read(cx).status.as_ref().map_or(dotlink_core::config::DEFAULT_PORT, |s| s.port);
        Some(
            row(p)
                .child(div().w(px(18.)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(13.))
                        .text_color(p.ink_muted)
                        .child("The webcam reaches apps through OBS: add this link as a Browser source, then Start Virtual Camera."),
                )
                .child(pill_icon(p, "copy-obs-link", IconName::Copy, "Copy OBS link", Kind::Ghost).on_click(cx.listener(
                    move |this, _: &ClickEvent, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(platform::camera_url(port)));
                        this.model.update(cx, |m, cx| m.notice("Copied: paste it as the URL of a Browser source in OBS", false, cx));
                    },
                ))),
        )
    }

    #[cfg(not(target_os = "macos"))]
    fn obs_note(&self, _p: &Palette, _cx: &mut Context<Self>) -> Option<Div> {
        None
    }

    /// Nothing paired or linked yet.
    fn no_devices(&self, p: &Palette, cx: &mut Context<Self>) -> Div {
        page(p, "Devices", None).child(
            group(p)
                .child(
                    row(p)
                        .child(Icon::new(IconName::Smartphone).size(px(18.)).text_color(p.ink_muted))
                        .child(div().flex_1().min_w_0().text_color(p.ink_muted).child("Pair your phone to send files, share the clipboard and use it as a webcam."))
                        .child(pill_icon(p, "pair-empty", IconName::ScanLine, "Pair a phone", Kind::Primary).on_click(cx.listener(
                            |this, _: &ClickEvent, _, cx| {
                                this.pair_tab = 0;
                                this.go(Page::Pair, cx)
                            },
                        ))),
                )
                .child(
                    row(p)
                        .border_t_1()
                        .border_color(p.line)
                        .child(Icon::new(IconName::Monitor).size(px(18.)).text_color(p.ink_muted))
                        .child(div().flex_1().min_w_0().text_color(p.ink_muted).child("Another PC with Dotlink on this network?"))
                        .child(pill_icon(p, "join-empty", IconName::Link, "Join a PC", Kind::Ghost).on_click(cx.listener(
                            |this, _: &ClickEvent, _, cx| {
                                this.pair_tab = 1;
                                this.model.update(cx, |m, cx| m.refresh_nearby(cx));
                                this.go(Page::Pair, cx)
                            },
                        ))),
                ),
        )
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
                            .child(div().text_color(p.ink_muted).child("Open Dotlink on your phone, tap Pair and scan the code. Same Wi-Fi, or a USB cable."))
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
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.go(Page::Device, cx))),
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
            list = list.child(empty(p, if loading { "Looking for PCs…" } else { "No other Dotlink PC on this network" }));
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
            .child(div().text_color(p.ink_muted).child("Open Dotlink → Pair on the other PC, pick it here and type the 6-digit code it shows."))
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

    fn activity(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let transfers = model.status.as_ref().map(|s| s.transfers.clone()).unwrap_or_default();
        let mut files = group(p);
        if transfers.is_empty() {
            files = files.child(empty(p, "Nothing sent or received yet"));
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
            clips = clips.child(empty(p, "No clipboard sync since Dotlink started"));
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
        // Only this window's list: the texts themselves were never kept.
        let clear_clips = (!model.clips.is_empty()).then(|| {
            pill_icon(p, "clear-clips", IconName::Trash, "Clear", Kind::Ghost).on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.model.update(cx, |m, cx| {
                    m.clips.clear();
                    cx.notify();
                })
            }))
        });
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
            .child(section_with(p, "Clipboard", clear_clips, clips))
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
        // This PC's address and certificate fingerprint, shown under its name.
        let identity = model.status.as_ref().map(|s| {
            (s.addresses.iter().map(|a| format!("{a}:{}", s.port)).collect::<Vec<_>>().join("  "), s.fingerprint.clone())
        });
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

            ))
            .child(section(
                p,
                "This PC",
                group(p).child(field("Name", "What your phones call this PC.", &form.name, true)).when_some(
                    identity,
                    |group, (addresses, fingerprint)| {
                        let short = format!("{}…", &fingerprint[..32.min(fingerprint.len())]);
                        group.child(info_row(p, "Address", "Where phones and PCs reach it.", addresses.clone(), "copy-address", addresses, cx)).child(
                            info_row(p, "Fingerprint", "Its certificate, as paired devices know it.", short, "copy-fingerprint", fingerprint, cx),
                        )
                    },
                ),
            ))
            .child(section(
                p,
                "Files",
                group(p).child(field("Download folder", "Files from phones and PCs go here.", &form.download_dir, true)).child(
                    switch_row(
                        "ask-receive",
                        "Ask before receiving files",
                        "Files wait for Accept, once for files sent together. No answer in 2 minutes declines them.",
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

/// A sidebar entry: the selected one filled, the others muted until hovered.
fn nav_item(p: &Palette, id: impl Into<gpui::ElementId>, selected: bool) -> Stateful<Div> {
    let hover = p.surface_raised;
    let ink = p.ink;
    div()
        .id(id.into())
        .flex()
        .items_center()
        .gap(px(12.))
        .px(px(14.))
        .py(px(9.))
        .rounded(px(RADIUS))
        .cursor_pointer()
        .text_color(if selected { p.ink } else { p.ink_muted })
        .when(selected, |d| d.bg(p.nav_selected).font_weight(FontWeight::MEDIUM))
        .when(!selected, |d| d.hover(move |s| s.bg(hover).text_color(ink)))
}

/// A phone, a laptop (a PC with a battery) or a desktop PC.
fn device_icon(d: &DeviceInfo) -> IconName {
    if d.is_phone() {
        IconName::Smartphone
    } else if d.battery.is_some() {
        IconName::Laptop
    } else {
        IconName::Monitor
    }
}

/// A feature of a phone that starts and stops (Cast, Webcam, Mic): icon, title, `detail` under
/// it (what it does, or how it's going), a green dot while `live`, and its button.
fn feature_row(p: &Palette, icon: IconName, title: &str, detail: Div, live: bool, button: Stateful<Div>) -> Div {
    row(p)
        .child(Icon::new(icon).size(px(18.)).text_color(p.ink_muted))
        .child(div().flex_1().min_w_0().flex().flex_col().gap(px(2.)).child(title.to_string()).child(detail))
        .when(live, |r| r.child(status_dot(p.positive)))
        .child(button)
}

/// What a feature does, under its title.
fn describe(p: &Palette, text: &str) -> Div {
    div().text_size(px(13.)).text_color(p.ink_muted).child(text.to_string())
}

/// How a feature is going, under its title (LIVE · USB, CONNECTING…).
fn state(text: &str, color: Hsla) -> Div {
    div().font_family(MONO).text_size(px(12.)).text_color(color).child(text.to_string())
}

/// A row under a feature saying what it needs, lined up with the titles above.
fn row_note(p: &Palette, text: String) -> Div {
    row(p).child(div().w(px(18.))).child(div().flex_1().min_w_0().text_size(px(13.)).text_color(p.ink_muted).child(text))
}

/// Greys out a pill that can't be used right now.
fn enabled(button: Stateful<Div>, on: bool) -> Stateful<Div> {
    if on { button } else { button.opacity(0.4).cursor_default() }
}

/// A page's title in the display face, like DOTLINK in the sidebar.
fn page_title(title: &str) -> Div {
    div().font_family(DISPLAY).font_weight(FontWeight::BLACK).text_size(px(32.)).line_height(px(36.)).child(title.to_uppercase())
}

fn page(p: &Palette, title: &str, subtitle: Option<&str>) -> Div {
    div().flex().flex_col().gap(px(24.)).child(
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(page_title(title))
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

/// A read-only Settings row: title and detail, a monospaced value and Copy (copies `copy`).
fn info_row(
    p: &Palette,
    title: &'static str,
    detail: &'static str,
    value: String,
    id: &'static str,
    copy: String,
    cx: &mut Context<MainView>,
) -> Div {
    row(p)
        .border_t_1()
        .border_color(p.line)
        .child(
            div()
                .w(px(236.))
                .flex()
                .flex_col()
                .child(title)
                .child(div().text_size(px(13.)).text_color(p.ink_muted).child(detail)),
        )
        .child(div().flex_1().min_w_0().font_family(MONO).text_size(px(14.)).truncate().child(value))
        .child(pill_icon(p, id, IconName::Copy, "Copy", Kind::Ghost).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy.clone()));
            this.model.update(cx, |m, cx| m.notice(format!("{title} copied."), false, cx));
        })))
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
        .w(px(18.))
        .flex()
        .justify_center()
        .font_family(MONO)
        .text_size(px(16.))
        .text_color(if to_phone { p.ink } else { p.positive })
        .child(if to_phone { "↑" } else { "↓" })
}

/// Nothing to list yet: a faint dot meter and a mono line, in place of the rows.
fn empty(p: &Palette, text: &str) -> Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(12.))
        .py(px(28.))
        .child(dot_meter(p, 5, 0))
        .child(mono_label(p, text.to_uppercase()))
}

fn status_dot(color: Hsla) -> Div {
    div().flex_none().size(px(8.)).rounded(px(4.)).bg(color)
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
