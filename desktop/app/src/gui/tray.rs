//! Tray icon. Its menu sends [TrayAction]s to the window.
//!
//! Linux: StatusNotifierItem over D-Bus (KDE, Waybar on Hyprland/Sway, GNOME with the
//! AppIndicator extension). Without a tray host the app still runs; launching Dotlink again
//! brings the window back.
//!
//! Windows: the notification area, through `tray-icon`. It must be created on the main thread,
//! whose message loop (GPUI's) then drives it.
//!
//! The icon is the logo's dots alone, in one colour, like the system's own tray icons: the
//! panel recolours it where it can (Linux, a symbolic SVG), or it is drawn white on a dark
//! taskbar and black on a light one (Windows), or a template image (macOS).
//!
//! macOS: a menu bar item through `tray-icon`, created once GPUI has started NSApplication. Its
//! picture is a template image, which the menu bar draws in its own colour.

use futures::channel::mpsc::UnboundedSender;

#[derive(Debug, Clone, Copy)]
pub enum TrayAction {
    Open,
    Pair,
    SendFiles,
    SendClipboard,
    SendLink,
    Cast,
    Quit,
}

/// The logo's dots on its 5×5 grid (column, row), as in assets/dotlink.svg; the red one is
/// (2, 2), plain here.
const DOTS: [(u8, u8); 15] =
    [(0, 0), (1, 0), (2, 0), (0, 1), (2, 1), (0, 2), (1, 2), (2, 2), (3, 2), (4, 2), (2, 3), (4, 3), (2, 4), (3, 4), (4, 4)];

/// The dots in one colour on a transparent square of `size` pixels, as RGBA. In the logo a dot's
/// diameter is 3/4 of the grid step. The grid takes 3/4 of the square, like the padding of
/// other tray icons; edge to edge it looked too big next to them.
fn glyph(size: u32, [r, g, b]: [u8; 3]) -> Vec<u8> {
    let s = size as f32;
    let margin = s / 8.;
    // Five dots of diameter 0.75 step, four gaps of 0.25 step: 4.75 steps across.
    let step = (s - 2. * margin) / 4.75;
    let radius = step * 0.375;
    let mut out = vec![0u8; (size * size * 4) as usize];
    // 4×4 samples per pixel for smooth edges.
    const N: u32 = 4;
    for y in 0..size {
        for x in 0..size {
            let mut hits = 0;
            for sy in 0..N {
                for sx in 0..N {
                    let px = x as f32 + (sx as f32 + 0.5) / N as f32;
                    let py = y as f32 + (sy as f32 + 0.5) / N as f32;
                    hits += DOTS.iter().any(|&(c, rw)| {
                        let cx = margin + radius + c as f32 * step;
                        let cy = margin + radius + rw as f32 * step;
                        (px - cx).powi(2) + (py - cy).powi(2) <= radius * radius
                    }) as u32;
                }
            }
            let i = ((y * size + x) * 4) as usize;
            out[i..i + 4].copy_from_slice(&[r, g, b, (hits * 255 / (N * N)) as u8]);
        }
    }
    out
}

/// The dots as a symbolic SVG: KDE recolours `ColorScheme-Text`, GNOME and GTK panels recolour
/// any icon whose name ends in `-symbolic`.
#[cfg(target_os = "linux")]
fn symbolic_svg() -> String {
    let circles: String = DOTS
        .iter()
        .map(|&(c, r)| {
            format!(
                // Grid step 4, dot diameter 3 (3/4 of the step, as in the logo).
                "<circle class=\"ColorScheme-Text\" fill=\"currentColor\" cx=\"{}\" cy=\"{}\" r=\"1.5\"/>",
                1.5 + c as f32 * 4.,
                1.5 + r as f32 * 4.
            )
        })
        .collect();
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"-3.2 -3.2 25.4 25.4\" width=\"16\" height=\"16\">\
         <style id=\"current-color-scheme\">.ColorScheme-Text{{color:#dfdfdf}}</style>{circles}</svg>"
    )
}

/// Writes the symbolic icon into a private folder the tray host is pointed at (IconThemePath),
/// both loose and in a hicolor layout, as hosts look in either. Returns the folder and the
/// icon's name, or None when it can't be written (the pixmap is used then).
///
/// The name carries a hash of the picture: GNOME Shell caches icons by name, and kept showing
/// an old picture under the same name after it changed.
#[cfg(target_os = "linux")]
fn write_icon() -> Option<(String, String)> {
    let dir = dotlink_core::config::runtime_dir().join("dotlink-icons");
    let svg = symbolic_svg();
    // FNV-1a: stable across builds, unlike std's hasher.
    let hash = svg.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
    let name = format!("dotlink-tray-{:08x}-symbolic", hash as u32);
    let apps = dir.join("hicolor/scalable/apps");
    // Earlier pictures go; only this one is left.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&apps).ok()?;
    std::fs::write(dir.join(format!("{name}.svg")), &svg).ok()?;
    std::fs::write(apps.join(format!("{name}.svg")), &svg).ok()?;
    Some((dir.to_string_lossy().into_owned(), name))
}

/// Starts the tray icon; false when there is no tray on this desktop.
#[cfg(target_os = "linux")]
pub async fn start(actions: UnboundedSender<TrayAction>) -> bool {
    use ksni::TrayMethods;
    let icon = write_icon();
    let tray = DotlinkTray { actions: actions.clone(), icon: icon.clone() };
    match tray.spawn().await {
        Ok(handle) => {
            // Keep the D-Bus service alive for the app's lifetime.
            std::mem::forget(handle);
            true
        }
        Err(e) => {
            // At login Dotlink can start before the bar's tray (Hyprland's exec-once runs
            // everything at once): keep waiting, and the icon appears when the tray does.
            // Until then the app behaves as without one (false), so closing the window still
            // says Dotlink keeps running.
            tracing::info!("no tray yet ({e}); waiting for one");
            match (DotlinkTray { actions, icon }).assume_sni_available(true).spawn().await {
                Ok(handle) => std::mem::forget(handle),
                Err(e) => tracing::info!("no tray icon: {e}"),
            }
            false
        }
    }
}

/// Call on the main thread: Windows, `block_on` before GPUI starts; macOS, inside GPUI's `run`
/// (the status item needs the NSApplication GPUI makes).
#[cfg(any(windows, target_os = "macos"))]
pub async fn start(actions: UnboundedSender<TrayAction>) -> bool {
    use tray_icon::TrayIconBuilder;
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};

    let quit = if cfg!(target_os = "macos") { "Quit Dotlink" } else { "Quit" };
    let items = [
        ("open", "Open Dotlink", TrayAction::Open),
        ("send_files", "Send files…", TrayAction::SendFiles),
        ("send_clipboard", "Send clipboard", TrayAction::SendClipboard),
        ("send_link", "Open copied link", TrayAction::SendLink),
        ("cast", "Cast screen", TrayAction::Cast),
        ("pair", "Pair…", TrayAction::Pair),
        ("quit", quit, TrayAction::Quit),
    ];
    let item = |i: usize| MenuItem::with_id(items[i].0, items[i].1, true, None);
    let separator = PredefinedMenuItem::separator();
    let menu = Menu::new();
    let built = menu.append_items(&[
        &item(0),
        &separator,
        &item(1),
        &item(2),
        &item(3),
        &item(4),
        &item(5),
        &PredefinedMenuItem::separator(),
        &item(6),
    ]);
    if let Err(e) = built {
        tracing::warn!("no tray icon: {e}");
        return false;
    }

    #[cfg(windows)]
    let light = dotlink_core::platform::taskbar_is_light();
    #[cfg(windows)]
    let icon = taskbar_icon(light);
    #[cfg(target_os = "macos")]
    let icon = menu_bar_icon();
    let Some(icon) = icon else {
        tracing::warn!("no tray icon: couldn't make its picture");
        return false;
    };

    let menu_actions = actions.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if let Some((_, _, action)) = items.iter().find(|(id, _, _)| event.id == *id) {
            let _ = menu_actions.unbounded_send(*action);
        }
    }));

    let builder = TrayIconBuilder::new().with_tooltip("Dotlink").with_icon(icon).with_menu(Box::new(menu));
    // Windows: left click opens the window; the menu is on the right click. macOS: any click
    // opens the menu, as with every other menu bar item.
    #[cfg(windows)]
    let builder = {
        use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent};
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                let _ = actions.unbounded_send(TrayAction::Open);
            }
        }));
        builder.with_menu_on_left_click(false)
    };
    #[cfg(target_os = "macos")]
    let builder = {
        drop(actions);
        builder.with_icon_as_template(true)
    };

    match builder.build() {
        Ok(tray) => {
            // Removed from the notification area when dropped: kept for the app's lifetime, on
            // this (the main) thread, where follow_taskbar_theme recolours it.
            #[cfg(windows)]
            TRAY.with(|t| *t.borrow_mut() = Some((tray, light)));
            // Removed from the menu bar when dropped; it lives as long as the app.
            #[cfg(target_os = "macos")]
            std::mem::forget(tray);
            true
        }
        Err(e) => {
            tracing::warn!("no tray icon: {e}");
            false
        }
    }
}

/// The dots in black on transparent, for a template image: 44 px is the menu bar's 22 pt at
/// @2x, scaled down on @1x displays.
#[cfg(target_os = "macos")]
fn menu_bar_icon() -> Option<tray_icon::Icon> {
    tray_icon::Icon::from_rgba(glyph(44, [0, 0, 0]), 44, 44).ok()
}

// The tray icon and whether it's drawn for a light taskbar.
#[cfg(windows)]
thread_local! {
    static TRAY: std::cell::RefCell<Option<(tray_icon::TrayIcon, bool)>> = const { std::cell::RefCell::new(None) };
}

/// The dots in black for a light taskbar, white for a dark one. 32 px: Windows scales it down
/// to 16 at 100 % and uses it as is at 200 %.
#[cfg(windows)]
fn taskbar_icon(light: bool) -> Option<tray_icon::Icon> {
    let colour = if light { [0x1C, 0x1C, 0x1C] } else { [0xFF, 0xFF, 0xFF] };
    tray_icon::Icon::from_rgba(glyph(32, colour), 32, 32).ok()
}

/// Follows a switch between light and dark taskbar (checked every few seconds: cheap, and
/// the tray has no window of ours to get WM_SETTINGCHANGE on). Call on the main thread.
#[cfg(windows)]
pub fn follow_taskbar_theme(cx: &mut gpui::App) {
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        loop {
            cx.background_executor().timer(std::time::Duration::from_secs(3)).await;
            let light = dotlink_core::platform::taskbar_is_light();
            TRAY.with(|t| {
                if let Some((tray, drawn_light)) = t.borrow_mut().as_mut()
                    && *drawn_light != light
                    && let Some(icon) = taskbar_icon(light)
                    && tray.set_icon(Some(icon)).is_ok()
                {
                    *drawn_light = light;
                }
            });
        }
    })
    .detach();
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
pub async fn start(_actions: UnboundedSender<TrayAction>) -> bool {
    false
}

#[cfg(target_os = "linux")]
struct DotlinkTray {
    actions: UnboundedSender<TrayAction>,
    /// The symbolic icon's folder and name, if it could be written.
    icon: Option<(String, String)>,
}

#[cfg(target_os = "linux")]
impl DotlinkTray {
    /// A plain text entry: theme icons look mismatched next to each other, and the Windows
    /// menu has none either.
    fn item(label: &str, action: TrayAction) -> ksni::MenuItem<Self> {
        ksni::menu::StandardItem {
            label: label.into(),
            activate: Box::new(move |tray: &mut Self| {
                let _ = tray.actions.unbounded_send(action);
            }),
            ..Default::default()
        }
        .into()
    }
}

#[cfg(target_os = "linux")]
impl ksni::Tray for DotlinkTray {
    fn id(&self) -> String {
        "dotlink".into()
    }

    fn title(&self) -> String {
        "Dotlink".into()
    }

    fn icon_theme_path(&self) -> String {
        self.icon.as_ref().map(|(dir, _)| dir.clone()).unwrap_or_default()
    }

    /// The symbolic icon, which the panel colours to match itself.
    fn icon_name(&self) -> String {
        self.icon.as_ref().map(|(_, name)| name.clone()).unwrap_or_default()
    }

    /// For hosts that don't find the icon by name: white, as most panels are dark (GNOME's
    /// always is).
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        [22, 32, 48, 64]
            .into_iter()
            .map(|size| {
                // ksni wants ARGB.
                let argb = glyph(size, [0xFF, 0xFF, 0xFF]).as_chunks::<4>().0.iter().flat_map(|&[r, g, b, a]| [a, r, g, b]).collect();
                ksni::Icon { width: size as i32, height: size as i32, data: argb }
            })
            .collect()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.actions.unbounded_send(TrayAction::Open);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        vec![
            Self::item("Open Dotlink", TrayAction::Open),
            ksni::MenuItem::Separator,
            Self::item("Send files…", TrayAction::SendFiles),
            Self::item("Send clipboard", TrayAction::SendClipboard),
            Self::item("Open copied link", TrayAction::SendLink),
            Self::item("Cast screen", TrayAction::Cast),
            Self::item("Pair…", TrayAction::Pair),
            ksni::MenuItem::Separator,
            Self::item("Quit", TrayAction::Quit),
        ]
    }
}

