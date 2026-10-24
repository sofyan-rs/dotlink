//! The title bar Dotlink draws where the compositor leaves it to the app (GNOME on Wayland):
//! libadwaita's look (round grey buttons, no red close) with the buttons GNOME's
//! `button-layout` setting asks for, on the side it puts them. gpui-component's `TitleBar`
//! always shows minimize, maximize and close on the right.

use super::theme::Palette;
use gpui::prelude::*;
use gpui::{
    App, ClickEvent, Hsla, MouseButton, Window, WindowButton, WindowButtonLayout, WindowControlArea, div, px,
};
use gpui_component::{Icon, IconName, InteractiveElementExt as _};

/// libadwaita's header bar height.
pub const HEIGHT: f32 = 46.;
/// Each button's click target; the round background inside it is [BUTTON_DOT].
const BUTTON: f32 = 34.;
const BUTTON_DOT: f32 = 24.;

#[derive(IntoElement)]
pub struct TitleBar {
    palette: Palette,
}

impl TitleBar {
    pub fn new(palette: Palette) -> Self {
        Self { palette }
    }
}

/// Whether the press that may start a window move is still down (see `render`).
struct DragState {
    pressed: bool,
}

impl RenderOnce for TitleBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = self.palette;
        let layout = cx.button_layout().unwrap_or_else(WindowButtonLayout::linux_default);
        // What the compositor can do: a tiling one may offer neither minimize nor maximize.
        let supported = window.window_controls();
        let shown = move |b: &WindowButton| match b {
            WindowButton::Minimize => supported.minimize,
            WindowButton::Maximize => supported.maximize,
            WindowButton::Close => true,
        };
        let side = |buttons: [Option<WindowButton>; 3], window: &Window| {
            div().flex().items_center().px(px(6.)).children(
                buttons.into_iter().flatten().filter(shown).map(|b| control(b, window.is_maximized(), &p)),
            )
        };
        let state = window.use_state(cx, |_, _| DragState { pressed: false });

        div()
            .id("title-bar")
            .flex_shrink_0()
            .h(px(HEIGHT))
            .flex()
            .items_center()
            .bg(p.bg)
            .border_b_1()
            .border_color(p.line)
            .child(side(layout.left, window))
            .child(
                div()
                    .id("title-bar-drag")
                    .flex_1()
                    .h_full()
                    .window_control_area(WindowControlArea::Drag)
                    .on_double_click(|_, window, _| window.zoom_window())
                    // Start the move on the first motion, not on the press: moving on press
                    // hands the pointer to the compositor and a double click never arrives.
                    .on_mouse_down(MouseButton::Left, window.listener_for(&state, |s, _, _, _| s.pressed = true))
                    .on_mouse_up(MouseButton::Left, window.listener_for(&state, |s, _, _, _| s.pressed = false))
                    .on_mouse_down_out(window.listener_for(&state, |s, _, _, _| s.pressed = false))
                    .on_mouse_move(window.listener_for(&state, |s, _, window, _| {
                        if s.pressed {
                            s.pressed = false;
                            window.start_window_move();
                        }
                    }))
                    // GNOME's window menu (Always on top, Move to workspace…), as on its own apps.
                    .on_mouse_down(MouseButton::Right, |ev, window, _| window.show_window_menu(ev.position)),
            )
            .child(side(layout.right, window))
    }
}

/// One window button, drawn like libadwaita's: a 24 px grey circle, darker on hover.
fn control(button: WindowButton, maximized: bool, p: &Palette) -> impl IntoElement {
    let (icon, label) = match button {
        WindowButton::Minimize => (IconName::WindowMinimize, "Minimize"),
        WindowButton::Maximize if maximized => (IconName::WindowRestore, "Restore"),
        WindowButton::Maximize => (IconName::WindowMaximize, "Maximize"),
        WindowButton::Close => (IconName::WindowClose, "Close"),
    };
    let tint = |alpha: f32| Hsla { a: alpha, ..p.ink };
    let (rest, hover, pressed) = (tint(0.10), tint(0.15), tint(0.30));
    div()
        .id(button.id())
        .size(px(BUTTON))
        .flex()
        .items_center()
        .justify_center()
        .group(label)
        // A press here must not reach the drag area behind it.
        .on_mouse_down(MouseButton::Left, |_, window, cx| {
            window.prevent_default();
            cx.stop_propagation();
        })
        .on_click(move |_: &ClickEvent, window, cx| {
            cx.stop_propagation();
            match button {
                WindowButton::Minimize => window.minimize_window(),
                WindowButton::Maximize => window.zoom_window(),
                // Closing the window keeps Dotlink running in the tray (gui/mod.rs).
                WindowButton::Close => window.remove_window(),
            }
        })
        .child(
            div()
                .id(label)
                .size(px(BUTTON_DOT))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(rest)
                .group_hover(label, move |s| s.bg(hover))
                .group_active(label, move |s| s.bg(pressed))
                .text_color(p.ink)
                .child(Icon::new(icon).size(px(14.))),
        )
}
