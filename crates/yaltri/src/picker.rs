//! Color picking, PowerToys style.
//!
//! Windows / X11: the desktop is captured once and every display gets a
//! borderless, topmost window showing its part of that frozen image. A small
//! box next to the cursor shows the color, its code and name. Scrolling up
//! opens the magnifier (see `picker_zoom`): a frozen patch around the cursor
//! that grows 1x-8x; the pixel under the pointer inside it is highlighted and
//! picked. Left click picks, right click or Esc cancels.
//!
//! Wayland: clients cannot read the screen, so the compositor's own picker
//! is used through the `PickColor` portal.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, InteractiveElement as _, IntoElement,
    KeystrokeEvent, MouseButton, MouseDownEvent, MouseMoveEvent, ObjectFit, ParentElement as _, Pixels, Point,
    Render, RenderImage, ScrollEvent, Styled as _, StyledImage as _, Subscription, Window, WindowBounds,
    WindowKind, WindowOptions, div, img, px, rgb,
};
use yaltri_color::{Format, Rgb};
use yaltri_platform::screen::{self, Screenshot};

use crate::color_editor;
use crate::daemon::Daemon;
use crate::picker_zoom::{self as zoom, Capture};

/// Offset of the color box from the cursor.
const BOX_OFFSET: f32 = 14.;
/// Padding + border around the magnified image (PowerToys: 3 + 1).
const LOUPE_CHROME: f32 = 4.;
const RESIZE: Duration = Duration::from_millis(200);

/// Starts picking, or cancels if already picking.
pub fn toggle(cx: &mut App) {
    if !cx.global::<Daemon>().picker_windows.is_empty() {
        close_overlays(cx);
        return;
    }
    if cx.global::<Daemon>().session.is_wayland() {
        pick_with_portal(cx);
    } else {
        open_overlays(cx);
    }
}

fn open_overlays(cx: &mut App) {
    let shot = match screen::capture() {
        Ok(s) => Arc::new(s),
        Err(e) => {
            tracing::error!("screen capture failed: {e:#}");
            return;
        }
    };
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
            app_id: Some("yaltri-picker".into()),
            ..Default::default()
        };
        let shot = shot.clone();
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
            // Where the window's content really starts. On Windows a popup
            // window's client area can sit a few pixels off the display edge
            // (measured: 4 px above). Draw and sample relative to it, or the
            // frozen image looks shifted and the picked pixel is wrong.
            let client_origin = client_origin(window).unwrap_or(display_origin);
            let view = cx.new(|cx| PickerOverlay::new(shot, display_origin, client_origin, size, scale, window, cx));
            window.activate_window();
            view
        });
        match opened {
            Ok((handle, _)) => handles.push(handle),
            Err(e) => tracing::error!("opening picker window failed: {e:#}"),
        }
    }
    cx.global_mut::<Daemon>().picker_windows = handles;
    cx.activate(true);
}

/// Top-left of the window's content in physical desktop pixels.
#[cfg(windows)]
fn client_origin(window: &Window) -> Option<(i32, i32)> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(h) => screen::client_origin(h.hwnd.get()),
        _ => None,
    }
}

#[cfg(not(windows))]
fn client_origin(_: &Window) -> Option<(i32, i32)> {
    None
}

/// Closes every overlay window (deferred: may be called from inside one).
fn close_overlays(cx: &mut App) {
    let handles: Vec<AnyWindowHandle> = std::mem::take(&mut cx.global_mut::<Daemon>().picker_windows);
    cx.defer(move |cx| {
        for h in handles {
            let _ = h.update(cx, |_, window, _| window.remove_window());
        }
    });
}

#[cfg(target_os = "linux")]
fn pick_with_portal(cx: &mut App) {
    let task = cx.background_executor().spawn(screen::pick_color_portal());
    cx.spawn(async move |cx| match task.await {
        Ok(Some([r, g, b])) => cx.update(|cx| finish(Some(Rgb::new(r, g, b)), cx)),
        Ok(None) => {}
        Err(e) => tracing::error!("portal color pick failed: {e:#}"),
    })
    .detach();
}

#[cfg(not(target_os = "linux"))]
fn pick_with_portal(_cx: &mut App) {
    tracing::error!("portal color picking is only available on Linux");
}

/// Ends a pick: copies the color, records it, shows the editor.
pub fn finish(color: Option<Rgb>, cx: &mut App) {
    close_overlays(cx);
    let Some(color) = color else { return };
    let daemon = cx.global_mut::<Daemon>();
    let format: Format = daemon.settings.picker.copy_format.parse().unwrap_or(Format::Hex);
    let open_editor = daemon.settings.picker.open_editor;
    daemon.record_color(color);
    let text = format.format(color);
    tracing::info!(%text, "color picked");
    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text));
    if open_editor {
        color_editor::show(color, cx);
    }
}

fn fill(c: Rgb) -> gpui_kit::Rgba {
    rgb(u32::from_be_bytes([0, c.r, c.g, c.b]))
}

/// The open magnifier.
struct Loupe {
    level: usize,
    /// Center of the square, window-relative logical px (where the cursor
    /// was on the first scroll).
    center: Point<Pixels>,
    capture: Capture,
    image: Arc<RenderImage>,
    /// Size the resize animation started from, and when.
    from_side: f32,
    started: Instant,
}

impl Loupe {
    fn side_now(&self) -> f32 {
        let t = self.started.elapsed().as_secs_f32() / RESIZE.as_secs_f32();
        zoom::eased(self.from_side, zoom::side(self.level), t)
    }

    fn animating(&self) -> bool {
        self.started.elapsed() < RESIZE
    }

    /// Cell under `pos`, using the settled (target) size.
    fn cell_under(&self, pos: Point<Pixels>) -> Option<(usize, usize)> {
        let side = zoom::side(self.level);
        let left = self.center.x.as_f32() - side / 2.;
        let top = self.center.y.as_f32() - side / 2.;
        zoom::cell_at(pos.x.as_f32() - left, pos.y.as_f32() - top, side)
    }
}

fn loupe_image(capture: &Capture, level: usize, scale: f32) -> Arc<RenderImage> {
    let physical = (zoom::side(level) * scale).round().max(1.) as u32;
    let bgra = zoom::render(capture, physical);
    let frame = image::RgbaImage::from_raw(physical, physical, bgra).map(image::Frame::new).expect("square buffer");
    Arc::new(RenderImage::new(vec![frame]))
}

pub struct PickerOverlay {
    shot: Arc<Screenshot>,
    image: Arc<RenderImage>,
    /// Physical desktop position of the window's content (0,0).
    client_origin: (i32, i32),
    /// Where this display's image goes inside the window, and its size, in
    /// logical px.
    image_offset: (f32, f32),
    image_size: (f32, f32),
    scale: f32,
    cursor: Option<Point<Pixels>>,
    loupe: Option<Loupe>,
    /// Wheel movement not yet turned into zoom steps. GPUI merges wheel
    /// events that arrive close together, so one event can be several notches.
    wheel: f32,
    lines_per_notch: f32,
    _keys: Subscription,
}

impl PickerOverlay {
    fn new(
        shot: Arc<Screenshot>,
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
        // RenderImage expects BGRA, which is what the capture produced.
        let image = Arc::new(RenderImage::new(vec![frame]));
        let image_offset = (
            (display_origin.0 - client_origin.0) as f32 / scale,
            (display_origin.1 - client_origin.1) as f32 / scale,
        );

        let this_window = window.window_handle();
        let keys = cx.intercept_keystrokes(move |event: &KeystrokeEvent, window, cx| {
            if window.window_handle() == this_window && event.keystroke.key == "escape" {
                finish(None, cx);
                cx.stop_propagation();
            }
        });

        Self {
            shot,
            image,
            client_origin,
            image_offset,
            image_size,
            scale,
            cursor: Some(window.mouse_position()),
            loupe: None,
            wheel: 0.,
            lines_per_notch: screen::wheel_lines_per_notch(),
            _keys: keys,
        }
    }

    /// Desktop pixel under a window-relative logical position.
    fn desktop_pixel(&self, pos: Point<Pixels>) -> (i32, i32) {
        (
            self.client_origin.0 + (pos.x.as_f32() * self.scale).floor() as i32,
            self.client_origin.1 + (pos.y.as_f32() * self.scale).floor() as i32,
        )
    }

    /// The color under `pos`: the magnified pixel when pointing into the
    /// magnifier, the screen pixel otherwise.
    fn color_at(&self, pos: Point<Pixels>) -> Option<Rgb> {
        if let Some(loupe) = &self.loupe
            && let Some((col, row)) = loupe.cell_under(pos)
        {
            return Some(loupe.capture.at(col, row));
        }
        let (x, y) = self.desktop_pixel(pos);
        self.shot.pixel(x, y).map(|[r, g, b]| Rgb::new(r, g, b))
    }

    fn on_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.cursor = Some(event.position);
        cx.notify();
    }

    fn on_click(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let color = self.color_at(event.position);
        finish(color, cx);
    }

    fn on_wheel(&mut self, event: &ScrollEvent, window: &mut Window, cx: &mut Context<Self>) {
        // In notches: line deltas from a mouse wheel, pixel deltas from a
        // touchpad (48 px per step).
        self.wheel += match event.delta {
            gpui_kit::ScrollDelta::Lines(lines) => lines.y / self.lines_per_notch,
            gpui_kit::ScrollDelta::Pixels(pixels) => pixels.y.as_f32() / 48.,
        };
        tracing::debug!(?event.delta, acc = self.wheel, "picker wheel");
        let steps = self.wheel.trunc();
        self.wheel -= steps;
        let cursor = event.position;
        let current = self.loupe.as_ref().map_or(0, |l| l.level);
        let target = (current as i32 + steps as i32).clamp(0, zoom::MAX_LEVEL as i32) as usize;
        if target == current {
            return;
        }
        let old = match (self.loupe.take(), target) {
            (None, _) => {
                // First step: freeze the patch around the cursor; no animation.
                let (x, y) = self.desktop_pixel(cursor);
                let capture = Capture::from_shot(&self.shot, x, y);
                let image = loupe_image(&capture, 1, self.scale);
                self.loupe = Some(Loupe {
                    level: 1,
                    center: cursor,
                    capture,
                    image,
                    from_side: zoom::side(1),
                    started: Instant::now() - RESIZE,
                });
                None
            }
            (Some(loupe), 0) => Some(loupe.image),
            (Some(loupe), level) => {
                let from_side = loupe.side_now();
                let image = loupe_image(&loupe.capture, level, self.scale);
                let old_image = loupe.image.clone();
                self.loupe = Some(Loupe { level, image, from_side, started: Instant::now(), ..loupe });
                Some(old_image)
            }
        };
        if let Some(old) = old {
            let _ = window.drop_image(old);
        }
        cx.notify();
    }

    /// Magnifier card: frozen patch, baked grid, highlighted pointer cell and
    /// a small sample of the pointed color right at the cursor.
    fn loupe_view(&self, loupe: &Loupe, window: &mut Window, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let side = loupe.side_now();
        if loupe.animating() {
            window.request_animation_frame();
        }
        let left = loupe.center.x.as_f32() - side / 2.;
        let top = loupe.center.y.as_f32() - side / 2.;

        // Grid circle and pointer highlight only once the size settled.
        let highlight = (!loupe.animating())
            .then_some(self.cursor)
            .flatten()
            .and_then(|c| loupe.cell_under(c).map(|cell| (c, cell)))
            .map(|(cursor, (col, row))| {
                let cell = side / zoom::BASE as f32;
                let color = loupe.capture.at(col, row);
                let ring = if color.is_dark() { gpui_kit::white() } else { gpui_kit::black() };
                let pointer = (cursor.x.as_f32() - left, cursor.y.as_f32() - top);
                let grid = zoom::grid_near(&loupe.capture, side, pointer).into_iter().map(|l| {
                    let base = if l.black { gpui_kit::black() } else { gpui_kit::white() };
                    div()
                        .absolute()
                        .left(px(LOUPE_CHROME + l.x))
                        .top(px(LOUPE_CHROME + l.y))
                        .w(px(l.w))
                        .h(px(l.h))
                        .bg(base.opacity(l.alpha))
                });
                grid.chain([
                    div()
                        .absolute()
                        .left(px(LOUPE_CHROME + col as f32 * cell))
                        .top(px(LOUPE_CHROME + row as f32 * cell))
                        .size(px(cell))
                        .border_2()
                        .border_color(ring),
                    div()
                        .absolute()
                        .left(px(cursor.x.as_f32() - left + LOUPE_CHROME - 2.))
                        .top(px(cursor.y.as_f32() - top + LOUPE_CHROME - 2.))
                        .size(px(4.))
                        .bg(fill(color)),
                ])
                .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        div()
            .absolute()
            .left(px(left - LOUPE_CHROME))
            .top(px(top - LOUPE_CHROME))
            .size(px(side + LOUPE_CHROME * 2.))
            .p(px(LOUPE_CHROME - 1.))
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius)
            .shadow_lg()
            .child(img(loupe.image.clone()).size(px(side)).object_fit(ObjectFit::Fill))
            // Deferred: drawn after (above) the magnified image sprite.
            .children(highlight.into_iter().map(gpui_kit::deferred))
    }

    /// PowerToys' color box: swatch, code and color name, next to the cursor
    /// and flipped to stay on screen.
    fn color_box(&self, cursor: Point<Pixels>, window: &Window, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let color = self.color_at(cursor).unwrap_or(Rgb::new(0, 0, 0));
        let format: Format = cx.global::<Daemon>().settings.picker.copy_format.parse().unwrap_or(Format::Hex);
        let text = format.format(color);
        let name = color.name();

        let box_w = 56. + (text.chars().count().max(name.chars().count()) as f32) * 8.5;
        let box_h = 50.;
        let area = window.bounds().size;
        let mut x = cursor.x.as_f32() + BOX_OFFSET;
        let mut y = cursor.y.as_f32() + BOX_OFFSET;
        if x + box_w > area.width.as_f32() {
            x = cursor.x.as_f32() - BOX_OFFSET - box_w;
        }
        if y + box_h > area.height.as_f32() {
            y = cursor.y.as_f32() - BOX_OFFSET - box_h;
        }

        div()
            .absolute()
            .left(px(x))
            .top(px(y))
            .p_2()
            .flex()
            .items_center()
            .gap_2()
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius_lg)
            .shadow_lg()
            .child(
                div()
                    .size(px(32.))
                    .flex_shrink_0()
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(theme.border)
                    .bg(fill(color)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().text_sm().font_weight(gpui_kit::FontWeight::SEMIBOLD).child(text))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(name)),
            )
    }
}

impl Render for PickerOverlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let loupe = self.loupe.as_ref().map(|l| self.loupe_view(l, window, cx).into_any_element());
        let color_box = self.cursor.map(|c| self.color_box(c, window, cx));
        let image = img(self.image.clone())
            .absolute()
            .left(px(self.image_offset.0))
            .top(px(self.image_offset.1))
            .w(px(self.image_size.0))
            .h(px(self.image_size.1))
            .object_fit(ObjectFit::Fill);
        div()
            .id("picker")
            .size_full()
            .relative()
            .overflow_hidden()
            .on_mouse_move(cx.listener(Self::on_move))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_click))
            .on_mouse_down(MouseButton::Right, cx.listener(|_, _, _, cx| finish(None, cx)))
            .on_scroll(cx.listener(Self::on_wheel))
            .child(image)
            .children(loupe)
            .children(color_box)
    }
}
