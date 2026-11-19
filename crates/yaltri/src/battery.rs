//! Battery tool: one tray icon per supported mouse / earbuds showing its
//! level, refreshed every minute. Clicking one (or `yaltri toggle battery`)
//! opens a small card above the tray with each device, earbuds per bud.

use std::time::Duration;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _,
    Styled as _, Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, point, px, size,
};
use yaltri_platform::battery::{DeviceKind, Level, Reading};
use yaltri_platform::battery_icon::{Fill, Gauge, Tone};
use yaltri_platform::tray::BatteryBadge;

use crate::daemon::Daemon;
use crate::i18n::{t, tf};
use crate::theme::{self, R_LG, R_SM};
use crate::ui::{self, TextStyle as _};

/// How often the levels are read.
pub const POLL: Duration = Duration::from_secs(60);
/// At or below this the number turns red.
const LOW: u8 = 20;
/// How long the card stays up.
const CARD_FOR: Duration = Duration::from_secs(6);
const CARD_WIDTH: f32 = 300.;
const HEADER_HEIGHT: f32 = 44.;
const ROW_HEIGHT: f32 = 56.;
/// Gap between the card and the screen edge / taskbar.
const MARGIN: f32 = 12.;

pub fn device_icon(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Mouse => "mouse",
        DeviceKind::Earbuds | DeviceKind::Headset => "headphones",
        DeviceKind::Keyboard => "keyboard",
    }
}

pub fn percent(n: u8) -> String {
    tf("battery.percent", &[("n", &n.to_string())])
}

/// The line under a device's name: charging, or each bud.
pub fn detail(level: Level) -> Option<String> {
    match level {
        Level::Single { charging: true, .. } => Some(t("battery.charging").to_owned()),
        Level::Single { .. } => None,
        Level::Pair { left, right } => {
            let bud = |key: &str, v: Option<u8>| {
                tf(key, &[("value", &v.map_or_else(|| t("battery.in_case").to_owned(), percent))])
            };
            Some(format!("{} · {}", bud("battery.left", left), bud("battery.right", right)))
        }
    }
}

fn tone(level: Level) -> Tone {
    match level.headline() {
        _ if level.charging() => Tone::Charging,
        Some(n) if n <= LOW => Tone::Low,
        _ => Tone::Normal,
    }
}

/// How full the tray glyph is: each bud of a pair on its own.
fn gauge(kind: DeviceKind, level: Level) -> Gauge {
    let fill = |percent: u8, charging: bool| Fill {
        percent,
        tone: tone(Level::Single { percent, charging }),
    };
    let fills = match level {
        Level::Single { percent, charging } => [Some(fill(percent, charging)), None],
        Level::Pair { left, right } => [left.map(|p| fill(p, false)), right.map(|p| fill(p, false))],
    };
    Gauge { kind, number: level.headline().map(|n| (n, tone(level))), fills }
}

pub fn badge(reading: &Reading) -> BatteryBadge {
    let headline = reading.level.headline();
    let mut tooltip = reading.name.clone();
    if let Some(n) = headline {
        tooltip.push_str(&format!(" · {}", percent(n)));
    }
    if let Some(d) = detail(reading.level) {
        tooltip.push_str(&format!(" · {d}"));
    }
    BatteryBadge { gauge: gauge(reading.kind, reading.level), tooltip, open_label: t("settings.battery.tray_open").into() }
}

/// What a device's icon starts with, before its first reading.
pub fn placeholder(kind: DeviceKind) -> BatteryBadge {
    BatteryBadge {
        gauge: Gauge { kind, number: None, fills: [None, None] },
        tooltip: t("battery.title").into(),
        open_label: t("settings.battery.tray_open").into(),
    }
}

/// Reads every device in the background, then updates the tray icons and an
/// open card. Does nothing while the tool is off or a read is running.
pub fn refresh(cx: &mut App) {
    let daemon = cx.global_mut::<Daemon>();
    if !daemon.settings.battery.enabled || daemon.battery_busy {
        return;
    }
    daemon.battery_busy = true;
    let job = cx.background_executor().spawn(async { yaltri_platform::battery::read_all() });
    cx.spawn(async move |cx| {
        let readings = job.await;
        cx.update(|cx| {
            let daemon = cx.global_mut::<Daemon>();
            daemon.battery_busy = false;
            if daemon.settings.battery.enabled {
                daemon.battery_checked = true;
                daemon.set_battery(readings);
            }
            if let Some(card) = daemon.battery_window {
                let _ = card.update(cx, |_, window, _| window.refresh());
            }
        });
    })
    .detach();
}

/// Closes the card if it is up; says whether it was.
fn close(cx: &mut App) -> bool {
    let Some(old) = cx.global_mut::<Daemon>().battery_window.take() else {
        return false;
    };
    let _ = old.update(cx, |_, window, _| window.remove_window());
    true
}

/// Opens the card, or closes it when it is up.
pub fn toggle(cx: &mut App) {
    if close(cx) {
        return;
    }
    // Fresh numbers while the card is up.
    refresh(cx);
    let rows = cx.global::<Daemon>().battery.len().max(1);
    // The card fills its window: a popup's "transparent" background is drawn
    // dark on Windows, so the system draws the corners and shadow instead
    // (`ui::frame_popup`), as for search.
    let window_size = size(px(CARD_WIDTH), px(HEADER_HEIGHT + rows as f32 * ROW_HEIGHT + 4.));
    let mut bounds = Bounds::centered(None, window_size, cx);
    if let Some((area, near)) = tray_spot(cx) {
        let (x, y) = place((CARD_WIDTH, window_size.height.as_f32()), area, near);
        bounds.origin = point(px(x), px(y));
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
        app_id: Some("yaltri-battery".into()),
        window_background: WindowBackgroundAppearance::Opaque,
        ..Default::default()
    };
    let handle: AnyWindowHandle = match gpui_kit::open_window(options, cx, |_, cx| cx.new(|_| Card { framed_dark: None })) {
        Ok((handle, _)) => handle,
        Err(e) => {
            tracing::warn!("showing battery levels failed: {e:#}");
            return;
        }
    };
    cx.global_mut::<Daemon>().battery_window = Some(handle);
    cx.spawn(async move |cx| {
        cx.background_executor().timer(CARD_FOR).await;
        cx.update(|cx| {
            if cx.global::<Daemon>().battery_window == Some(handle) {
                cx.global_mut::<Daemon>().battery_window = None;
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            }
        });
    })
    .detach();
}

/// x, y, width, height in logical px.
type Rect = (f32, f32, f32, f32);

/// The area the card may cover and a point by the tray icon that was
/// clicked, in logical px.
fn tray_spot(cx: &App) -> Option<(Rect, (f32, f32))> {
    let display = cx.primary_display()?;
    let rect = |b: Bounds<gpui_kit::Pixels>| {
        (b.origin.x.as_f32(), b.origin.y.as_f32(), b.size.width.as_f32(), b.size.height.as_f32())
    };
    let whole = rect(display.bounds());
    // X11: the pointer is on the icon or the menu just under it, whichever
    // corner or edge the panel is on.
    #[cfg(target_os = "linux")]
    if cx.global::<Daemon>().session == yaltri_platform::session::Session::X11 {
        match yaltri_platform::x11::pointer_area() {
            Ok(pa) => {
                // The X screen spans every monitor; GPUI's display is the
                // same, in logical px.
                let scale = f32::from(pa.screen_width) / whole.2.max(1.);
                let (x, y, w, h) = pa.area;
                let area = (x as f32 / scale, y as f32 / scale, w as f32 / scale, h as f32 / scale);
                return Some((area, (pa.pointer.0 as f32 / scale, pa.pointer.1 as f32 / scale)));
            }
            Err(e) => tracing::debug!("reading the pointer failed: {e:#}"),
        }
    }
    // Otherwise the tray is at the end of the taskbar, on the edge the work
    // area leaves out: the right end of a top or bottom bar, the bottom of
    // a side one. GNOME's top bar is not left out on Wayland, so assume it.
    let area = rect(display.visible_bounds());
    let (x, y, w, h) = area;
    let right = x + w;
    let bottom = y + h;
    let near = if x > whole.0 {
        (x, bottom)
    } else if right < whole.0 + whole.2 {
        (right, bottom)
    } else if y > whole.1 || gnome() {
        (right, y)
    } else {
        (right, bottom)
    };
    Some((area, near))
}

fn gnome() -> bool {
    cfg!(target_os = "linux")
        && std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.split(':').any(|d| d.eq_ignore_ascii_case("gnome")))
}

/// Where a `card` (width, height) goes in `area`: against the edge nearest
/// `near`, lined up with it along that edge, never past the area.
fn place(card: (f32, f32), area: Rect, near: (f32, f32)) -> (f32, f32) {
    let (x, y, w, h) = area;
    let (px_, py) = (near.0.clamp(x, x + w), near.1.clamp(y, y + h));
    let clamp_x = |v: f32| v.min(x + w - card.0 - MARGIN).max(x + MARGIN);
    let clamp_y = |v: f32| v.min(y + h - card.1 - MARGIN).max(y + MARGIN);
    let (left, right, top, bottom) = (px_ - x, x + w - px_, py - y, y + h - py);
    let nearest = left.min(right).min(top).min(bottom);
    if nearest == top {
        (clamp_x(px_ - card.0 / 2.), y + MARGIN)
    } else if nearest == bottom {
        (clamp_x(px_ - card.0 / 2.), y + h - card.1 - MARGIN)
    } else if nearest == left {
        (x + MARGIN, clamp_y(py - card.1 / 2.))
    } else {
        (x + w - card.0 - MARGIN, clamp_y(py - card.1 / 2.))
    }
}

struct Card {
    /// Theme the system frame was drawn for.
    framed_dark: Option<bool>,
}

impl Render for Card {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::of(window);
        if self.framed_dark != Some(p.dark) {
            self.framed_dark = Some(p.dark);
            ui::frame_popup(window, p);
        }
        ui::round_corners(window, R_LG);
        let readings = cx.global::<Daemon>().battery.clone();
        let empty = readings.is_empty();
        let checked = cx.global::<Daemon>().battery_checked;
        let rows = readings.into_iter().map(|r| {
            let low = r.level.headline().is_some_and(|n| n <= LOW) && !r.level.charging();
            div()
                .h(px(ROW_HEIGHT))
                .flex()
                .items_center()
                .gap(px(12.))
                .border_t_1()
                .border_color(p.border)
                .child(ui::icon(device_icon(r.kind), 20., p.foreground))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().ts(13., 18.).text_color(p.foreground).truncate().child(r.name.clone()))
                        .when_some(detail(r.level), |d, text| {
                            d.child(div().ts(12., 16.).text_color(p.muted).truncate().child(text))
                        }),
                )
                .when_some(r.level.headline(), |d, n| {
                    d.child(
                        div()
                            .ts(16., 20.)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if low {
                                p.danger
                            } else if r.level.charging() {
                                p.success
                            } else {
                                p.foreground
                            })
                            .child(percent(n)),
                    )
                })
        });
        // Linux: X11 cuts the window to the rounded card (`ui::round_corners`);
        // the border color behind it keeps the cut edge clean.
        div().size_full().font_family(theme::ui_font()).when(cfg!(target_os = "linux"), |d| d.bg(p.border)).child(
            div()
                .size_full()
                .px(px(16.))
                .flex()
                .flex_col()
                .bg(p.surface)
                .when(!cfg!(windows), |card| card.rounded(px(R_LG)).border_1().border_color(p.border))
                .child(
                    div()
                        .h(px(HEADER_HEIGHT))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(ui::icon("battery-medium", 16., p.muted))
                        .child(
                            div()
                                .flex_1()
                                .ts(13., 18.)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.foreground)
                                .child(t("battery.title")),
                        )
                        .child(
                            div()
                                .id("battery-close")
                                .size(px(24.))
                                .mr(px(-4.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(R_SM))
                                .cursor_pointer()
                                .hover(|s| s.bg(p.list_hover))
                                .child(ui::icon("x", 14., p.muted))
                                // Inside the card's own window: close it directly.
                                .on_click(|_, window, cx| {
                                    cx.global_mut::<Daemon>().battery_window = None;
                                    window.remove_window();
                                }),
                        ),
                )
                .children(rows)
                .when(empty, |d| {
                    d.child(
                        div()
                            .h(px(ROW_HEIGHT))
                            .flex()
                            .items_center()
                            .border_t_1()
                            .border_color(p.border)
                            .ts(13., 18.)
                            .text_color(p.muted)
                            .child(if checked { t("battery.none") } else { t("battery.searching") }),
                    )
                }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn earbuds_show_each_bud_and_the_lower_one() {
        // The UI language is global and other tests switch it: check only
        // what holds in every language.
        let reading = Reading {
            kind: DeviceKind::Earbuds,
            name: "Arctis GameBuds".into(),
            level: Level::Pair { left: Some(57), right: None },
        };
        let detail = detail(reading.level).unwrap();
        let (left, right) = detail.split_once(" · ").unwrap();
        assert!(left.contains("57") && !right.chars().any(|c| c.is_ascii_digit()), "{detail}");
        let b = badge(&reading);
        assert_eq!(b.gauge.number, Some((57, Tone::Normal)));
        assert_eq!(b.gauge.fills, [Some(Fill { percent: 57, tone: Tone::Normal }), None], "the docked bud stays empty");
        assert!(b.tooltip.starts_with("Arctis GameBuds · ") && b.tooltip.matches("57").count() == 2, "{}", b.tooltip);
    }

    #[test]
    fn card_opens_by_the_tray_in_every_corner() {
        let area = (0., 0., 1920., 1080.);
        let card = (300., 200.);
        let m = MARGIN;
        // Top bar, icon at the right end (GNOME).
        assert_eq!(place(card, area, (1900., 5.)), (1920. - 300. - m, m));
        // Bottom bar, right end (Windows, KDE).
        assert_eq!(place(card, area, (1900., 1075.)), (1920. - 300. - m, 1080. - 200. - m));
        // Top left and bottom left.
        assert_eq!(place(card, area, (10., 3.)), (m, m));
        assert_eq!(place(card, area, (10., 1078.)), (m, 1080. - 200. - m));
        // Mid bar: under the icon.
        assert_eq!(place(card, area, (1000., 2.)), (850., m));
        // Side panel on the right: by the icon's height.
        assert_eq!(place(card, area, (1915., 500.)), (1920. - 300. - m, 400.));
        // Offset monitor.
        assert_eq!(place(card, (1920., 32., 1920., 1048.), (3830., 40.)), (3840. - 300. - m, 32. + m));
    }

    #[test]
    fn charging_wins_over_low() {
        assert_eq!(tone(Level::Single { percent: 5, charging: true }), Tone::Charging);
        assert_eq!(tone(Level::Single { percent: 20, charging: false }), Tone::Low);
        assert_eq!(tone(Level::Single { percent: 21, charging: false }), Tone::Normal);
    }
}
