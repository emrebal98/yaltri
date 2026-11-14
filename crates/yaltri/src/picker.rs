//! Color picking, PowerToys style.
//!
//! Windows / X11: the desktop is captured once and every display gets a
//! borderless, topmost window showing its part of that frozen image. A small
//! box next to the cursor shows the color and its code. Scrolling up opens a
//! magnifier square centered on the cursor that grows with every step;
//! scrolling down shrinks and finally hides it. Left click picks, right click
//! or Esc cancels.
//!
//! Wayland: clients cannot read the screen, so the compositor's own picker
//! is used through the `PickColor` portal.

use std::sync::Arc;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, CursorStyle, InteractiveElement as _, IntoElement,
    KeystrokeEvent, MouseButton, MouseDownEvent, MouseMoveEvent, ObjectFit, ParentElement as _, Pixels, Point,
    Render, RenderImage, ScrollEvent, Styled as _, StyledImage as _, Subscription, Window, WindowBounds,
    WindowKind, WindowOptions, div, img, px, rgb,
};
use yaltri_color::{Format, Rgb};
use yaltri_platform::screen::{self, Screenshot};

use crate::color_editor;
use crate::daemon::Daemon;

/// Magnifier levels: (cells per side, logical size of one cell). Each step
/// shows a bigger square with fewer, larger pixels.
const ZOOM_LEVELS: [(usize, f32); 5] = [(21, 8.), (21, 12.), (17, 18.), (15, 24.), (11, 36.)];
/// Gap between the cursor (or the magnifier) and the color box.
const BOX_GAP: f32 = 16.;

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
    /// 0 = no magnifier, 1..=ZOOM_LEVELS.len() = magnifier level.
    zoom: usize,
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
            zoom: 0,
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

    fn color_at(&self, pos: Point<Pixels>) -> Option<Rgb> {
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

    fn on_wheel(&mut self, event: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let dy = event.delta.pixel_delta(px(16.)).y.as_f32();
        if dy > 0. && self.zoom < ZOOM_LEVELS.len() {
            self.zoom += 1;
        } else if dy < 0. && self.zoom > 0 {
            self.zoom -= 1;
        }
        cx.notify();
    }

    /// Side length of the magnifier square in logical px (0 when hidden).
    fn loupe_size(&self) -> f32 {
        match self.zoom {
            0 => 0.,
            z => {
                let (cells, cell) = ZOOM_LEVELS[z - 1];
                cells as f32 * cell
            }
        }
    }

    /// Magnifier square centered on the cursor, nearest-neighbor pixels.
    fn loupe(&self, cursor: Point<Pixels>, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let (cells, cell) = ZOOM_LEVELS[self.zoom - 1];
        let size = cells as f32 * cell;
        let (cx0, cy0) = self.desktop_pixel(cursor);
        let half = (cells / 2) as i32;
        let rows = (0..cells as i32).map(move |row| {
            div().flex().children((0..cells as i32).map(move |col| {
                let [r, g, b] = self.shot.pixel(cx0 + col - half, cy0 + row - half).unwrap_or([0, 0, 0]);
                let swatch = div().size(px(cell)).bg(rgb(u32::from_be_bytes([0, r, g, b])));
                if row == half && col == half {
                    let ring = if Rgb::new(r, g, b).is_dark() { gpui_kit::white() } else { gpui_kit::black() };
                    swatch.border_2().border_color(ring)
                } else {
                    swatch
                }
            }))
        });
        div()
            .absolute()
            .left(cursor.x - px(size / 2.))
            .top(cursor.y - px(size / 2.))
            .size(px(size))
            .rounded(theme.radius_lg)
            .overflow_hidden()
            .border_2()
            .border_color(theme.border)
            .shadow_lg()
            .children(rows)
    }

    /// Small box with the color and its code, beside the cursor (or beside
    /// the magnifier when it is open), flipped to stay on screen.
    fn color_box(&self, cursor: Point<Pixels>, window: &Window, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let color = self.color_at(cursor).unwrap_or(Rgb::new(0, 0, 0));
        let format: Format = cx.global::<Daemon>().settings.picker.copy_format.parse().unwrap_or(Format::Hex);
        let text = format.format(color);

        let (box_w, box_h) = (40. + text.len() as f32 * 9., 40.);
        let reach = self.loupe_size() / 2. + BOX_GAP;
        let area = window.bounds().size;
        let mut x = cursor.x.as_f32() + reach;
        let mut y = cursor.y.as_f32() + reach;
        if x + box_w > area.width.as_f32() {
            x = cursor.x.as_f32() - reach - box_w;
        }
        if y + box_h > area.height.as_f32() {
            y = cursor.y.as_f32() - reach - box_h;
        }

        div()
            .absolute()
            .left(px(x))
            .top(px(y))
            .h(px(box_h))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius_lg)
            .shadow_lg()
            .child(div().size(px(24.)).rounded(theme.radius).border_1().border_color(theme.border).bg(fill(color)))
            .child(div().text_sm().child(text))
    }
}

impl Render for PickerOverlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let loupe = self.cursor.filter(|_| self.zoom > 0).map(|c| self.loupe(c, cx));
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
            .cursor(CursorStyle::Crosshair)
            .on_mouse_move(cx.listener(Self::on_move))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_click))
            .on_mouse_down(MouseButton::Right, cx.listener(|_, _, _, cx| finish(None, cx)))
            .on_scroll(cx.listener(Self::on_wheel))
            .child(image)
            .children(loupe)
            .children(color_box)
    }
}
