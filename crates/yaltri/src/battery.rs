//! Battery tool: one tray icon per supported mouse / earbuds showing its
//! level, refreshed every minute. Clicking one (or `yaltri toggle battery`)
//! opens a small card above the tray with each device, earbuds per bud.

use std::time::Duration;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, FontWeight, IntoElement, ParentElement as _, Render,
    Styled as _, Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, point, px, size,
};
use yaltri_platform::battery::{DeviceKind, Level, Reading};
use yaltri_platform::icon::Tone;
use yaltri_platform::tray::BatteryBadge;

use crate::daemon::Daemon;
use crate::i18n::{t, tf};
use crate::theme::{self, R_LG};
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
        DeviceKind::Earbuds => "headphones",
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

pub fn badge(reading: &Reading) -> BatteryBadge {
    let headline = reading.level.headline();
    let mut tooltip = reading.name.clone();
    if let Some(n) = headline {
        tooltip.push_str(&format!(" · {}", percent(n)));
    }
    if let Some(d) = detail(reading.level) {
        tooltip.push_str(&format!(" · {d}"));
    }
    BatteryBadge {
        text: headline.map_or_else(|| "-".to_owned(), |n| n.to_string()),
        tone: tone(reading.level),
        tooltip,
        ..placeholder(reading.kind)
    }
}

/// What a device's icon starts with, before its first reading.
pub fn placeholder(kind: DeviceKind) -> BatteryBadge {
    BatteryBadge {
        device: kind,
        text: "-".into(),
        tone: Tone::Normal,
        tooltip: t("battery.title").into(),
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
                daemon.set_battery(readings);
            }
            if let Some(card) = daemon.battery_window {
                let _ = card.update(cx, |_, window, _| window.refresh());
            }
        });
    })
    .detach();
}

/// Opens the card, or closes it when it is up.
pub fn toggle(cx: &mut App) {
    if let Some(old) = cx.global_mut::<Daemon>().battery_window.take() {
        let _ = old.update(cx, |_, window, _| window.remove_window());
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
    if let Some(display) = cx.primary_display() {
        // Above the tray: the bottom right of the area the taskbar leaves.
        let area = display.visible_bounds();
        bounds.origin = point(
            area.origin.x + area.size.width - window_size.width - px(MARGIN),
            area.origin.y + area.size.height - window_size.height - px(MARGIN),
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
                                .ts(13., 18.)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.foreground)
                                .child(t("battery.title")),
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
                            .child(t("battery.none")),
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
        assert_eq!(b.text, "57");
        assert_eq!(b.tone, Tone::Normal);
        assert!(b.tooltip.starts_with("Arctis GameBuds · ") && b.tooltip.matches("57").count() == 2, "{}", b.tooltip);
    }

    #[test]
    fn charging_wins_over_low() {
        assert_eq!(tone(Level::Single { percent: 5, charging: true }), Tone::Charging);
        assert_eq!(tone(Level::Single { percent: 20, charging: false }), Tone::Low);
        assert_eq!(tone(Level::Single { percent: 21, charging: false }), Tone::Normal);
    }
}
