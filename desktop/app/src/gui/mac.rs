//! macOS: the app menu, the Dock icon and notifications. Dotlink lives in the menu bar; the Dock
//! icon (and the app menu with it) shows only while the window is open, like other menu bar
//! apps. The service's notifications come here to be posted, so they can have buttons.

use super::model::Page;
use dotlink_core::platform::{self, Notify};
use gpui::{
    ActivationPolicy, App, AsyncApp, KeyBinding, Menu, MenuItem, OsAction, SystemNotification, SystemNotificationAction,
    SystemNotificationResponse, actions,
};
use gpui_component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};

actions!(dotlink, [About, OpenSettings, Hide, HideOthers, ShowAll, CloseWindow, Quit]);

/// Call once at the start of GPUI's `run`. `has_tray`: the menu bar item exists, so the Dock
/// icon can go when the window closes.
pub fn init(has_tray: bool, cx: &mut App) {
    cx.on_action(|_: &About, cx| open_page(Page::Settings, cx));
    cx.on_action(|_: &OpenSettings, cx| open_page(Page::Settings, cx));
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &CloseWindow, cx| {
        if let Some(handle) = cx.global::<super::State>().window {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    });
    cx.on_action(|_: &Quit, cx| super::quit(cx));
    cx.bind_keys([
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("alt-cmd-h", HideOthers, None),
        KeyBinding::new("cmd-w", CloseWindow, None),
        KeyBinding::new("cmd-q", Quit, None),
    ]);
    // Text fields only get ⌘C / ⌘V / ⌘Z through an Edit menu that names them.
    cx.set_menus([
        Menu::new("Dotlink").items([
            MenuItem::action("About Dotlink", About),
            MenuItem::separator(),
            MenuItem::action("Settings…", OpenSettings),
            MenuItem::separator(),
            MenuItem::action("Hide Dotlink", Hide),
            MenuItem::action("Hide Others", HideOthers),
            MenuItem::action("Show All", ShowAll),
            MenuItem::separator(),
            MenuItem::action("Quit Dotlink", Quit),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Undo", Undo, OsAction::Undo),
            MenuItem::os_action("Redo", Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", Cut, OsAction::Cut),
            MenuItem::os_action("Copy", Copy, OsAction::Copy),
            MenuItem::os_action("Paste", Paste, OsAction::Paste),
            MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
        ]),
        Menu::new("Window").items([MenuItem::action("Close Window", CloseWindow)]),
    ]);
    if !has_tray {
        tracing::info!("no menu bar item: the Dock icon stays");
    }
    notifications(cx);
}

/// Posts the service's notifications and acts on their buttons. GPUI shows them only from the
/// app bundle (macOS needs its bundle id); `cargo run` logs that they're off.
fn notifications(cx: &mut App) {
    cx.set_app_identity(super::APP_ID, "Dotlink");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Notify>();
    platform::set_notifier(tx);
    cx.spawn(async move |cx: &mut AsyncApp| {
        while let Some(n) = rx.recv().await {
            cx.update(|cx| match n {
                Notify::Show { tag, title, body, actions } => cx.show_system_notification(SystemNotification {
                    tag: tag.into(),
                    title: title.into(),
                    body: body.into(),
                    actions: actions
                        .into_iter()
                        .map(|(id, label)| SystemNotificationAction { id: id.into(), label: label.into() })
                        .collect(),
                }),
                Notify::Dismiss { tag } => cx.dismiss_system_notification(&tag),
            });
        }
    })
    .detach();
}

/// The answer to a notification's buttons. Call after `gpui_component::init`: GPUI keeps one
/// handler, and gpui-component registers its own (for notifications it posts, which Dotlink
/// doesn't use) that would replace this one.
pub fn handle_notification_responses(cx: &mut App) {
    cx.on_system_notification_response(respond);
}

fn respond(response: SystemNotificationResponse, cx: &mut App) {
    let action = response.action_id.as_deref();
    if let Some(path) = response.tag.strip_prefix("file:") {
        let path = std::path::Path::new(path);
        match action {
            Some("open") => platform::open_path(path),
            _ => platform::show_in_folder(path),
        }
    } else if let Some(id) = response.tag.strip_prefix("incoming:") {
        match action {
            Some(answer @ ("accept" | "decline")) => {
                let model = cx.global::<super::State>().model.clone();
                model.update(cx, |m, cx| m.answer_incoming(id.to_string(), answer == "accept", cx));
            }
            // The notification itself: the window, where the banner asks too.
            _ => super::show_window(cx),
        }
    } else {
        super::show_window(cx);
    }
}

/// Clicking the Dock icon, or opening Dotlink from Finder again, brings the window back.
pub fn on_reopen(cx: &mut App) {
    if cx.has_global::<super::State>() {
        super::show_window(cx);
    }
}

/// Before the window opens: a Dock icon and the app menu.
pub fn show_dock_icon(cx: &mut App) {
    cx.set_activation_policy(ActivationPolicy::Regular);
}

/// The window closed (or was never opened): only the menu bar item is left.
pub fn hide_dock_icon(cx: &mut App) {
    cx.set_activation_policy(ActivationPolicy::Accessory);
}

fn open_page(page: Page, cx: &mut App) {
    let model = cx.global::<super::State>().model.clone();
    model.update(cx, |m, cx| {
        m.page = page;
        cx.notify();
    });
    super::show_window(cx);
}
