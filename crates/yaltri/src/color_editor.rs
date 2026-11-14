//! Color editor, shown after a pick: history rail on the left, a bar of
//! lighter/darker shades on top, and every text format with a copy button.
//! The default copy format is chosen in Settings; its row is tagged here.

use std::time::{Duration, Instant};

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, BoxShadow, ClipboardItem, Context, FontWeight, InteractiveElement as _,
    IntoElement, KeystrokeEvent, MouseButton, MouseDownEvent, ParentElement as _, Pixels, Point, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, anchored, deferred, div, point, px, rgb,
};
use yaltri_color::{Format, Rgb};

use crate::daemon::Daemon;
use crate::i18n::{t, tf};
use crate::picker;
use crate::theme::{self, Palette, R_MD, R_SM, R_XS};
use crate::ui::{self, ButtonKind, TextStyle as _};

const WIDTH: f32 = 620.;
/// 520 in the spec; the system frame takes a few px of it on Windows.
const HEIGHT: f32 = 532.;

const TOAST_IN: f32 = 180.;
const TOAST_VISIBLE: f32 = 1600.;
const TOAST_OUT: f32 = 180.;

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

    let options = ui::frameless_window(t("editor.title").into(), WIDTH, HEIGHT, cx);
    match gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| ColorEditor::new(color, window, cx))) {
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

fn fill(c: Rgb) -> gpui_kit::Rgba {
    rgb(u32::from_be_bytes([0, c.r, c.g, c.b]))
}

/// `c` mixed towards white (`t` > 0) or black (`t` < 0), like the spec's
/// "white 60 %" / "black 20 %" shades.
fn shade(c: Rgb, t: f32) -> Rgb {
    let target = if t > 0. { 255. } else { 0. };
    let k = t.abs();
    let mix = |v: u8| (v as f32 + (target - v as f32) * k).round().clamp(0., 255.) as u8;
    Rgb::new(mix(c.r), mix(c.g), mix(c.b))
}

/// Ring drawn as two spread shadows: a gap in `gap` color, then `ring`.
fn ring(gap: gpui_kit::Hsla, ring: gpui_kit::Hsla) -> Vec<BoxShadow> {
    let spread = |color, by: f32| BoxShadow {
        color,
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(by),
        inset: false,
    };
    // Later shadows paint below earlier ones.
    vec![spread(gap, 2.), spread(ring, 4.)]
}

struct Toast {
    label: SharedString,
    value: Option<SharedString>,
    shown: Instant,
}

/// Right-click menu on a history swatch.
struct Menu {
    color: Rgb,
    at: Point<Pixels>,
}

pub struct ColorEditor {
    selected: Rgb,
    hovered_shade: Option<usize>,
    menu: Option<Menu>,
    /// Open "clear history?" dialog; the focused button (0 cancel, 1 clear).
    confirm: Option<usize>,
    toast: Option<Toast>,
    /// The format whose copy button shows a check.
    copied: Option<Format>,
    _toast_timer: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl ColorEditor {
    fn new(selected: Rgb, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let this_window = window.window_handle();
        let weak = cx.weak_entity();
        let keys = cx.intercept_keystrokes(move |event: &KeystrokeEvent, window, cx| {
            if window.window_handle() != this_window {
                return;
            }
            let handled = weak.update(cx, |this, cx| this.on_key(event, window, cx)).unwrap_or(false);
            if handled {
                cx.stop_propagation();
            }
        });
        Self {
            selected,
            hovered_shade: None,
            menu: None,
            confirm: None,
            toast: None,
            copied: None,
            _toast_timer: None,
            _subscriptions: vec![keys, theme::follow_system(window, cx)],
        }
    }

    fn on_key(&mut self, event: &KeystrokeEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let key = event.keystroke.key.as_str();
        if let Some(focus) = self.confirm {
            match key {
                "escape" => self.confirm = None,
                "enter" => {
                    if focus == 1 {
                        self.clear_history(cx);
                    }
                    self.confirm = None;
                }
                "tab" | "left" | "right" => self.confirm = Some(1 - focus),
                _ => return false,
            }
            cx.notify();
            return true;
        }
        if key == "escape" {
            if self.menu.take().is_none() {
                window.remove_window();
            }
            cx.notify();
            return true;
        }
        false
    }

    fn copy_text(&mut self, text: String, format: Option<Format>, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        self.copied = format;
        self.show_toast(t("editor.copied").into(), Some(text.into()), cx);
    }

    fn copy(&mut self, format: Format, cx: &mut Context<Self>) {
        self.copy_text(format.format(self.selected), Some(format), cx);
    }

    fn show_toast(&mut self, label: SharedString, value: Option<SharedString>, cx: &mut Context<Self>) {
        self.toast = Some(Toast { label, value, shown: Instant::now() });
        self._toast_timer = Some(cx.spawn(async move |this, cx| {
            // Wake for the fade-out, then drop the toast.
            cx.background_executor().timer(Duration::from_millis((TOAST_IN + TOAST_VISIBLE) as u64)).await;
            let _ = this.update(cx, |_, cx| cx.notify());
            cx.background_executor().timer(Duration::from_millis(TOAST_OUT as u64)).await;
            let _ = this.update(cx, |this, cx| {
                this.toast = None;
                this.copied = None;
                cx.notify();
            });
        }));
        cx.notify();
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

    fn clear_history(&mut self, cx: &mut Context<Self>) {
        cx.global_mut::<Daemon>().clear_colors();
        cx.notify();
    }

    fn history_rail(&self, history: &[Rgb], p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("history")
            .w(px(64.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.))
            .py(px(12.))
            .overflow_y_scroll()
            .border_r_1()
            .border_t_1()
            .border_color(p.border)
            .rounded_bl(px(ui::FRAME_INNER_RADIUS))
            .bg(p.background)
            .children(history.iter().copied().enumerate().map(|(ix, c)| {
                div()
                    .id(("hist", ix))
                    .size(px(36.))
                    .flex_none()
                    .rounded(px(R_MD))
                    .cursor_pointer()
                    .bg(fill(c))
                    .border_1()
                    .border_color(p.swatch_border())
                    .when(c == self.selected, |d| d.shadow(ring(p.background, p.accent_strong)))
                    .on_click(cx.listener(move |this, _, _, cx| this.select(c, cx)))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                            this.menu = Some(Menu { color: c, at: e.position });
                            cx.notify();
                        }),
                    )
            }))
    }

    fn header(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let c = self.selected;
        div()
            .h(px(32.))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                div()
                    .size(px(28.))
                    .flex_none()
                    .rounded(px(R_SM))
                    .bg(fill(c))
                    .border_1()
                    .border_color(p.swatch_border()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().ts(15., 20.).font_weight(FontWeight::SEMIBOLD).truncate().child(c.name()))
                    .child(div().ts(11., 14.).font_family(theme::mono_font()).text_color(p.muted).child(c.to_hex())),
            )
            .child(
                ui::button("pick", ButtonKind::Secondary, p)
                    .gap(px(8.))
                    .child(ui::icon("pipette", 16., p.foreground))
                    .child(t("editor.pick"))
                    .on_click(cx.listener(|_, _, _, cx| picker::toggle(cx))),
            )
    }

    fn shade_bar(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let c = self.selected;
        let steps: [(f32, bool); 7] =
            [(0.6, false), (0.4, false), (0.2, false), (0., true), (-0.2, false), (-0.4, false), (-0.6, false)];
        div().h(px(72.)).flex_none().flex().items_center().gap(px(6.)).px(px(4.)).children(
            steps.into_iter().enumerate().map(|(ix, (k, picked))| {
                let color = if picked { c } else { shade(c, k) };
                let hovered = !picked && self.hovered_shade == Some(ix);
                let mut seg = div()
                    .id(("shade", ix))
                    .relative()
                    .flex_basis(px(0.))
                    .h(px(if picked { 64. } else { 52. }))
                    .rounded(px(R_SM))
                    .bg(fill(color))
                    .border_1()
                    .border_color(if hovered { p.foreground } else { p.swatch_border() });
                seg.style().flex_grow = Some(if picked { 1.5 } else { 1. });
                seg.style().flex_shrink = Some(1.);
                if picked {
                    return seg.shadow(ring(p.surface, p.foreground)).into_any_element();
                }
                seg.cursor_pointer()
                    .when(hovered, |d| {
                        d.top(px(-3.)).shadow(p.shadow_md.clone()).child(
                            div().absolute().top(px(-30.)).left_0().right_0().flex().justify_center().child(
                                div()
                                    .h(px(22.))
                                    .px(px(8.))
                                    .flex()
                                    .items_center()
                                    .rounded(px(R_SM))
                                    .bg(p.toast_bg)
                                    .text_color(p.toast_fg)
                                    .font_family(theme::mono_font())
                                    .text_size(px(11.))
                                    .whitespace_nowrap()
                                    .child(color.to_hex()),
                            ),
                        )
                    })
                    .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                        if *over {
                            this.hovered_shade = Some(ix);
                        } else if this.hovered_shade == Some(ix) {
                            this.hovered_shade = None;
                        }
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| this.adopt(color, cx)))
                    .into_any_element()
            }),
        )
    }

    fn format_rows(&self, default_format: Format, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        div().flex().flex_col().gap(px(2.)).children(Format::ALL.into_iter().enumerate().map(|(ix, f)| {
            let copied = self.copied == Some(f);
            div()
                .id(("format", ix))
                .h(px(30.))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(8.))
                .pl(px(10.))
                .pr(px(6.))
                .rounded(px(R_SM))
                .cursor_pointer()
                .hover(|s| s.bg(p.list_hover))
                .child(div().w(px(72.)).flex_none().text_size(px(13.)).text_color(p.muted).truncate().child(f.label()))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_family(theme::mono_font())
                        .text_size(px(13.))
                        .text_color(p.foreground)
                        .child(f.format(self.selected)),
                )
                .when(f == default_format, |d| {
                    d.child(
                        div()
                            .h(px(18.))
                            .px(px(6.))
                            .flex()
                            .flex_none()
                            .items_center()
                            .rounded(px(R_XS))
                            .bg(p.list_active)
                            .text_color(p.foreground)
                            .text_size(px(11.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(t("editor.default_tag")),
                    )
                })
                .child(div().flex_1())
                .child(
                    div()
                        .size(px(28.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(R_SM))
                        .child(if copied { ui::icon("check", 16., p.success) } else { ui::icon("copy", 16., p.muted) }),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.copy(f, cx)))
        }))
    }

    fn toast_view(&self, toast: &Toast, p: &Palette, window: &mut Window) -> Option<AnyElement> {
        let ms = toast.shown.elapsed().as_secs_f32() * 1000.;
        let (opacity, dy) = if ms < TOAST_IN {
            let k = ms / TOAST_IN;
            (k, 8. * (1. - k))
        } else if ms < TOAST_IN + TOAST_VISIBLE {
            (1., 0.)
        } else {
            let k = ((ms - TOAST_IN - TOAST_VISIBLE) / TOAST_OUT).min(1.);
            (1. - k, 0.)
        };
        if !(TOAST_IN..=TOAST_IN + TOAST_VISIBLE).contains(&ms) {
            window.request_animation_frame();
        }
        Some(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(px(40. - dy))
                .flex()
                .justify_center()
                .opacity(opacity)
                .child(ui::toast(toast.label.clone(), toast.value.clone(), p))
                .into_any_element(),
        )
    }

    fn menu_view(&self, menu: &Menu, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let c = menu.color;
        let close_then = |f: fn(&mut Self, Rgb, &mut Context<Self>)| {
            cx.listener(move |this: &mut Self, _, _, cx| {
                this.menu = None;
                f(this, c, cx);
                cx.notify();
            })
        };
        let body = ui::menu_box(p)
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.menu = None;
                cx.notify();
            }))
            .child(
                ui::menu_item(
                    "menu-copy",
                    "copy",
                    tf("editor.menu.copy", &[("format", Format::Hex.label())]),
                    false,
                    p,
                )
                .on_click(close_then(|this, c, cx| this.copy_text(Format::Hex.format(c), None, cx))),
            )
            .child(
                ui::menu_item("menu-edit", "pencil", t("editor.menu.edit"), false, p)
                    .on_click(close_then(|this, c, cx| this.select(c, cx))),
            )
            .child(ui::menu_separator(p))
            .child(
                ui::menu_item("menu-delete", "trash-2", t("editor.menu.delete"), true, p)
                    .on_click(close_then(|this, c, cx| this.remove(c, cx))),
            )
            .child(
                ui::menu_item("menu-clear", "eraser", t("editor.menu.clear"), false, p)
                    .on_click(close_then(|this, _, _| this.confirm = Some(0))),
            );
        deferred(anchored().position(menu.at).snap_to_window().child(body)).with_priority(1).into_any_element()
    }

    fn confirm_view(&self, focus: usize, count: usize, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let focused = |d: gpui_kit::Stateful<gpui_kit::Div>, on: bool| {
            d.px(px(14.)).when(on, |d| d.shadow(ring(p.surface, p.accent_strong)))
        };
        div()
            .id("confirm-scrim")
            .absolute()
            .top(px(ui::TITLEBAR_HEIGHT))
            .left_0()
            .right_0()
            .bottom_0()
            // Keeps clicks and hovers from reaching the editor behind it.
            .occlude()
            .rounded_b(px(ui::FRAME_INNER_RADIUS))
            .bg(p.scrim())
            .flex()
            .items_center()
            .justify_center()
            .child(
                ui::dialog_box(p)
                    .child(div().ts(15., 20.).font_weight(FontWeight::SEMIBOLD).child(t("editor.clear.title")))
                    .child(
                        div()
                            .ts(13., 19.)
                            .text_color(p.muted)
                            .child(tf("editor.clear.body", &[("count", &count.to_string())])),
                    )
                    .child(
                        div()
                            .mt(px(12.))
                            .flex()
                            .justify_end()
                            .gap(px(8.))
                            .child(
                                focused(ui::button("cancel", ButtonKind::Secondary, p), focus == 0)
                                    .child(t("common.cancel"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.confirm = None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                focused(ui::button("clear", ButtonKind::Danger, p), focus == 1)
                                    .child(t("editor.clear.confirm"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.confirm = None;
                                        this.clear_history(cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }
}

impl Render for ColorEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::of(window);
        let daemon = cx.global::<Daemon>();
        let history: Vec<Rgb> = daemon.colors.colors().to_vec();
        let default_format: Format = daemon.settings.picker.copy_format.parse().unwrap_or(Format::Hex);

        let main = div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .gap(px(12.))
            .pt(px(16.))
            .px(px(20.))
            .pb(px(16.))
            .rounded_br(px(ui::FRAME_INNER_RADIUS))
            .bg(p.surface)
            .border_t_1()
            .border_color(p.border)
            .child(self.header(p, cx))
            .child(self.shade_bar(p, cx))
            .child(self.format_rows(default_format, p, cx))
            .child(div().flex_1())
            .child(div().ts(11., 14.).text_color(p.muted).truncate().child(t("editor.hint")));
        let toast = self.toast.as_ref().and_then(|toast| self.toast_view(toast, p, window));
        let main = main.children(toast);

        let body = div().flex_1().min_h_0().flex().child(self.history_rail(&history, p, cx)).child(main);
        let menu = self.menu.as_ref().map(|m| self.menu_view(m, p, cx));
        let confirm = self.confirm.map(|focus| self.confirm_view(focus, history.len(), p, cx));

        window.set_window_title(t("editor.title"));
        ui::window_frame(t("editor.title").into(), p, body).relative().children(menu).children(confirm)
    }
}
