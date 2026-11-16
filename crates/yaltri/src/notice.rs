//! A short message near the bottom of the screen ("Text copied"), for tools
//! that have no window of their own. It does not take focus and goes away
//! by itself.

use std::time::Duration;

use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, point, px, size,
};

use crate::daemon::Daemon;
use crate::theme;
use crate::ui;

const HEIGHT: f32 = 48.;
/// Distance of the message from the bottom of the display.
const BOTTOM: f32 = 96.;

/// Shows `text`; `error` messages stay longer and get a warning icon.
pub fn show(text: impl Into<SharedString>, error: bool, cx: &mut App) {
    let text: SharedString = text.into();
    if let Some(old) = cx.global_mut::<Daemon>().notice_window.take() {
        let _ = old.update(cx, |_, window, _| window.remove_window());
    }
    // Rough width: the window is transparent, so a little slack is harmless.
    let width = text.chars().count() as f32 * 7.5 + 72.;
    let window_size = size(px(width), px(HEIGHT));
    let mut bounds = Bounds::centered(None, window_size, cx);
    if let Some(display) = cx.primary_display() {
        let area = display.bounds();
        bounds.origin = point(
            area.origin.x + (area.size.width - window_size.width) / 2.,
            area.origin.y + area.size.height - px(BOTTOM + HEIGHT),
        );
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        focus: false,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        app_id: Some("yaltri-notice".into()),
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };
    let opened = gpui_kit::open_window(options, cx, move |_, cx| cx.new(|_| Notice { text, error }));
    let handle: AnyWindowHandle = match opened {
        Ok((handle, _)) => handle,
        Err(e) => {
            tracing::warn!("showing a notice failed: {e:#}");
            return;
        }
    };
    cx.global_mut::<Daemon>().notice_window = Some(handle);
    let wait = Duration::from_millis(if error { 6000 } else { 2200 });
    cx.spawn(async move |cx| {
        cx.background_executor().timer(wait).await;
        cx.update(|cx| {
            if cx.global::<Daemon>().notice_window == Some(handle) {
                cx.global_mut::<Daemon>().notice_window = None;
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            }
        });
    })
    .detach();
}

struct Notice {
    text: SharedString,
    error: bool,
}

impl Render for Notice {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let p = theme::of(window);
        let card = if self.error {
            div()
                .h(px(36.))
                .px(px(12.))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(px(8.))
                .bg(p.danger_bg)
                .border_1()
                .border_color(p.danger)
                .text_color(p.danger)
                .text_size(px(13.))
                .whitespace_nowrap()
                .child(ui::icon("circle-alert", 14., p.danger))
                .child(self.text.clone())
        } else {
            ui::toast(self.text.clone(), None, p)
        };
        div().size_full().flex().items_center().justify_center().font_family(theme::ui_font()).child(card)
    }
}
