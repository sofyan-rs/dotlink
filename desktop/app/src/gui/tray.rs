//! Tray icon. Its menu sends [TrayAction]s to the window.
//!
//! Linux: StatusNotifierItem over D-Bus (KDE, Waybar on Hyprland/Sway, GNOME with the
//! AppIndicator extension). Without a tray host the app still runs; launching Dotlink again
//! brings the window back.
//!
//! Windows: the notification area, through `tray-icon`. It must be created on the main thread,
//! whose message loop (GPUI's) then drives it.
//!
//! TODO(macos): `tray-icon` as a menu bar item, with a template image.

use futures::channel::mpsc::UnboundedSender;

#[derive(Debug, Clone, Copy)]
pub enum TrayAction {
    Open,
    Pair,
    SendFiles,
    SendClipboard,
    Cast,
    Quit,
}

/// Starts the tray icon; false when there is no tray on this desktop.
#[cfg(target_os = "linux")]
pub async fn start(actions: UnboundedSender<TrayAction>) -> bool {
    use ksni::TrayMethods;
    match (DotlinkTray { actions }).spawn().await {
        Ok(handle) => {
            // Keep the D-Bus service alive for the app's lifetime.
            std::mem::forget(handle);
            true
        }
        Err(e) => {
            tracing::info!("no tray icon: {e}");
            false
        }
    }
}

/// Call on the main thread (`block_on` before GPUI starts runs it there).
#[cfg(windows)]
pub async fn start(actions: UnboundedSender<TrayAction>) -> bool {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let items = [
        ("open", "Open Dotlink", TrayAction::Open),
        ("send_files", "Send files…", TrayAction::SendFiles),
        ("send_clipboard", "Send clipboard", TrayAction::SendClipboard),
        ("cast", "Cast screen", TrayAction::Cast),
        ("pair", "Pair a phone…", TrayAction::Pair),
        ("quit", "Quit", TrayAction::Quit),
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
        &PredefinedMenuItem::separator(),
        &item(5),
    ]);
    if let Err(e) = built {
        tracing::warn!("no tray icon: {e}");
        return false;
    }

    // The same picture as on Linux, ARGB to RGBA.
    let argb = include_bytes!("../../assets/tray-32.argb");
    let rgba: Vec<u8> = argb.as_chunks::<4>().0.iter().flat_map(|&[a, r, g, b]| [r, g, b, a]).collect();
    let icon = match Icon::from_rgba(rgba, 32, 32) {
        Ok(icon) => icon,
        Err(e) => {
            tracing::warn!("no tray icon: {e}");
            return false;
        }
    };

    let menu_actions = actions.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if let Some((_, _, action)) = items.iter().find(|(id, _, _)| event.id == *id) {
            let _ = menu_actions.unbounded_send(*action);
        }
    }));
    // Left click opens the window; the menu is on the right click.
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
            let _ = actions.unbounded_send(TrayAction::Open);
        }
    }));

    match TrayIconBuilder::new()
        .with_tooltip("Dotlink")
        .with_icon(icon)
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()
    {
        Ok(tray) => {
            // Removed from the notification area when dropped: keep it for the app's lifetime.
            std::mem::forget(tray);
            true
        }
        Err(e) => {
            tracing::warn!("no tray icon: {e}");
            false
        }
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
pub async fn start(_actions: UnboundedSender<TrayAction>) -> bool {
    false
}

#[cfg(target_os = "linux")]
struct DotlinkTray {
    actions: UnboundedSender<TrayAction>,
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

    /// A pixmap rather than an icon name: it shows even when the icon isn't installed
    /// (AppImage, running from the build dir).
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        [(32, &include_bytes!("../../assets/tray-32.argb")[..]), (64, &include_bytes!("../../assets/tray-64.argb")[..])]
            .into_iter()
            .map(|(size, data)| ksni::Icon { width: size, height: size, data: data.to_vec() })
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
            Self::item("Cast screen", TrayAction::Cast),
            Self::item("Pair a phone…", TrayAction::Pair),
            ksni::MenuItem::Separator,
            Self::item("Quit", TrayAction::Quit),
        ]
    }
}
