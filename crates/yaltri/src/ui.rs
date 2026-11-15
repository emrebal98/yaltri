//! Shared building blocks drawn from the design spec: icons, the brand mark,
//! key caps, switches, buttons, filter chips, our window titlebar, context
//! menus, toasts and dialogs. Each takes the active `Palette`.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Bounds, Div, ElementId, FontWeight, Hsla, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _, Styled, Svg, TitlebarOptions,
    WindowBackgroundAppearance, WindowBounds, WindowControlArea, WindowDecorations, WindowOptions, div, px, size, svg,
};

use crate::theme::{self, Palette, R_LG, R_MD, R_SM, R_XS};

/// Windows: has the system round the popup's corners (with its shadow) and
/// draw its border in our `border` color. No-op elsewhere.
pub fn frame_popup(window: &gpui_kit::Window, p: &Palette) {
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = HasWindowHandle::window_handle(window)
            && let RawWindowHandle::Win32(h) = handle.as_raw()
        {
            let c = gpui_kit::Rgba::from(p.border);
            let rgb = |v: f32| (v * 255.).round() as u32;
            yaltri_platform::screen::round_popup(h.hwnd.get(), (rgb(c.r) << 16) | (rgb(c.g) << 8) | rgb(c.b));
        }
    }
    #[cfg(not(windows))]
    let _ = (window, p);
}

/// The X11 window id behind `window`; `None` on Wayland and Windows.
#[cfg(target_os = "linux")]
pub fn x11_window(window: &gpui_kit::Window) -> Option<u32> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Xlib(h) => u32::try_from(h.window).ok(),
        RawWindowHandle::Xcb(h) => Some(h.window.get()),
        _ => None,
    }
}

/// X11: gives a popup keyboard focus. GPUI maps popups override-redirect,
/// which the window manager never focuses. No-op elsewhere.
pub fn focus_popup(window: &gpui_kit::Window) {
    #[cfg(target_os = "linux")]
    if let Some(xid) = x11_window(window) {
        yaltri_platform::x11::focus_window(xid);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = window;
}

/// X11: clips the window to our rounded frame (`radius` logical px), so the
/// corners stay see-through when the GPU driver cannot draw transparency
/// (NVIDIA). Cheap to call every frame: reshapes only when the size changes.
/// No-op elsewhere.
pub fn round_corners(window: &gpui_kit::Window, radius: f32) {
    #[cfg(target_os = "linux")]
    if let Some(xid) = x11_window(window) {
        use std::cell::RefCell;
        use std::collections::HashMap;
        thread_local! {
            static SHAPED: RefCell<HashMap<u32, (u16, u16, u16)>> = RefCell::default();
        }
        let scale = window.scale_factor();
        let size = window.bounds().size;
        let physical = |v: f32| (v * scale).round() as u16;
        let shape = (physical(size.width.as_f32()), physical(size.height.as_f32()), physical(radius));
        if SHAPED.with_borrow(|m| m.get(&xid) == Some(&shape)) {
            return;
        }
        match yaltri_platform::x11::round_window(xid, shape.0, shape.1, shape.2) {
            Ok(()) => SHAPED.with_borrow_mut(|m| _ = m.insert(xid, shape)),
            Err(e) => tracing::warn!("rounding window corners failed: {e:#}"),
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (window, radius);
}

/// Radius for whatever touches a corner of a tool window's frame (inside its
/// 1 px border). GPUI clips children to a rectangle, not to rounded corners,
/// so every element that paints a background into a corner must round that
/// corner itself or it shows as a square nub. Windows: the system frame.
pub const FRAME_INNER_RADIUS: f32 = if cfg!(windows) { 0. } else { theme::R_WINDOW - 1. };

/// Height of our titlebar.
pub const TITLEBAR_HEIGHT: f32 = 40.;

/// A Lucide icon from `assets/icons`, tinted `color`.
pub fn icon(name: &str, size_px: f32, color: Hsla) -> Svg {
    svg().path(format!("icons/{name}.svg")).size(px(size_px)).flex_none().text_color(color)
}

/// The keycap-glint mark in two colors.
pub fn mark(size_px: f32, key: Hsla, glint: Hsla) -> Div {
    let layer = |path: &'static str, color: Hsla| {
        svg().path(path).absolute().top_0().left_0().size(px(size_px)).text_color(color)
    };
    div()
        .relative()
        .size(px(size_px))
        .flex_none()
        .child(layer("brand/mark-key.svg", key))
        .child(layer("brand/mark-glint.svg", glint))
}

/// A key cap: 24 px in the shortcut field, 20/18 px in hints.
pub fn keycap(content: impl IntoElement, height: f32, p: &Palette) -> Div {
    let font = if height >= 24. { 12. } else { 11. };
    div()
        .h(px(height))
        .min_w(px(height))
        .px(px(6.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(R_XS))
        .border_1()
        .border_b_2()
        .border_color(p.border)
        .bg(p.surface_raised)
        .shadow(p.shadow_sm.clone())
        .text_color(p.foreground)
        .text_size(px(font))
        .font_weight(FontWeight::MEDIUM)
        .child(content)
}

/// Shortcut spec as caps joined by small "+" signs.
pub fn keycaps(parts: Vec<String>, height: f32, p: &Palette) -> Vec<gpui_kit::AnyElement> {
    let mut out = Vec::new();
    for (ix, part) in parts.into_iter().enumerate() {
        if ix > 0 {
            out.push(div().text_size(px(11.)).text_color(p.muted).child("+").into_any_element());
        }
        out.push(keycap(part, height, p).into_any_element());
    }
    out
}

/// 40×20 toggle. The caller adds `.on_click`.
pub fn switch(id: impl Into<ElementId>, on: bool, p: &Palette) -> Stateful<Div> {
    div()
        .id(id)
        .w(px(40.))
        .h(px(20.))
        .flex_none()
        .flex()
        .items_center()
        .px(px(3.))
        .rounded(px(10.))
        .cursor_pointer()
        .when(on, |d| d.justify_end().bg(p.accent).child(div().size(px(14.)).rounded(px(7.)).bg(p.accent_fg)))
        .when(!on, |d| {
            d.border_1().border_color(p.border_strong).child(div().size(px(12.)).rounded(px(6.)).bg(p.muted))
        })
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Secondary,
    Danger,
}

/// 32 px button; the caller adds `.on_click`.
pub fn button(id: impl Into<ElementId>, kind: ButtonKind, p: &Palette) -> Stateful<Div> {
    let base = div()
        .id(id)
        .h(px(32.))
        .px(px(12.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .rounded(px(R_SM))
        .text_size(px(13.))
        .cursor_pointer();
    match kind {
        ButtonKind::Primary => base
            .bg(p.accent)
            .text_color(p.accent_fg)
            .font_weight(FontWeight::SEMIBOLD)
            .hover(|s| s.bg(theme::mix(p.accent, p.foreground, 0.08))),
        ButtonKind::Secondary => base
            .bg(p.surface_raised)
            .border_1()
            .border_color(p.border)
            .text_color(p.foreground)
            .font_weight(FontWeight::MEDIUM)
            .hover(|s| s.border_color(p.border_strong)),
        ButtonKind::Danger => base
            .bg(if p.dark { theme::hex("#C42B2B") } else { p.danger })
            .text_color(gpui_kit::white())
            .font_weight(FontWeight::SEMIBOLD)
            .hover(|s| s.opacity(0.9)),
    }
}

/// Track of a segmented control (32 px); add `segment`s.
pub fn segmented_track(p: &Palette) -> Div {
    div()
        .h(px(32.))
        .p(px(2.))
        .flex()
        .flex_none()
        .gap(px(2.))
        .rounded(px(R_SM))
        .bg(p.surface_raised)
        .border_1()
        .border_color(p.border)
}

/// One option of a segmented control; the caller adds `.on_click`.
pub fn segment(id: impl Into<ElementId>, icon_name: &str, label: impl Into<SharedString>, active: bool, p: &Palette) -> Stateful<Div> {
    let color = if active { p.foreground } else { p.muted };
    div()
        .id(id)
        .h(px(26.))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded(px(R_XS))
        .border_1()
        .text_size(px(13.))
        .cursor_pointer()
        .when(active, |d| {
            d.bg(p.surface).border_color(p.border).shadow(p.shadow_sm.clone()).font_weight(FontWeight::SEMIBOLD)
        })
        .when(!active, |d| d.border_color(gpui_kit::transparent_black()).font_weight(FontWeight::MEDIUM))
        .text_color(color)
        .hover(|s| s.text_color(p.foreground))
        .child(icon(icon_name, 14., color))
        .child(label.into())
}

/// Launcher filter chip.
pub fn chip(id: impl Into<ElementId>, label: impl Into<SharedString>, active: bool, p: &Palette) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(28.))
        .px(px(12.))
        .flex()
        .flex_none()
        .items_center()
        .rounded(px(14.))
        .text_size(px(13.))
        .cursor_pointer()
        .when(active, |c| c.bg(p.accent).text_color(p.accent_fg).font_weight(FontWeight::SEMIBOLD))
        .when(!active, |c| c.text_color(p.muted).font_weight(FontWeight::MEDIUM).hover(|s| s.bg(p.list_hover)))
        .child(label.into())
}

/// Window options for our frameless tool windows (settings, color editor).
pub fn frameless_window(title: SharedString, width: f32, height: f32, cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(width), px(height)), cx))),
        titlebar: Some(TitlebarOptions { title: Some(title), appears_transparent: true, traffic_light_position: None }),
        window_decorations: Some(WindowDecorations::Client),
        // Linux: the component library draws the rounded frame and shadow on
        // a transparent window; Windows keeps the system frame (rounded
        // corners, shadow, snap).
        window_background: if cfg!(windows) {
            WindowBackgroundAppearance::Opaque
        } else {
            WindowBackgroundAppearance::Transparent
        },
        is_resizable: false,
        app_id: Some("yaltri".into()),
        icon: app_icon(),
        ..Default::default()
    }
}

/// Taskbar / dock icon (X11 reads it from the window; others ignore it).
fn app_icon() -> Option<std::sync::Arc<image::RgbaImage>> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    const SIZE: u32 = 128;
    let rgba = yaltri_platform::icon::svg_rgba(include_str!("../../../assets/brand/app-icon.svg"), SIZE, None)?;
    image::RgbaImage::from_raw(SIZE, SIZE, rgba).map(std::sync::Arc::new)
}

/// The frame every tool window draws: our titlebar over `content`.
pub fn window_frame(title: SharedString, p: &Palette, content: impl IntoElement) -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(p.background)
        .text_color(p.foreground)
        .font_family(theme::ui_font())
        .rounded(px(FRAME_INNER_RADIUS))
        .child(titlebar(title, p))
        .child(content)
}

/// 40 px bar: mark + title, minimize and close. The whole bar drags the
/// window; Windows handles the buttons as native caption buttons.
pub fn titlebar(title: SharedString, p: &Palette) -> Stateful<Div> {
    let close_hover = theme::hex("#C42B2B");
    // Plain client-side buttons: the native caption-button hit areas
    // missed clicks. Only the title part drags the window.
    let caption = |id: &'static str, name: &'static str| {
        div()
            .id(id)
            .size(px(28.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(R_SM))
            .cursor_pointer()
            .text_color(p.muted)
            .child(svg().path(format!("icons/{name}.svg")).size(px(14.)).text_color(p.muted))
    };
    div()
        .id("titlebar")
        .h(px(TITLEBAR_HEIGHT))
        .flex_none()
        .flex()
        .items_center()
        .pr(px(6.))
        .rounded_t(px(FRAME_INNER_RADIUS))
        .bg(p.background)
        .child(
            div()
                .id("titlebar-drag")
                .flex_1()
                .min_w_0()
                .h_full()
                .pl(px(12.))
                .flex()
                .items_center()
                .gap(px(8.))
                .window_control_area(WindowControlArea::Drag)
                .on_mouse_down(MouseButton::Left, |_, window, _| {
                    if !cfg!(windows) {
                        window.start_window_move();
                    }
                })
                .child(mark(16., p.foreground, p.accent))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .font_weight(FontWeight::MEDIUM)
                        .child(title),
                ),
        )
        .child(
            div()
                .flex()
                .gap(px(2.))
                .child(
                    caption("minimize", "minus")
                        .hover(|s| s.bg(p.list_hover))
                        .on_click(|_, window, _| window.minimize_window()),
                )
                .child(
                    caption("close", "x")
                        .hover(move |s| s.bg(close_hover))
                        .on_click(|_, window, _| window.remove_window()),
                ),
        )
}

/// Records an element's window bounds while it is painted (add it as a child
/// of the element). Dropdowns use this to attach to their trigger.
pub fn track_bounds<V: 'static>(
    view: gpui_kit::WeakEntity<V>,
    store: impl Fn(&mut V, gpui_kit::Bounds<gpui_kit::Pixels>) + 'static,
) -> impl IntoElement {
    gpui_kit::canvas(
        move |bounds, _, cx| {
            let _ = view.update(cx, |view, _| store(view, bounds));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// A dropdown list right under `trigger`, left-aligned with it. It moves
/// above the trigger when it does not fit below, and always stays 8 px
/// inside the window.
pub fn dropdown(trigger: gpui_kit::Bounds<gpui_kit::Pixels>, body: impl IntoElement) -> impl IntoElement {
    gpui_kit::deferred(
        gpui_kit::anchored()
            .attach(trigger, gpui_kit::Anchor::BottomLeft)
            .anchor(gpui_kit::Anchor::TopLeft)
            .offset(gpui_kit::point(px(0.), px(4.)))
            .snap_to_window_with_margin(px(8.))
            .child(body),
    )
    .with_priority(1)
}

/// A dropdown's list box: as wide as its trigger at least, scrolling inside
/// itself when the options do not fit.
pub fn dropdown_list(id: impl Into<ElementId>, trigger: gpui_kit::Bounds<gpui_kit::Pixels>, p: &Palette) -> Stateful<Div> {
    menu_box(p).id(id).min_w(trigger.size.width).max_h(px(320.)).overflow_y_scroll()
}

/// Context menu box; add `menu_item`s and `menu_separator`s.
pub fn menu_box(p: &Palette) -> Div {
    div()
        .min_w(px(200.))
        .p(px(4.))
        .flex()
        .flex_col()
        .rounded(px(R_MD))
        .bg(p.surface)
        .border_1()
        .border_color(p.border)
        .shadow(p.shadow_md.clone())
        .text_color(p.foreground)
        // Clicks on the menu must not reach what is behind it.
        .occlude()
}

pub fn menu_item(
    id: impl Into<ElementId>,
    icon_name: &str,
    label: impl Into<SharedString>,
    danger: bool,
    p: &Palette,
) -> Stateful<Div> {
    let color = if danger { p.danger } else { p.foreground };
    div()
        .id(id)
        .h(px(32.))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(10.))
        .rounded(px(R_XS))
        .cursor_pointer()
        .text_size(px(13.))
        .text_color(color)
        .hover(|s| s.bg(p.list_hover))
        .child(icon(icon_name, 16., if danger { p.danger } else { p.muted }))
        .child(div().flex_1().truncate().child(label.into()))
}

pub fn menu_separator(p: &Palette) -> Div {
    div().h(px(1.)).my(px(4.)).bg(p.border)
}

/// "Copied" style toast: check + label + monospace value.
pub fn toast(label: impl Into<SharedString>, value: Option<SharedString>, p: &Palette) -> Div {
    div()
        .h(px(32.))
        .px(px(12.))
        .flex()
        .items_center()
        .gap(px(8.))
        .rounded(px(R_MD))
        .bg(p.toast_bg)
        .text_color(p.toast_fg)
        .shadow(p.shadow_md.clone())
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .whitespace_nowrap()
        .child(icon("check", 14., p.toast_fg))
        .child(label.into())
        .when_some(value, |d, v| {
            d.child(div().font_family(theme::mono_font()).text_size(px(12.)).opacity(0.7).child(v))
        })
}

/// Dialog box for confirmations (320 wide).
pub fn dialog_box(p: &Palette) -> Div {
    div()
        .w(px(320.))
        .p(px(20.))
        .flex()
        .flex_col()
        .gap(px(8.))
        .rounded(px(R_LG))
        .bg(p.surface)
        .border_1()
        .border_color(p.border)
        .shadow(p.shadow_lg.clone())
}

/// Text size and line height in one call: `.ts(13., 18.)`.
pub trait TextStyle: Styled + Sized {
    fn ts(self, size_px: f32, line: f32) -> Self {
        self.text_size(px(size_px)).line_height(px(line))
    }
}

impl<T: Styled> TextStyle for T {}
