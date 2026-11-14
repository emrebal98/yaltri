//! Color picking, PowerToys style.
//!
//! Windows / X11: the desktop is captured once and every display gets a
//! borderless, topmost window showing its part of that frozen image. A panel
//! follows the cursor with a magnifier and the color under it; the wheel
//! zooms, left click picks, right click or Esc cancels.
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

/// Magnifier side length in logical pixels and the zoom steps (cells per side).
const LOUPE: f32 = 132.;
const ZOOM_STEPS: [usize; 6] = [5, 7, 9, 11, 15, 21];
const DEFAULT_ZOOM: usize = 2;
const PANEL_OFFSET: f32 = 20.;

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
            let origin = (
                (display_bounds.origin.x.as_f32() * scale).round() as i32,
                (display_bounds.origin.y.as_f32() * scale).round() as i32,
            );
            let size = (
                (display_bounds.size.width.as_f32() * scale).round() as u32,
                (display_bounds.size.height.as_f32() * scale).round() as u32,
            );
            let view = cx.new(|cx| PickerOverlay::new(shot, origin, size, scale, window, cx));
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

pub struct PickerOverlay {
    shot: Arc<Screenshot>,
    image: Arc<RenderImage>,
    /// This display's top-left in physical desktop pixels.
    origin: (i32, i32),
    scale: f32,
    cursor: Option<Point<Pixels>>,
    zoom: usize,
    _keys: Subscription,
}

impl PickerOverlay {
    fn new(
        shot: Arc<Screenshot>,
        origin: (i32, i32),
        size: (u32, u32),
        scale: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let part = shot.crop(origin.0, origin.1, size.0, size.1);
        let frame = image::RgbaImage::from_raw(part.width, part.height, part.bgra)
            .map(image::Frame::new)
            .expect("crop has width*height*4 bytes");
        // RenderImage expects BGRA, which is what the capture produced.
        let image = Arc::new(RenderImage::new(vec![frame]));

        let this_window = window.window_handle();
        let keys = cx.intercept_keystrokes(move |event: &KeystrokeEvent, window, cx| {
            if window.window_handle() == this_window && event.keystroke.key == "escape" {
                finish(None, cx);
                cx.stop_propagation();
            }
        });

        Self { shot, image, origin, scale, cursor: Some(window.mouse_position()), zoom: DEFAULT_ZOOM, _keys: keys }
    }

    /// Desktop pixel under a window-relative logical position.
    fn desktop_pixel(&self, pos: Point<Pixels>) -> (i32, i32) {
        (
            self.origin.0 + (pos.x.as_f32() * self.scale).floor() as i32,
            self.origin.1 + (pos.y.as_f32() * self.scale).floor() as i32,
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
        if dy > 0. && self.zoom > 0 {
            self.zoom -= 1; // fewer cells = more magnified
        } else if dy < 0. && self.zoom + 1 < ZOOM_STEPS.len() {
            self.zoom += 1;
        }
        cx.notify();
    }

    fn panel(&self, cursor: Point<Pixels>, window: &Window, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let cells = ZOOM_STEPS[self.zoom];
        let cell = LOUPE / cells as f32;
        let (cx0, cy0) = self.desktop_pixel(cursor);
        let half = (cells / 2) as i32;

        let grid = (0..cells as i32).map(|row| {
            div().flex().children((0..cells as i32).map(move |col| {
                let [r, g, b] = self.shot.pixel(cx0 + col - half, cy0 + row - half).unwrap_or([0, 0, 0]);
                let center = row == half && col == half;
                let swatch = div().size(px(cell)).bg(rgb(u32::from_be_bytes([0, r, g, b])));
                if center {
                    swatch.border_2().border_color(if Rgb::new(r, g, b).is_dark() { gpui_kit::white() } else { gpui_kit::black() })
                } else {
                    swatch
                }
            }))
        });

        let color = self.color_at(cursor).unwrap_or(Rgb::new(0, 0, 0));
        let format: Format = cx.global::<Daemon>().settings.picker.copy_format.parse().unwrap_or(Format::Hex);

        // Keep the panel on screen: flip to the other side near edges.
        let panel_w = LOUPE + 16.;
        let panel_h = LOUPE + 52.;
        let size = window.bounds().size;
        let mut x = cursor.x.as_f32() + PANEL_OFFSET;
        let mut y = cursor.y.as_f32() + PANEL_OFFSET;
        if x + panel_w > size.width.as_f32() {
            x = cursor.x.as_f32() - PANEL_OFFSET - panel_w;
        }
        if y + panel_h > size.height.as_f32() {
            y = cursor.y.as_f32() - PANEL_OFFSET - panel_h;
        }

        div()
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(panel_w))
            .p_2()
            .flex()
            .flex_col()
            .gap_2()
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius_lg)
            .shadow_lg()
            .child(div().rounded(theme.radius).overflow_hidden().children(grid))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size(px(22.))
                            .rounded(theme.radius)
                            .border_1()
                            .border_color(theme.border)
                            .bg(rgb(u32::from_be_bytes([0, color.r, color.g, color.b]))),
                    )
                    .child(div().text_sm().child(format.format(color))),
            )
    }
}

impl Render for PickerOverlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = self.cursor.map(|c| self.panel(c, window, cx));
        div()
            .id("picker")
            .size_full()
            .relative()
            .cursor(CursorStyle::Crosshair)
            .on_mouse_move(cx.listener(Self::on_move))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_click))
            .on_mouse_down(MouseButton::Right, cx.listener(|_, _, _, cx| finish(None, cx)))
            .on_scroll(cx.listener(Self::on_wheel))
            .child(img(self.image.clone()).absolute().top_0().left_0().size_full().object_fit(ObjectFit::Fill))
            .children(panel)
    }
}
