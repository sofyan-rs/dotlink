//! Tray icon. Its menu sends [TrayAction]s to the window.
//!
//! Linux: StatusNotifierItem over D-Bus (KDE, Waybar on Hyprland/Sway, GNOME with the
//! AppIndicator extension). Without a tray host the app still runs; launching Dotlink again
//! brings the window back.
//!
//! TODO(windows, macos): the `tray-icon` crate (notification area / menu bar). It needs to be
//! created on the main thread, inside GPUI's event loop.

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

#[cfg(not(target_os = "linux"))]
pub async fn start(_actions: UnboundedSender<TrayAction>) -> bool {
    false
}

#[cfg(target_os = "linux")]
struct DotlinkTray {
    actions: UnboundedSender<TrayAction>,
}

#[cfg(target_os = "linux")]
impl DotlinkTray {
    fn item(label: &str, icon: &str, action: TrayAction) -> ksni::MenuItem<Self> {
        ksni::menu::StandardItem {
            label: label.into(),
            icon_name: icon.into(),
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
            Self::item("Open Dotlink", "", TrayAction::Open),
            ksni::MenuItem::Separator,
            Self::item("Send files…", "document-send", TrayAction::SendFiles),
            Self::item("Send clipboard", "edit-paste", TrayAction::SendClipboard),
            Self::item("Cast screen", "video-display", TrayAction::Cast),
            Self::item("Pair a phone…", "list-add", TrayAction::Pair),
            ksni::MenuItem::Separator,
            Self::item("Quit", "application-exit", TrayAction::Quit),
        ]
    }
}
