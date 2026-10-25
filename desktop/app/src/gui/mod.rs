//! The desktop app: the service plus a window and a tray icon, in one process.
//!
//! Launch flow:
//! 1. Dotlink already running with a window → ask it to show that window, exit.
//! 2. `dotlink daemon` already running (no window) → open a window that controls it.
//! 3. Nothing running → start the service here, then the tray and (unless `--background`)
//!    the window.
//!
//! Closing the window keeps the app (and the phone connection) running; Quit in the tray or
//! in Settings stops it.
//!
//! TODO(macos):
//! - [ ] Tray: see tray.rs.
//! - [ ] An app menu (About, Quit ⌘Q) via `cx.set_menus`, and reopening the window when
//!   the Dock icon is clicked (`cx.on_reopen`). Hide the Dock icon while only in the menu bar.
//!
//! TODO(windows, macos): packaging: .msi/NSIS (Windows), signed + notarized .app in a .dmg
//! (macOS). See the `[package.metadata.packager]` section in Cargo.toml.

mod assets;
mod client;
mod model;
mod theme;
mod title_bar;
mod tray;
mod window;

use anyhow::{Result, bail};
use client::Client;
use dotlink_core::events::UiRequest;
use dotlink_core::ipc;
use futures::StreamExt;
use gpui::{App, AppContext, AsyncApp, Bounds, Global, QuitMode, WindowBounds, WindowHandle, WindowOptions, px, size};
use gpui_component::Root;
use model::{Model, Page};
use serde_json::json;
use std::time::Duration;
use tray::TrayAction;

/// Wayland app id / X11 WM_CLASS: the desktop file is dev.dotlink.Dotlink.desktop (and says
/// StartupWMClass=dev.dotlink.Dotlink for packages that name it otherwise).
const APP_ID: &str = "dev.dotlink.Dotlink";

struct State {
    model: gpui::Entity<Model>,
    window: Option<WindowHandle<Root>>,
    owns_service: bool,
    /// The "still running" notice (no tray) was shown once.
    told_still_running: bool,
}

impl Global for State {}

pub fn run(rt: tokio::runtime::Runtime, background: bool) -> Result<()> {
    let (ui_tx, ui_rx) = tokio::sync::mpsc::unbounded_channel::<UiRequest>();
    let owns_service = rt.block_on(async {
        if !ipc::running().await {
            return Ok(true);
        }
        // Already running: started at login again, nothing to do.
        if background {
            std::process::exit(0);
        }
        // A Dotlink app with a window shows it and we're done.
        let shown = ipc::call(json!({ "cmd": "show" })).await?;
        if shown["ok"].is_string() {
            std::process::exit(0);
        }
        Ok::<_, anyhow::Error>(false)
    })?;

    // Fires when the service stops for any reason (Quit, `dotlink quit`, SIGTERM, a crash).
    let (stopped_tx, stopped_rx) = futures::channel::oneshot::channel::<()>();
    if owns_service {
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        rt.spawn(async move {
            if let Err(e) = dotlink_core::service::run(Some(ui_tx), Some(ready_tx)).await {
                // Started from the app menu there is no terminal to read this in.
                tracing::error!("{e:#}");
                dotlink_core::platform::notify("Dotlink stopped", &format!("{e:#}"));
            }
            let _ = stopped_tx.send(());
        });
        if let Err(e) = rt.block_on(ready_rx)? {
            bail!("{e}");
        }
    }

    let (tray_tx, tray_rx) = futures::channel::mpsc::unbounded::<TrayAction>();
    let has_tray = owns_service && rt.block_on(tray::start(tray_tx));
    let client = Client::new(rt.handle().clone());

    gpui_platform::application().with_assets(assets::Assets).with_quit_mode(QuitMode::Explicit).run(
        move |cx: &mut App| {
            gpui_component::init(cx);
            theme::load_fonts(cx);
            theme::apply(None, cx);
            #[cfg(target_os = "linux")]
            theme::follow_portal(cx);
            let model = Model::new(client, owns_service, cx);
            model.update(cx, |m, _| m.has_tray = has_tray);
            #[cfg(windows)]
            if has_tray {
                tray::follow_taskbar_theme(cx);
            }
            // With the window closed (tray actions), messages go to the desktop instead.
            cx.subscribe(&model, |_, notice: &model::Notice, cx| {
                if cx.global::<State>().window.is_none() {
                    dotlink_core::platform::notify("Dotlink", &notice.text);
                }
            })
            .detach();
            cx.set_global(State { model, window: None, owns_service, told_still_running: false });
            cx.on_window_closed(move |cx, _| {
                let state = cx.global_mut::<State>();
                state.window = None;
                // A window that only controls a separate daemon has nothing to do once closed.
                if !state.owns_service {
                    cx.quit();
                    return;
                }
                // No tray (GNOME without the AppIndicator extension): say where Dotlink went.
                if !has_tray && !state.told_still_running {
                    state.told_still_running = true;
                    tracing::info!("window closed without a tray; still running in the background");
                    dotlink_core::platform::notify(
                        "Dotlink is still running",
                        "Your phone stays connected. Open Dotlink again to show the window; Quit is in Settings.",
                    );
                }
                // Windows users expect closing the window to quit the app: say once that it didn't.
                if cfg!(windows) && has_tray && !state.told_still_running {
                    state.told_still_running = true;
                    dotlink_core::platform::notify(
                        "Dotlink is still running",
                        "Your phone stays connected. Dotlink is in the notification area; Quit is in its menu.",
                    );
                }
            })
            .detach();

            // Without a tray there is no way back to a hidden window, so always show it.
            if !background || !has_tray {
                show_window(cx);
            }
            listen_service(ui_rx, stopped_rx, cx);
            listen_tray(tray_rx, cx);
        },
    );
    // The service stops with the process; give it a moment to remove its socket.
    rt.shutdown_timeout(Duration::from_secs(1));
    Ok(())
}

/// Opens the window, or brings it to the front.
fn show_window(cx: &mut App) {
    let (existing, model) = {
        let state = cx.global::<State>();
        (state.window, state.model.clone())
    };
    if let Some(handle) = existing
        && handle.update(cx, |_, window, _| window.activate_window()).is_ok()
    {
        return;
    }
    let bounds = Bounds::centered(None, size(px(1040.), px(740.)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(gpui::TitlebarOptions { title: Some("Dotlink".into()), ..Default::default() }),
        app_id: Some(APP_ID.into()),
        window_min_size: Some(size(px(720.), px(520.))),
        // Where we draw our own frame (GNOME), its shadow must be see-through; every view
        // paints its own opaque background, so this changes nothing elsewhere.
        window_background: gpui::WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        theme::apply(Some(window), cx);
        let view = cx.new(|cx| window::MainView::new(model, window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    });
    match opened {
        Ok(handle) => {
            cx.global_mut::<State>().window = Some(handle);
            cx.activate(true);
        }
        Err(e) => tracing::error!("could not open the window: {e:#}"),
    }
}

/// Stops the app. When the service runs inside it, stops that too.
pub fn quit(cx: &mut App) {
    let state = cx.global::<State>();
    if !state.owns_service {
        cx.quit();
        return;
    }
    let stop = state.model.read(cx).client.quit_service();
    cx.spawn(async move |cx: &mut AsyncApp| {
        let _ = stop.await;
        cx.update(|cx| cx.quit());
    })
    .detach();
}

fn listen_service(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<UiRequest>,
    stopped: futures::channel::oneshot::Receiver<()>,
    cx: &mut App,
) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        while let Some(req) = rx.recv().await {
            match req {
                UiRequest::Show => cx.update(show_window),
            }
        }
    })
    .detach();
    // When the service inside this app stops, so does the app. (A window controlling a
    // separate daemon never gets this: the sender is dropped without sending.)
    cx.spawn(async move |cx: &mut AsyncApp| {
        if stopped.await.is_ok() {
            cx.update(|cx| cx.quit());
        }
    })
    .detach();
}

fn listen_tray(mut rx: futures::channel::mpsc::UnboundedReceiver<TrayAction>, cx: &mut App) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        while let Some(action) = rx.next().await {
            cx.update(|cx| {
                let model = cx.global::<State>().model.clone();
                match action {
                    TrayAction::Open => show_window(cx),
                    TrayAction::Pair => {
                        model.update(cx, |m, cx| {
                            m.page = Page::Pair;
                            m.start_pairing(cx);
                        });
                        show_window(cx);
                    }
                    TrayAction::SendFiles => {
                        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
                            files: true,
                            directories: false,
                            multiple: true,
                            prompt: Some("Send".into()),
                        });
                        cx.spawn(async move |cx: &mut AsyncApp| {
                            if let Ok(Ok(Some(paths))) = picked.await {
                                cx.update(|cx| model.update(cx, |m, cx| m.send_files(paths, cx)));
                            }
                        })
                        .detach();
                    }
                    TrayAction::SendClipboard => model.update(cx, |m, cx| m.send_clipboard(cx)),
                    // With several phones online, the Devices page asks which one. Needing
                    // pairing shows there too.
                    TrayAction::Cast => {
                        let online = model.read(cx).online_counts().0;
                        if online > 1 {
                            model.update(cx, |m, _| m.page = Page::Phones);
                            show_window(cx);
                        } else {
                            model.update(cx, |m, cx| m.set_cast(true, None, cx));
                        }
                    }
                    TrayAction::Quit => quit(cx),
                }
            });
        }
    })
    .detach();
}
