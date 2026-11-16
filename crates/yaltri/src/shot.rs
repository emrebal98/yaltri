//! Screenshots.
//!
//! Windows / X11 / GNOME: the desktop is captured once (on GNOME through the
//! screenshot portal) and every display gets a borderless topmost window
//! showing its part, dimmed. A bar at the top picks the mode: rectangle
//! (drag), window (click the outlined window) or full screen (click the
//! outlined display). Once something is selected a toolbar under it copies
//! or saves it. Esc or right click cancels.
//!
//! Other Wayland desktops: the desktop's own screenshot picker runs through
//! the portal and its result is copied and saved directly.

use std::sync::Arc;

use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, CursorStyle, InteractiveElement as _, IntoElement,
    KeystrokeEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, ParentElement as _, Pixels,
    Point, Render, RenderImage, StatefulInteractiveElement as _, Styled as _, StyledImage as _, Subscription, Window,
    WindowBounds, WindowKind, WindowOptions, div, img, px,
};
use gpui_kit::prelude::FluentBuilder as _;
use yaltri_platform::screen::{self, Screenshot};
use yaltri_platform::windows_list::{self, WinRect};

use crate::daemon::Daemon;
use crate::i18n::t;
use crate::markup::{self, Pt, Shape, Tool};
use crate::shot_output::{self, Action};
use crate::theme::{self, Palette};
use crate::ui::{self, TextStyle as _};

/// Dim over everything outside the selection.
const DIM: f32 = 0.45;
/// Drags shorter than this (logical px) are clicks, not selections.
const MIN_DRAG: f32 = 4.;
const BAR_GAP: f32 = 8.;
const BUTTON: f32 = 32.;
const SWATCH: f32 = 24.;
const WIDTH_BUTTON: f32 = 28.;
const GAP: f32 = 4.;
const TOOLBAR_H: f32 = BUTTON + 2. * 6. + 2.;

/// Tools, separator, colours, widths, separator, undo, separator, copy /
/// save / cancel.
fn toolbar_width() -> f32 {
    let tools = Tool::ALL.len() as f32 * BUTTON;
    let colors = markup::COLORS.len() as f32 * SWATCH;
    let widths = markup::WIDTHS.len() as f32 * WIDTH_BUTTON;
    let items = Tool::ALL.len() + markup::COLORS.len() + markup::WIDTHS.len() + 4 + 3;
    tools + colors + widths + 4. * BUTTON + 3. + (items as f32 - 1.) * GAP + 2. * 6. + 2.
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Rect,
    Window,
    Screen,
}

/// A rectangle in window-relative logical px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl LRect {
    /// The rectangle spanned by two corners, in any order.
    pub fn spanning(a: (f32, f32), b: (f32, f32)) -> Self {
        Self { x: a.0.min(b.0), y: a.1.min(b.1), w: (a.0 - b.0).abs(), h: (a.1 - b.1).abs() }
    }

    /// Cut to `0..width, 0..height`; `None` when nothing is left.
    pub fn clip(self, width: f32, height: f32) -> Option<Self> {
        let (x0, y0) = (self.x.max(0.), self.y.max(0.));
        let (x1, y1) = ((self.x + self.w).min(width), (self.y + self.h).min(height));
        (x1 > x0 && y1 > y0).then_some(Self { x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
    }
}

/// Top-left of a `size` box under `sel` (above when there is no room below,
/// inside the bottom edge when neither fits), kept inside the area.
pub fn toolbar_position(sel: LRect, size: (f32, f32), area: (f32, f32)) -> (f32, f32) {
    let x = (sel.x + sel.w - size.0).clamp(BAR_GAP, (area.0 - size.0 - BAR_GAP).max(BAR_GAP));
    let below = sel.y + sel.h + BAR_GAP;
    let above = sel.y - BAR_GAP - size.1;
    let y = if below + size.1 + BAR_GAP <= area.1 {
        below
    } else if above >= BAR_GAP {
        above
    } else {
        sel.y + sel.h - size.1 - BAR_GAP
    };
    (x, y)
}

/// Starts a screenshot, or cancels the open one.
pub fn toggle(cx: &mut App) {
    if !cx.global::<Daemon>().shot_windows.is_empty() {
        close(cx);
        return;
    }
    let session = cx.global::<Daemon>().session;
    match session {
        yaltri_platform::session::Session::WaylandGnome => capture_with_portal(false, cx),
        yaltri_platform::session::Session::WaylandLayerShell => capture_with_portal(true, cx),
        _ => match screen::capture() {
            Ok(shot) => open_overlays(Arc::new(shot), true, cx),
            Err(e) => tracing::error!("screen capture failed: {e:#}"),
        },
    }
}

#[cfg(target_os = "linux")]
fn capture_with_portal(interactive: bool, cx: &mut App) {
    let task = cx.background_executor().spawn(screen::capture_portal(interactive));
    cx.spawn(async move |cx| match task.await {
        Ok(Some(shot)) if interactive => cx.update(|cx| shot_output::finish(Action::Save, shot, cx)),
        Ok(Some(shot)) => cx.update(|cx| open_overlays(Arc::new(shot), false, cx)),
        Ok(None) => {}
        Err(e) => tracing::error!("portal screenshot failed: {e:#}"),
    })
    .detach();
}

#[cfg(not(target_os = "linux"))]
fn capture_with_portal(_: bool, _: &mut App) {
    tracing::error!("portal screenshots are only available on Linux");
}

fn open_overlays(shot: Arc<Screenshot>, window_mode: bool, cx: &mut App) {
    let windows = Arc::new(if window_mode { windows_list::visible_windows() } else { Vec::new() });
    let mut handles = Vec::new();
    for display in cx.displays() {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(display.bounds())),
            display_id: Some(display.id()),
            titlebar: None,
            focus: true,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            app_id: Some("yaltri-shot".into()),
            ..Default::default()
        };
        let (shot, windows) = (shot.clone(), windows.clone());
        let display_bounds = display.bounds();
        let opened = gpui_kit::open_window(options, cx, move |window, cx| {
            // Display bounds are logical; the screenshot is physical.
            let scale = window.scale_factor();
            let display_origin = (
                (display_bounds.origin.x.as_f32() * scale).round() as i32,
                (display_bounds.origin.y.as_f32() * scale).round() as i32,
            );
            let size = (
                (display_bounds.size.width.as_f32() * scale).round() as u32,
                (display_bounds.size.height.as_f32() * scale).round() as u32,
            );
            // See the picker: a popup's client area can sit off the display edge.
            let client_origin = crate::picker::client_origin(window).unwrap_or(display_origin);
            let view = cx.new(|cx| {
                ShotOverlay::new(shot, windows, window_mode, display_origin, client_origin, size, scale, window, cx)
            });
            window.activate_window();
            ui::focus_popup(window);
            view
        });
        match opened {
            Ok((handle, _)) => handles.push(handle),
            Err(e) => tracing::error!("opening screenshot window failed: {e:#}"),
        }
    }
    cx.global_mut::<Daemon>().shot_windows = handles;
    cx.activate(true);
}

/// Closes every overlay window (deferred: may be called from inside one).
pub fn close(cx: &mut App) {
    let handles: Vec<AnyWindowHandle> = std::mem::take(&mut cx.global_mut::<Daemon>().shot_windows);
    cx.defer(move |cx| {
        for h in handles {
            let _ = h.update(cx, |_, window, _| window.remove_window());
        }
    });
}

pub struct ShotOverlay {
    shot: Arc<Screenshot>,
    image: Arc<RenderImage>,
    windows: Arc<Vec<WinRect>>,
    window_mode: bool,
    /// Physical desktop position of the window's content (0,0).
    client_origin: (i32, i32),
    /// Where this display's image goes inside the window, and its size, in
    /// logical px.
    image_offset: (f32, f32),
    image_size: (f32, f32),
    scale: f32,
    mode: Mode,
    /// Where the current drag started.
    drag_from: Option<Point<Pixels>>,
    selection: Option<LRect>,
    /// What a click would select in window / full screen mode.
    hover: Option<LRect>,
    /// Drawing tool in use; while set, the selection stays put.
    tool: Option<Tool>,
    color: u32,
    width: f32,
    shapes: Vec<Shape>,
    /// The shape being dragged out or typed.
    drawing: Option<Shape>,
    typing: bool,
    /// The selection's pixels, taken when the first shape starts.
    base: Option<Screenshot>,
    /// Selection with the shapes drawn on it.
    composite: Option<Arc<RenderImage>>,
    dirty: bool,
    _keys: Subscription,
}

impl ShotOverlay {
    #[allow(clippy::too_many_arguments)]
    fn new(
        shot: Arc<Screenshot>,
        windows: Arc<Vec<WinRect>>,
        window_mode: bool,
        display_origin: (i32, i32),
        client_origin: (i32, i32),
        size: (u32, u32),
        scale: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let part = shot.crop(display_origin.0, display_origin.1, size.0, size.1);
        let image_size = (part.width as f32 / scale, part.height as f32 / scale);
        let frame = image::RgbaImage::from_raw(part.width, part.height, part.bgra)
            .map(image::Frame::new)
            .expect("crop has width*height*4 bytes");
        let image = Arc::new(RenderImage::new(vec![frame]));
        let image_offset = (
            (display_origin.0 - client_origin.0) as f32 / scale,
            (display_origin.1 - client_origin.1) as f32 / scale,
        );

        let this_window = window.window_handle();
        let view = cx.entity().downgrade();
        let keys = cx.intercept_keystrokes(move |event: &KeystrokeEvent, window, cx| {
            if window.window_handle() != this_window {
                return;
            }
            let ks = &event.keystroke;
            let ctrl = ks.modifiers.control || ks.modifiers.platform;
            let handled = view
                .update(cx, |this, cx| {
                    if this.typing && !ctrl {
                        return this.type_key(&ks.key, ks.key_char.as_deref(), cx);
                    }
                    let free = this.tool.is_none() && !ctrl;
                    match ks.key.as_str() {
                        "escape" => {
                            close(cx);
                            true
                        }
                        "enter" => this.finish(Action::Copy, cx),
                        "c" if ctrl => this.finish(Action::Copy, cx),
                        "s" if ctrl => this.finish(Action::Save, cx),
                        "z" if ctrl => this.undo(cx),
                        "r" if free => this.set_mode(Mode::Rect, cx),
                        "w" if free && this.window_mode => this.set_mode(Mode::Window, cx),
                        "f" if free => this.set_mode(Mode::Screen, cx),
                        _ => false,
                    }
                })
                .unwrap_or(false);
            if handled {
                cx.stop_propagation();
            }
        });

        Self {
            shot,
            image,
            windows,
            window_mode,
            client_origin,
            image_offset,
            image_size,
            scale,
            mode: Mode::Rect,
            drag_from: None,
            selection: None,
            hover: None,
            tool: None,
            color: markup::COLORS[0],
            width: markup::WIDTHS[1],
            shapes: Vec::new(),
            drawing: None,
            typing: false,
            base: None,
            composite: None,
            dirty: false,
            _keys: keys,
        }
    }

    fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) -> bool {
        self.mode = mode;
        self.selection = None;
        self.hover = None;
        self.drag_from = None;
        self.clear_markup();
        cx.notify();
        true
    }

    /// Drops every shape (the selection changed).
    fn clear_markup(&mut self) {
        self.tool = None;
        self.shapes.clear();
        self.drawing = None;
        self.typing = false;
        self.base = None;
        self.dirty = true;
    }

    /// Picks a drawing tool, or puts it down when it is already in hand.
    fn toggle_tool(&mut self, tool: Tool, cx: &mut Context<Self>) {
        self.commit_text();
        self.tool = (self.tool != Some(tool)).then_some(tool);
        cx.notify();
    }

    fn undo(&mut self, cx: &mut Context<Self>) -> bool {
        if self.drawing.take().is_none() {
            self.shapes.pop();
        }
        self.typing = false;
        self.dirty = true;
        cx.notify();
        true
    }

    /// Keys while typing a text shape.
    fn type_key(&mut self, key: &str, typed: Option<&str>, cx: &mut Context<Self>) -> bool {
        match key {
            "escape" => {
                self.drawing = None;
                self.typing = false;
            }
            "enter" => self.commit_text(),
            "backspace" => {
                if let Some(Shape::Text { text, .. }) = &mut self.drawing {
                    text.pop();
                }
            }
            _ => match (typed, &mut self.drawing) {
                (Some(c), Some(Shape::Text { text, .. })) if !c.chars().any(char::is_control) => text.push_str(c),
                _ => return false,
            },
        }
        self.dirty = true;
        cx.notify();
        true
    }

    /// Keeps the text being typed, if any.
    fn commit_text(&mut self) {
        if self.typing {
            self.typing = false;
            if let Some(shape) = self.drawing.take().filter(|s| !s.is_empty()) {
                self.shapes.push(shape);
            }
            self.dirty = true;
        }
    }

    /// The selected pixels of the frozen desktop.
    fn selection_pixels(&self) -> Option<Screenshot> {
        let sel = self.selection?;
        let s = self.scale;
        let x = self.client_origin.0 + (sel.x * s).round() as i32;
        let y = self.client_origin.1 + (sel.y * s).round() as i32;
        let part = self.shot.crop(x, y, (sel.w * s).round() as u32, (sel.h * s).round() as u32);
        (part.width > 0 && part.height > 0).then_some(part)
    }

    /// `pos` relative to the selection, kept inside it.
    fn local(sel: LRect, pos: Point<Pixels>) -> Pt {
        ((pos.x.as_f32() - sel.x).clamp(0., sel.w), (pos.y.as_f32() - sel.y).clamp(0., sel.h))
    }

    /// Starts a shape when a tool is in hand. `true` when the click was used.
    fn start_shape(&mut self, pos: Point<Pixels>) -> bool {
        let (Some(tool), Some(sel)) = (self.tool, self.selection) else { return false };
        self.commit_text();
        let (x, y) = (pos.x.as_f32(), pos.y.as_f32());
        if x < sel.x || y < sel.y || x > sel.x + sel.w || y > sel.y + sel.h {
            return true;
        }
        if self.base.is_none() {
            self.base = self.selection_pixels();
        }
        let next = markup::next_number(&self.shapes);
        let shape = Shape::start(tool, Self::local(sel, pos), self.color, self.width, next);
        match tool {
            Tool::Number => self.shapes.push(shape),
            Tool::Text => {
                self.drawing = Some(shape);
                self.typing = true;
            }
            _ => self.drawing = Some(shape),
        }
        self.dirty = true;
        true
    }

    /// The image area in logical px: what selections are clipped to.
    fn area(&self) -> (f32, f32) {
        (self.image_offset.0 + self.image_size.0, self.image_offset.1 + self.image_size.1)
    }

    /// The window-relative logical rect of a desktop rect, clipped.
    fn to_logical(&self, r: WinRect) -> Option<LRect> {
        let s = self.scale;
        LRect {
            x: (r.x - self.client_origin.0) as f32 / s,
            y: (r.y - self.client_origin.1) as f32 / s,
            w: r.width as f32 / s,
            h: r.height as f32 / s,
        }
        .clip(self.area().0, self.area().1)
    }

    /// What window / full screen mode would select at `pos`.
    fn target_at(&self, pos: Point<Pixels>) -> Option<LRect> {
        let full = LRect { x: self.image_offset.0, y: self.image_offset.1, w: self.image_size.0, h: self.image_size.1 };
        match self.mode {
            Mode::Rect => None,
            Mode::Screen => Some(full),
            Mode::Window => {
                let x = self.client_origin.0 + (pos.x.as_f32() * self.scale).floor() as i32;
                let y = self.client_origin.1 + (pos.y.as_f32() * self.scale).floor() as i32;
                windows_list::window_at(&self.windows, x, y).and_then(|w| self.to_logical(w))
            }
        }
    }

    fn on_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.start_shape(event.position) {
            cx.notify();
            return;
        }
        self.clear_markup();
        match self.mode {
            Mode::Rect => {
                self.drag_from = Some(event.position);
                self.selection = None;
            }
            Mode::Window | Mode::Screen => self.selection = self.target_at(event.position),
        }
        cx.notify();
    }

    fn on_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.typing
            && let (Some(shape), Some(sel)) = (self.drawing.as_mut(), self.selection)
        {
            shape.drag_to(Self::local(sel, event.position), event.modifiers.shift);
            self.dirty = true;
            cx.notify();
        } else if let Some(from) = self.drag_from {
            let (w, h) = self.area();
            self.selection = LRect::spanning(
                (from.x.as_f32(), from.y.as_f32()),
                (event.position.x.as_f32(), event.position.y.as_f32()),
            )
            .clip(w, h);
            cx.notify();
        } else if self.selection.is_none() {
            let hover = self.target_at(event.position);
            if hover != self.hover {
                self.hover = hover;
                cx.notify();
            }
        }
    }

    fn on_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.typing && self.drawing.is_some() {
            if let Some(shape) = self.drawing.take().filter(|s| !s.is_empty()) {
                self.shapes.push(shape);
            }
            self.dirty = true;
            cx.notify();
        } else if self.drag_from.take().is_some() {
            if self.selection.is_some_and(|s| s.w < MIN_DRAG || s.h < MIN_DRAG) {
                self.selection = None;
            }
            cx.notify();
        }
    }

    /// Hands the selected pixels to the output; `false` when nothing is
    /// selected.
    fn finish(&mut self, action: Action, cx: &mut Context<Self>) -> bool {
        if self.drag_from.is_some() {
            return false;
        }
        self.commit_text();
        let part = match (&self.base, self.shapes.is_empty()) {
            (Some(base), false) => markup::render(base, &self.shapes, self.scale, false),
            _ => match self.selection_pixels() {
                Some(part) => part,
                None => return false,
            },
        };
        close(cx);
        shot_output::finish(action, part, cx);
        true
    }

    fn mode_bar(&self, p: &Palette, cx: &mut Context<Self>) -> gpui_kit::Div {
        let mut modes = vec![(Mode::Rect, "square-dashed", t("shot.mode.rect"))];
        if self.window_mode {
            modes.push((Mode::Window, "app-window", t("shot.mode.window")));
        }
        modes.push((Mode::Screen, "monitor", t("shot.mode.screen")));
        let segments = modes.into_iter().map(|(mode, icon, label)| {
            ui::segment(("shot-mode", mode as usize), icon, label, self.mode == mode, p)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.set_mode(mode, cx);
                }))
        });
        let width = self.area().0;
        div()
            .absolute()
            .top(px(16.))
            .left_0()
            .w(px(width))
            .flex()
            .justify_center()
            .child(
                bar(p)
                    .id("shot-modes")
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(ui::segmented_track(p).children(segments))
                    .child(tool_button("shot-close", "x", p).on_click(|_, _, cx| close(cx))),
            )
    }

    fn toolbar(&self, sel: LRect, p: &Palette, cx: &mut Context<Self>) -> gpui_kit::Stateful<gpui_kit::Div> {
        let (x, y) = toolbar_position(sel, (toolbar_width(), TOOLBAR_H), self.area());
        let tools = Tool::ALL.into_iter().enumerate().map(|(i, tool)| {
            let active = self.tool == Some(tool);
            div()
                .id(("shot-tool", i))
                .size(px(BUTTON))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .cursor_pointer()
                .when(active, |d| d.bg(p.list_active))
                .hover(|s| s.bg(p.list_hover))
                .child(ui::icon(tool.icon(), 16., if active { p.accent_strong } else { p.foreground }))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_tool(tool, cx)))
        });
        let colors = markup::COLORS.into_iter().enumerate().map(|(i, color)| {
            let active = self.color == color;
            div()
                .id(("shot-color", i))
                .size(px(SWATCH))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .border_2()
                .border_color(if active { p.accent } else { gpui_kit::transparent_black() })
                .child(div().size(px(14.)).rounded_full().border_1().border_color(p.border_strong).bg(gpui_kit::rgb(color)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.color = color;
                    cx.notify();
                }))
        });
        let widths = markup::WIDTHS.into_iter().enumerate().map(|(i, width)| {
            let active = self.width == width;
            div()
                .id(("shot-width", i))
                .size(px(WIDTH_BUTTON))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .cursor_pointer()
                .when(active, |d| d.bg(p.list_active))
                .hover(|s| s.bg(p.list_hover))
                .child(div().size(px(width + 2.)).rounded_full().bg(p.foreground))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.width = width;
                    cx.notify();
                }))
        });
        let separator = || div().w(px(1.)).h(px(20.)).bg(p.border);
        bar(p)
            .id("shot-toolbar")
            .absolute()
            .left(px(x))
            .top(px(y))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(tools)
            .child(separator())
            .children(colors)
            .children(widths)
            .child(separator())
            .child(tool_button("shot-undo", "undo-2", p).on_click(cx.listener(|this, _, _, cx| {
                this.undo(cx);
            })))
            .child(separator())
            .child(tool_button("shot-copy", "copy", p).on_click(cx.listener(|this, _, _, cx| {
                this.finish(Action::Copy, cx);
            })))
            .child(tool_button("shot-save", "download", p).on_click(cx.listener(|this, _, _, cx| {
                this.finish(Action::Save, cx);
            })))
            .child(tool_button("shot-cancel", "x", p).on_click(|_, _, cx| close(cx)))
    }

    /// Pixel size next to the selection.
    fn size_label(&self, sel: LRect, p: &Palette) -> gpui_kit::Div {
        let text = format!("{} × {}", (sel.w * self.scale).round(), (sel.h * self.scale).round());
        let y = if sel.y >= 28. { sel.y - 26. } else { sel.y + 4. };
        div()
            .absolute()
            .left(px(sel.x))
            .top(px(y))
            .px(px(6.))
            .h(px(22.))
            .flex()
            .items_center()
            .rounded(px(4.))
            .bg(p.surface)
            .text_color(p.foreground)
            .font_family(theme::mono_font())
            .ts(12., 16.)
            .child(text)
    }
}

/// Floating bar background.
fn bar(p: &Palette) -> gpui_kit::Div {
    div()
        .p(px(6.))
        .flex()
        .items_center()
        .gap(px(4.))
        .rounded(px(10.))
        .bg(p.surface)
        .border_1()
        .border_color(p.border)
        .shadow(p.shadow_lg.clone())
        .font_family(theme::ui_font())
        .cursor(CursorStyle::Arrow)
}

fn tool_button(id: &'static str, icon: &str, p: &Palette) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .size(px(32.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|s| s.bg(p.list_hover))
        .child(ui::icon(icon, 16., p.foreground))
}

impl Render for ShotOverlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::of(window);
        let (aw, ah) = (window.bounds().size.width.as_f32(), window.bounds().size.height.as_f32());
        let shade = gpui_kit::black().opacity(DIM);
        let lit = self.selection.or(self.hover);
        let dim: Vec<gpui_kit::Div> = match lit {
            None => vec![div().absolute().top_0().left_0().size_full().bg(shade)],
            Some(r) => {
                let piece = |x: f32, y: f32, w: f32, h: f32| {
                    div().absolute().left(px(x)).top(px(y)).w(px(w.max(0.))).h(px(h.max(0.))).bg(shade)
                };
                vec![
                    piece(0., 0., aw, r.y),
                    piece(0., r.y + r.h, aw, ah - r.y - r.h),
                    piece(0., r.y, r.x, r.h),
                    piece(r.x + r.w, r.y, aw - r.x - r.w, r.h),
                ]
            }
        };
        let outline = lit.map(|r| {
            div()
                .absolute()
                .left(px(r.x - 1.))
                .top(px(r.y - 1.))
                .w(px(r.w + 2.))
                .h(px(r.h + 2.))
                .border_1()
                .border_color(p.accent)
        });
        let size_label = self.selection.map(|s| self.size_label(s, p));
        let toolbar = self.selection.filter(|_| self.drag_from.is_none()).map(|s| self.toolbar(s, p, cx));
        if self.dirty {
            self.dirty = false;
            let mut all = self.shapes.clone();
            all.extend(self.drawing.clone());
            let next = match (&self.base, all.is_empty()) {
                (Some(base), false) => {
                    let out = markup::render(base, &all, self.scale, self.typing);
                    image::RgbaImage::from_raw(out.width, out.height, out.bgra)
                        .map(|frame| Arc::new(RenderImage::new(vec![image::Frame::new(frame)])))
                }
                _ => None,
            };
            if let Some(old) = std::mem::replace(&mut self.composite, next) {
                let _ = window.drop_image(old);
            }
        }
        let composite = self.composite.clone().zip(self.selection).map(|(image, sel)| {
            img(image).absolute().left(px(sel.x)).top(px(sel.y)).w(px(sel.w)).h(px(sel.h)).object_fit(ObjectFit::Fill)
        });
        let cursor = match self.tool {
            Some(Tool::Text) => CursorStyle::IBeam,
            _ => CursorStyle::Crosshair,
        };
        let image = img(self.image.clone())
            .absolute()
            .left(px(self.image_offset.0))
            .top(px(self.image_offset.1))
            .w(px(self.image_size.0))
            .h(px(self.image_size.1))
            .object_fit(ObjectFit::Fill);
        div()
            .id("shot")
            .size_full()
            .relative()
            .overflow_hidden()
            .cursor(cursor)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_down))
            .on_mouse_move(cx.listener(Self::on_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_up))
            .on_mouse_down(MouseButton::Right, |_, _, cx| close(cx))
            .child(image)
            .children(composite)
            .children(dim)
            .children(outline)
            .children(size_label)
            .child(self.mode_bar(p, cx))
            .children(toolbar)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_corners_in_any_order() {
        let r = LRect::spanning((50., 10.), (20., 40.));
        assert_eq!(r, LRect { x: 20., y: 10., w: 30., h: 30. });
    }

    #[test]
    fn clips_to_the_area() {
        let r = LRect { x: -10., y: 5., w: 30., h: 200. };
        assert_eq!(r.clip(100., 100.), Some(LRect { x: 0., y: 5., w: 20., h: 95. }));
        assert_eq!(LRect { x: 120., y: 0., w: 10., h: 10. }.clip(100., 100.), None);
    }

    #[test]
    fn toolbar_goes_below_then_above_then_inside() {
        let area = (1000., 800.);
        let size = (100., 40.);
        let mid = LRect { x: 100., y: 100., w: 400., h: 200. };
        assert_eq!(toolbar_position(mid, size, area), (400., 308.));
        let low = LRect { x: 100., y: 600., w: 400., h: 180. };
        assert_eq!(toolbar_position(low, size, area), (400., 552.));
        let full = LRect { x: 0., y: 0., w: 1000., h: 800. };
        assert_eq!(toolbar_position(full, size, area), (892., 752.));
    }
}
