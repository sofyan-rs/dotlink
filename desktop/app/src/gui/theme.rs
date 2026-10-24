//! Dotline, the phone app's look: black or paper-white, one red accent, dot-matrix titles
//! (Doto), Geist for reading and Geist Mono for labels and data. Colors match the Android
//! app's ui/theme/Color.kt.

use gpui::{App, Hsla, SharedString, Window, px, rgb, rgba};
use gpui_component::{Theme, ThemeMode};
use std::borrow::Cow;
use std::sync::atomic::{AtomicU8, Ordering};

pub const DISPLAY: &str = "Doto";
pub const SANS: &str = "Geist";
pub const MONO: &str = "Geist Mono";

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Hsla,
    pub surface: Hsla,
    pub surface_raised: Hsla,
    pub line: Hsla,
    pub line_strong: Hsla,
    pub ink: Hsla,
    pub ink_muted: Hsla,
    pub ink_faint: Hsla,
    pub on_ink: Hsla,
    pub accent: Hsla,
    pub on_accent: Hsla,
    pub accent_text: Hsla,
    pub positive: Hsla,
    pub nav_selected: Hsla,
}

fn dark() -> Palette {
    Palette {
        bg: rgb(0x000000).into(),
        surface: rgb(0x111111).into(),
        surface_raised: rgb(0x1C1C1C).into(),
        line: rgba(0xFFFFFF1F).into(),
        line_strong: rgb(0x4A4A4A).into(),
        ink: rgb(0xFFFFFF).into(),
        ink_muted: rgb(0xA6A6A6).into(),
        ink_faint: rgb(0x808080).into(),
        on_ink: rgb(0x000000).into(),
        accent: rgb(0xE5211C).into(),
        on_accent: rgb(0xFFFFFF).into(),
        accent_text: rgb(0xFF5A52).into(),
        positive: rgb(0x4AD991).into(),
        nav_selected: rgb(0x3A3A3A).into(),
    }
}

fn light() -> Palette {
    Palette {
        bg: rgb(0xF2F2F2).into(),
        surface: rgb(0xFFFFFF).into(),
        surface_raised: rgb(0xE8E8E8).into(),
        line: rgba(0x0000001A).into(),
        line_strong: rgb(0xBDBDBD).into(),
        ink: rgb(0x000000).into(),
        ink_muted: rgb(0x4D4D4D).into(),
        ink_faint: rgb(0x6B6B6B).into(),
        on_ink: rgb(0xFFFFFF).into(),
        accent: rgb(0xD70015).into(),
        on_accent: rgb(0xFFFFFF).into(),
        accent_text: rgb(0xD70015).into(),
        positive: rgb(0x0B6B3C).into(),
        // The phone's white chip sits on a grey nav bar; the sidebar here is white itself.
        nav_selected: rgb(0xE2E2E2).into(),
    }
}

pub fn palette(cx: &App) -> Palette {
    if Theme::global(cx).is_dark() { dark() } else { light() }
}

/// The phone app's fonts (shared with android/, so both stay the same).
pub fn load_fonts(cx: &mut App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!("../../../../android/app/src/main/res/font/geist.ttf")),
        Cow::Borrowed(include_bytes!("../../../../android/app/src/main/res/font/geist_mono.ttf")),
        Cow::Borrowed(include_bytes!("../../../../android/app/src/main/res/font/doto.ttf")),
    ];
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        tracing::warn!("fonts: {e}");
    }
}

/// Follows the system's light/dark setting and paints gpui-component's widgets in Dotline colors.
/// Settings → Theme: 0 follows the desktop, 1 light, 2 dark (same order as the phone app).
static CHOICE: AtomicU8 = AtomicU8::new(0);

pub fn choice() -> u8 {
    CHOICE.load(Ordering::Relaxed)
}

/// `config.theme` as a [choice].
pub fn choice_from(name: &str) -> u8 {
    match name {
        "light" => 1,
        "dark" => 2,
        _ => 0,
    }
}

pub fn choice_name(choice: u8) -> &'static str {
    ["system", "light", "dark"][choice.min(2) as usize]
}

/// Sets the theme choice and repaints; no-op when it's already that.
pub fn set_choice(choice: u8, cx: &mut App) {
    if CHOICE.swap(choice, Ordering::Relaxed) != choice {
        apply(None, cx);
        cx.refresh_windows();
    }
}

pub fn apply(window: Option<&mut Window>, cx: &mut App) {
    let mode: ThemeMode = match choice() {
        1 => ThemeMode::Light,
        2 => ThemeMode::Dark,
        _ => match (portal_dark(), window.as_ref()) {
            (Some(true), _) => ThemeMode::Dark,
            (Some(false), _) => ThemeMode::Light,
            (None, Some(w)) => w.appearance().into(),
            (None, None) => cx.window_appearance().into(),
        },
    };
    Theme::change(mode, window, cx);
    let p = if mode.is_dark() { dark() } else { light() };
    Theme::update(cx, |t| {
        t.font_family = SharedString::from(SANS);
        t.mono_font_family = SharedString::from(MONO);
        t.font_size = px(15.);
        // Text fields and popovers match the window's slight corners (window.rs RADIUS).
        t.radius = px(8.);
        t.radius_lg = px(12.);
        t.background = p.bg;
        t.foreground = p.ink;
        t.border = p.line;
        t.input = p.line_strong;
        t.ring = p.ink;
        t.caret = p.ink;
        t.selection = p.accent.opacity(0.35);
        t.muted = p.surface_raised;
        t.muted_foreground = p.ink_muted;
        t.accent = p.surface_raised;
        t.accent_foreground = p.ink;
        t.primary = p.ink;
        t.primary_hover = p.ink.opacity(0.85);
        t.primary_active = p.ink.opacity(0.7);
        t.primary_foreground = p.on_ink;
        t.secondary = p.surface_raised;
        t.secondary_hover = p.nav_selected;
        t.secondary_active = p.line_strong;
        t.secondary_foreground = p.ink;
        t.danger = p.accent;
        t.danger_hover = p.accent.opacity(0.85);
        t.danger_active = p.accent.opacity(0.7);
        t.danger_foreground = p.on_accent;
        t.success = p.positive;
        t.popover = p.surface;
        t.popover_foreground = p.ink;
        t.switch = p.line_strong;
        t.switch_thumb = p.surface;
        t.progress_bar = p.ink;
        t.list_hover = p.surface_raised;
        t.drag_border = p.accent;
        // Our own title bar (GNOME): the sidebar's surface, no gradient seam.
        t.title_bar = p.surface;
        t.title_bar_border = p.line;
        t.drop_target = p.accent.opacity(0.08);
    });
}

/// The desktop's light/dark setting as [follow_portal] last read it: 0 not yet, 1 light, 2 dark.
static PORTAL: AtomicU8 = AtomicU8::new(0);

fn portal_dark() -> Option<bool> {
    match PORTAL.load(Ordering::Relaxed) {
        0 => None,
        v => Some(v == 2),
    }
}

/// Linux: gpui asks the desktop portal for light/dark once, at startup. Started at login
/// (Hyprland's exec), we can come up before the portal does; gpui then stays light for good and
/// misses later changes too. So read it here as well, retrying until the portal answers, and
/// follow its changes from then on.
#[cfg(target_os = "linux")]
pub fn follow_portal(cx: &mut App) {
    use ashpd::desktop::settings::{ColorScheme, Settings};
    use futures::StreamExt;
    use std::time::Duration;

    let (tx, mut rx) = futures::channel::mpsc::unbounded::<bool>();
    let timer = cx.background_executor().clone();
    cx.background_executor()
        .spawn(async move {
            let dark = |s: ColorScheme| s == ColorScheme::PreferDark;
            // About two minutes, then give up: gpui's own reading is all there is.
            for _ in 0..60 {
                if let Ok(settings) = Settings::new().await
                    && let Ok(scheme) = settings.color_scheme().await
                {
                    if tx.unbounded_send(dark(scheme)).is_err() {
                        return;
                    }
                    if let Ok(mut changes) = settings.receive_color_scheme_changed().await {
                        while let Some(scheme) = changes.next().await {
                            if tx.unbounded_send(dark(scheme)).is_err() {
                                return;
                            }
                        }
                    }
                    return;
                }
                timer.timer(Duration::from_secs(2)).await;
            }
            tracing::debug!("no answer from the desktop portal; light/dark is gpui's guess");
        })
        .detach();
    cx.spawn(async move |cx| {
        while let Some(dark) = rx.next().await {
            let v = if dark { 2 } else { 1 };
            if PORTAL.swap(v, Ordering::Relaxed) != v {
                cx.update(|cx| {
                    apply(None, cx);
                    cx.refresh_windows();
                });
            }
        }
    })
    .detach();
}
