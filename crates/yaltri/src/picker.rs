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

use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, FontWeight, InteractiveElement as _, IntoElement, KeystrokeEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, ObjectFit, ParentElement as _, Pixels, Point, Render, RenderImage,
    ScrollEvent, Styled as _, StyledImage as _, Subscription, Window, WindowBounds, WindowKind, WindowOptions, div,
    img, px, rgb,
};
use yaltri_color::{Format, Rgb};
use yaltri_platform::screen::{self, Screenshot};

use crate::color_editor;
use crate::daemon::Daemon;
use crate::picker_zoom::{self as zoom, Capture};
use crate::theme::{self, Palette};
use crate::ui::{self, TextStyle as _};

/// Offset of the color label from the cursor.
const BOX_OFFSET: f32 = 16.;
/// The label flips to the other side this close to a screen edge.
const EDGE: f32 = 8.;
/// Gap between the magnifier and the label under it.
const LOUPE_LABEL_GAP: f32 = 8.;
/// The magnifier's 1 px frame around the magnified image.
const LOUPE_CHROME: f32 = 1.;
const LOUPE_RADIUS: f32 = 12.;
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
            ui::focus_popup(window);
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
    fn loupe_view(&self, loupe: &Loupe, window: &mut Window, p: &Palette) -> impl IntoElement {
        let side = loupe.side_now();
        if loupe.animating() {
            window.request_animation_frame();
        }
        let left = loupe.center.x.as_f32() - side / 2.;
        let top = loupe.center.y.as_f32() - side / 2.;

        // From 4x on: outline around the pointed pixel and the tiny grid
        // circle, once the size settled (PowerToys' grid shader).
        let overlay = (!loupe.animating())
            .then_some(self.cursor)
            .flatten()
            .and_then(|c| zoom::pointer_overlay(&loupe.capture, loupe.level, (c.x.as_f32() - left, c.y.as_f32() - top)))
            .map(|(outline, grid)| {
                let at = |x: i32, y: i32| {
                    div().absolute().left(px(LOUPE_CHROME + x as f32)).top(px(LOUPE_CHROME + y as f32))
                };
                let line = |white: bool| {
                    if white { gpui_kit::white() } else { gpui_kit::black() }
                };
                grid.into_iter()
                    .map(|g| at(g.x, g.y).size(px(1.)).bg(line(g.white).opacity(g.strength)))
                    .chain([at(outline.x, outline.y).size(px(outline.side as f32)).border_1().border_color(line(outline.white))])
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        div()
            .absolute()
            .left(px(left - LOUPE_CHROME))
            .top(px(top - LOUPE_CHROME))
            .size(px(side + LOUPE_CHROME * 2.))
            .border_1()
            .border_color(p.border)
            .rounded(px(LOUPE_RADIUS))
            .shadow(p.shadow_lg.clone())
            .child(
                img(loupe.image.clone())
                    .size(px(side))
                    .rounded(px(LOUPE_RADIUS - LOUPE_CHROME))
                    .object_fit(ObjectFit::Fill),
            )
            // Deferred: drawn after (above) the magnified image sprite.
            .children(overlay.into_iter().map(gpui_kit::deferred))
    }

    /// Color label: swatch, code and name. Follows the cursor (flipping
    /// near screen edges), or sits centered under the open magnifier.
    fn color_box(&self, cursor: Point<Pixels>, window: &Window, p: &Palette, cx: &App) -> impl IntoElement {
        let color = self.color_at(cursor).unwrap_or(Rgb::new(0, 0, 0));
        let format: Format = cx.global::<Daemon>().settings.picker.copy_format.parse().unwrap_or(Format::Hex);
        let text = format.format(color);
        let name = color.name();

        // Size estimate for placement: 6 + swatch 44 + gap 10 + text + 14.
        let text_w = (text.chars().count() as f32 * 9.).max(name.chars().count() as f32 * 7.5);
        let box_w = 6. + 44. + 10. + text_w + 14. + 2.;
        let box_h = 6. + 44. + 6. + 2.;
        let area = window.bounds().size;
        let (x, y) = match &self.loupe {
            Some(loupe) => {
                let side = loupe.side_now() + LOUPE_CHROME * 2.;
                let x = loupe.center.x.as_f32() - box_w / 2.;
                let below = loupe.center.y.as_f32() + side / 2. + LOUPE_LABEL_GAP;
                let y = if below + box_h + EDGE > area.height.as_f32() {
                    loupe.center.y.as_f32() - side / 2. - LOUPE_LABEL_GAP - box_h
                } else {
                    below
                };
                (x.clamp(EDGE, (area.width.as_f32() - box_w - EDGE).max(EDGE)), y)
            }
            None => {
                let mut x = cursor.x.as_f32() + BOX_OFFSET;
                let mut y = cursor.y.as_f32() + BOX_OFFSET;
                if x + box_w + EDGE > area.width.as_f32() {
                    x = cursor.x.as_f32() - BOX_OFFSET - box_w;
                }
                if y + box_h + EDGE > area.height.as_f32() {
                    y = cursor.y.as_f32() - BOX_OFFSET - box_h;
                }
                (x, y)
            }
        };

        div()
            .absolute()
            .left(px(x))
            .top(px(y))
            .pl(px(6.))
            .pr(px(14.))
            .py(px(6.))
            .flex()
            .items_center()
            .gap(px(10.))
            .bg(p.surface)
            .text_color(p.foreground)
            .font_family(theme::ui_font())
            .border_1()
            .border_color(p.border)
            .rounded(px(10.))
            .shadow(p.shadow_md.clone())
            .child(
                div()
                    .size(px(44.))
                    .flex_none()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(p.swatch_border())
                    .bg(fill(color)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().ts(15., 20.).font_weight(FontWeight::SEMIBOLD).whitespace_nowrap().child(text))
                    .child(div().ts(13., 18.).text_color(p.muted).whitespace_nowrap().child(name)),
            )
    }
}

impl Render for PickerOverlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::of(window);
        let loupe = self.loupe.as_ref().map(|l| self.loupe_view(l, window, p).into_any_element());
        let color_box = self.cursor.map(|c| self.color_box(c, window, p, cx));
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
