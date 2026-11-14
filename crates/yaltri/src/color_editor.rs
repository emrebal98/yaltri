//! Color editor, shown after a pick (PowerToys style): picked-color history,
//! lighter/darker shades of the selected color and every text format with a
//! copy button. Clicking a format's star makes it the one copied on pick.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Selectable as _};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Bounds, ClipboardItem, Context, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Task, TitlebarOptions,
    Window, WindowBounds, WindowOptions, div, px, rgb, size,
};
use yaltri_color::{Format, Rgb};

use crate::daemon::Daemon;
use crate::picker;

const SWATCH: f32 = 32.;

/// Opens the editor showing `color`, or updates it if already open.
pub fn show(color: Rgb, cx: &mut App) {
    if let Some(editor) = cx.global::<Daemon>().color_editor.clone()
        && let Some(handle) = cx.global::<Daemon>().color_editor_window
        && cx.windows().contains(&handle)
    {
        editor.update(cx, |e, cx| {
            e.selected = color;
            cx.notify();
        });
        let _ = handle.update(cx, |_, window, _| window.activate_window());
        return;
    }

    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(620.), px(520.)), cx))),
        titlebar: Some(TitlebarOptions { title: Some("Yaltri Renk Seçici".into()), ..Default::default() }),
        app_id: Some("yaltri".into()),
        ..Default::default()
    };
    match gpui_kit::open_window(options, cx, |_, cx| cx.new(|_| ColorEditor { selected: color, toast: None, _toast_timer: None })) {
        Ok((handle, editor)) => {
            let daemon = cx.global_mut::<Daemon>();
            daemon.color_editor = Some(editor);
            daemon.color_editor_window = Some(handle);
            let _ = handle.update(cx, |_, window, _| window.activate_window());
            cx.activate(true);
        }
        Err(e) => tracing::error!("opening color editor failed: {e:#}"),
    }
}

pub struct ColorEditor {
    selected: Rgb,
    toast: Option<SharedString>,
    _toast_timer: Option<Task<()>>,
}

fn fill(c: Rgb) -> gpui_kit::Rgba {
    rgb(u32::from_be_bytes([0, c.r, c.g, c.b]))
}

impl ColorEditor {
    fn copy(&mut self, format: Format, cx: &mut Context<Self>) {
        let text = format.format(self.selected);
        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        self.flash(format!("Kopyalandı: {text}"), cx);
    }

    fn set_default_format(&mut self, format: Format, cx: &mut Context<Self>) {
        let daemon = cx.global_mut::<Daemon>();
        let mut settings = daemon.settings.clone();
        settings.picker.copy_format = format.key().into();
        let msg = match daemon.update_settings(settings) {
            Ok(()) => format!("Seçimde artık {} kopyalanacak", format.label()),
            Err(e) => format!("Kaydedilemedi: {e:#}"),
        };
        self.flash(msg, cx);
    }

    fn select(&mut self, color: Rgb, cx: &mut Context<Self>) {
        self.selected = color;
        cx.notify();
    }

    /// Adopts a shade: selects it and remembers it in the history.
    fn adopt(&mut self, color: Rgb, cx: &mut Context<Self>) {
        cx.global_mut::<Daemon>().record_color(color);
        self.select(color, cx);
    }

    fn remove(&mut self, color: Rgb, cx: &mut Context<Self>) {
        let daemon = cx.global_mut::<Daemon>();
        daemon.forget_color(color);
        if self.selected == color
            && let Some(next) = daemon.colors.latest()
        {
            self.selected = next;
        }
        cx.notify();
    }

    fn flash(&mut self, msg: String, cx: &mut Context<Self>) {
        self.toast = Some(msg.into());
        self._toast_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            let _ = this.update(cx, |this, cx| {
                this.toast = None;
                cx.notify();
            });
        }));
        cx.notify();
    }
}

impl Render for ColorEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let daemon = cx.global::<Daemon>();
        let history: Vec<Rgb> = daemon.colors.colors().to_vec();
        let default_format: Format = daemon.settings.picker.copy_format.parse().unwrap_or(Format::Hex);
        let selected = self.selected;

        // Left: history.
        let history_col = div()
            .id("history")
            .w(px(SWATCH + 24.))
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .py_2()
            .overflow_y_scroll()
            .border_r_1()
            .border_color(theme.border)
            .children(history.into_iter().enumerate().map(|(ix, c)| {
                div()
                    .id(("hist", ix))
                    .size(px(SWATCH))
                    .flex_shrink_0()
                    .rounded(theme.radius)
                    .cursor_pointer()
                    .bg(fill(c))
                    .border_2()
                    .border_color(if c == selected { theme.foreground } else { theme.border })
                    .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(c.to_hex()).build(window, cx))
                    .on_click(cx.listener(move |this, _, _, cx| this.select(c, cx)))
                    .on_mouse_down(gpui_kit::MouseButton::Right, cx.listener(move |this, _, _, cx| this.remove(c, cx)))
            }));

        // Top: shades, selected in the middle.
        let (lighter, darker) = selected.shades(2);
        let strip: Vec<(Rgb, bool)> = lighter
            .iter()
            .rev()
            .map(|c| (*c, false))
            .chain(std::iter::once((selected, true)))
            .chain(darker.iter().map(|c| (*c, false)))
            .collect();
        let shades = div().flex().h(px(96.)).rounded(theme.radius_lg).overflow_hidden().children(
            strip.into_iter().enumerate().map(|(ix, (c, is_selected))| {
                let text = if c.is_dark() { gpui_kit::white() } else { gpui_kit::black() };
                div()
                    .id(("shade", ix))
                    .h_full()
                    .flex()
                    .items_end()
                    .justify_center()
                    .pb_1()
                    .text_xs()
                    .text_color(text)
                    .bg(fill(c))
                    .when(is_selected, |d| d.flex_1().text_sm())
                    .when(!is_selected, |d| d.w(px(64.)).cursor_pointer().on_click(cx.listener(move |this, _, _, cx| this.adopt(c, cx))))
                    .child(c.to_hex())
            }),
        );

        // Formats.
        let rows = Format::ALL.into_iter().enumerate().map(|(ix, f)| {
            let is_default = f == default_format;
            div()
                .flex()
                .items_center()
                .gap_2()
                .h(px(32.))
                .child(
                    Button::new(("default", ix))
                        .ghost()
                        .icon(IconName::Star)
                        .selected(is_default)
                        .when(!is_default, |b| b.tooltip("Seçimde bu formatı kopyala"))
                        .when(is_default, |b| b.tooltip("Seçimde kopyalanan format"))
                        .on_click(cx.listener(move |this, _, _, cx| this.set_default_format(f, cx))),
                )
                .child(
                    div()
                        .w(px(72.))
                        .text_sm()
                        .text_color(if is_default { theme.foreground } else { theme.muted_foreground })
                        .child(f.label()),
                )
                .child(div().flex_1().text_sm().child(f.format(selected)))
                .child(
                    Button::new(("copy", ix))
                        .ghost()
                        .icon(IconName::Copy)
                        .tooltip("Kopyala")
                        .on_click(cx.listener(move |this, _, _, cx| this.copy(f, cx))),
                )
        });

        div()
            .size_full()
            .flex()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(history_col)
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_4()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().text_xl().child("Renk"))
                            .child(
                                Button::new("pick")
                                    .primary()
                                    .icon(IconName::Pipette)
                                    .label("Renk seç")
                                    .on_click(cx.listener(|_, _, _, cx| picker::toggle(cx))),
                            ),
                    )
                    .child(shades)
                    .child(div().flex().flex_col().children(rows))
                    .child(div().flex_1())
                    .child(
                        div()
                            .h(px(20.))
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(self.toast.clone().unwrap_or_else(|| {
                                "Sağ tık: geçmişten sil · ★: seçimde kopyalanacak format".into()
                            })),
                    ),
            )
    }
}
