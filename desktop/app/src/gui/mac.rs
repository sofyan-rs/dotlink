//! macOS: the app menu, and the Dock icon. Dotlink lives in the menu bar; the Dock icon (and
//! the app menu with it) shows only while the window is open, like other menu bar apps.

use super::model::Page;
use gpui::{ActivationPolicy, App, KeyBinding, Menu, MenuItem, OsAction, actions};
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
