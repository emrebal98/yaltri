//! Settings window: one section per tool (its master switch in the header,
//! everything that belongs to it in one card), then "About". Shortcuts and
//! master switches apply at once; the rest is staged until "Save".

use std::time::{Duration, SystemTime};

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Bounds, Context, Div, Entity, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Render, ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _,
    Task, Window, div, img, px,
};
use yaltri_apps::games::Launcher;
use yaltri_color::Format;
use yaltri_core::{Settings, ToolId};
use yaltri_files::Phase;
use yaltri_platform::hotkeys::{self, HotkeyErrorKind};
use yaltri_platform::keys;

use crate::daemon::{Daemon, is_implemented};
use crate::i18n::{t, tf};
use crate::shortcut_input::{ShortcutChanged, ShortcutInput};
use crate::theme::{self, Palette, R_MD, R_SM, ThemeChoice};
use crate::ui::{self, ButtonKind, TextStyle as _};
use crate::update::{self, UpdateState};

const WIDTH: f32 = 560.;
const HEIGHT: f32 = 820.;
const TOAST_FOR: Duration = Duration::from_millis(1960);

pub fn open(cx: &mut App) {
    open_at(None, cx);
}

/// Opens settings scrolled to `tool`'s section (the top when `None`).
pub fn open_at(tool: Option<ToolId>, cx: &mut App) {
    if let Some((handle, view)) = cx.global::<Daemon>().settings_window.clone()
        && cx.windows().contains(&handle)
    {
        // The window's root is gpui-kit's wrapper, so reach the view
        // through the handle kept at open time.
        if let Some(tool) = tool {
            let _ = view.update(cx, |view, cx| {
                view.scroll_to = Some(tool);
                cx.notify();
            });
        }
        let _ = handle.update(cx, |_, window, _| window.activate_window());
        return;
    }

    let options = ui::frameless_window(t("settings.title").into(), WIDTH, HEIGHT, cx);
    let build = move |window: &mut Window, cx: &mut App| {
        cx.new(|cx| {
            let mut view = SettingsView::new(window, cx);
            view.scroll_to = tool;
            view
        })
    };
    match gpui_kit::open_window(options, cx, build) {
        Ok((handle, view)) => {
            cx.global_mut::<Daemon>().settings_window = Some((handle, view.downgrade()));
            let _ = handle.update(cx, |_, window, _| window.activate_window());
            cx.activate(true);
        }
        Err(e) => tracing::error!("opening settings failed: {e:#}"),
    }
}

/// Localized tool name.
pub fn tool_name(tool: ToolId) -> &'static str {
    match tool {
        ToolId::Launcher => t("tool.launcher"),
        ToolId::Picker => t("tool.picker"),
    }
}

struct ToolRow {
    tool: ToolId,
    enabled: bool,
    hotkey: Entity<ShortcutInput>,
    /// Why the last recorded shortcut was not taken.
    refused: Option<SharedString>,
}

pub struct SettingsView {
    rows: Vec<ToolRow>,
    files_enabled: bool,
    apps_enabled: bool,
    games: yaltri_core::settings::GamesSettings,
    picker_open_editor: bool,
    copy_format: Format,
    format_menu: bool,
    /// Where the dropdown triggers were last painted.
    format_trigger: Option<Bounds<Pixels>>,
    language_trigger: Option<Bounds<Pixels>>,
    /// The real OS startup entry, read when the window opens.
    autostart: bool,
    autostart_error: Option<SharedString>,
    /// `general.language`: "auto", "tr", "en".
    language: String,
    language_menu: bool,
    theme: ThemeChoice,
    /// Startup warning (settings file was unreadable).
    warning: Option<SharedString>,
    /// (`is_error`, text).
    toast: Option<(bool, SharedString)>,
    update: UpdateState,
    scroll: ScrollHandle,
    /// Section to bring to the top on the next render.
    scroll_to: Option<ToolId>,
    _toast_timer: Option<Task<()>>,
    _refresh: Task<()>,
    _subscriptions: Vec<gpui_kit::Subscription>,
}

impl SettingsView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let daemon = cx.global::<Daemon>();
        let settings = daemon.settings.clone();
        let warning = daemon.settings_warning.clone().map(|e| tf("settings.recovered", &[("error", &e)]).into());
        let mut subscriptions = vec![theme::follow_system(window, cx)];
        let rows = ToolId::ALL
            .into_iter()
            .map(|tool| {
                let value = settings.hotkey(tool).to_owned();
                let hotkey = cx.new(|cx| ShortcutInput::new(tool.as_str(), value, window, cx));
                // A recorded shortcut takes effect immediately.
                subscriptions.push(cx.subscribe(&hotkey, |this: &mut Self, _, _: &ShortcutChanged, cx| this.save(cx)));
                ToolRow { tool, enabled: settings.is_enabled(tool), hotkey, refused: None }
            })
            .collect();
        // Re-render once a second so the index status stays live.
        let refresh = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        });
        Self {
            rows,
            files_enabled: settings.files.files_enabled,
            apps_enabled: settings.files.apps_enabled,
            games: settings.games.clone(),
            picker_open_editor: settings.picker.open_editor,
            copy_format: settings.picker.copy_format.parse().unwrap_or(Format::Hex),
            format_menu: false,
            format_trigger: None,
            language_trigger: None,
            autostart: yaltri_platform::autostart::is_enabled(),
            autostart_error: None,
            language: settings.general.language.clone(),
            language_menu: false,
            theme: ThemeChoice::from_key(&settings.general.theme),
            warning,
            toast: None,
            update: UpdateState::Idle,
            _toast_timer: None,
            scroll: ScrollHandle::new(),
            scroll_to: None,
            _refresh: refresh,
            _subscriptions: subscriptions,
        }
    }

    fn row(&self, tool: ToolId) -> &ToolRow {
        self.rows.iter().find(|r| r.tool == tool).expect("one row per tool")
    }

    fn row_mut(&mut self, tool: ToolId) -> &mut ToolRow {
        self.rows.iter_mut().find(|r| r.tool == tool).expect("one row per tool")
    }

    fn flash(&mut self, is_error: bool, text: SharedString, cx: &mut Context<Self>) {
        self.toast = Some((is_error, text));
        self._toast_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TOAST_FOR).await;
            let _ = this.update(cx, |this, cx| {
                this.toast = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let previous: Settings = cx.global::<Daemon>().settings.clone();
        let mut settings = previous.clone();
        for row in &mut self.rows {
            row.refused = None;
            let spec = row.hotkey.read(cx).value().to_owned();
            if row.enabled && !spec.is_empty() && hotkeys::parse(&spec).is_err() {
                row.refused = Some(t("settings.shortcut.invalid").into());
                cx.notify();
                return;
            }
            settings.set_enabled(row.tool, row.enabled);
            settings.set_hotkey(row.tool, spec);
        }
        settings.files.files_enabled = self.files_enabled;
        settings.files.apps_enabled = self.apps_enabled;
        settings.games = self.games.clone();
        settings.picker.open_editor = self.picker_open_editor;
        settings.picker.copy_format = self.copy_format.key().into();
        settings.general.language = self.language.clone();
        settings.general.theme = self.theme.key().into();
        let looks_changed =
            settings.general.language != previous.general.language || settings.general.theme != previous.general.theme;

        // Two tools on one shortcut: keep the second one's old shortcut.
        if let Some((a, b)) = settings.hotkey_conflicts().first().copied() {
            let old = previous.hotkey(b).to_owned();
            let row = self.row_mut(b);
            row.refused = Some(tf("settings.shortcut.same_as", &[("tool", tool_name(a))]).into());
            row.hotkey.update(cx, |h, cx| h.set_value(old, cx));
            cx.notify();
            return;
        }

        if let Err(e) = cx.global_mut::<Daemon>().update_settings(settings.clone()) {
            self.flash(true, tf("settings.save_failed", &[("error", &format!("{e:#}"))]).into(), cx);
            return;
        }
        // Language and theme apply to every open window right away.
        if looks_changed {
            theme::sync_components(cx);
            cx.refresh_windows();
        }

        // A new shortcut another program holds: go back to the old one.
        let in_use: Vec<ToolId> = cx
            .global::<Daemon>()
            .hotkey_errors
            .iter()
            .filter(|e| e.kind == HotkeyErrorKind::InUse && settings.hotkey(e.tool) != previous.hotkey(e.tool))
            .map(|e| e.tool)
            .collect();
        if !in_use.is_empty() {
            for tool in &in_use {
                let old = previous.hotkey(*tool).to_owned();
                settings.set_hotkey(*tool, old.clone());
                let shown = keys::display_parts(&old).join(" + ");
                let row = self.row_mut(*tool);
                row.refused = Some(if old.is_empty() {
                    t("settings.shortcut.in_use").into()
                } else {
                    tf("settings.shortcut.in_use_kept", &[("old", &shown)]).into()
                });
                row.hotkey.update(cx, |h, cx| h.set_value(old, cx));
            }
            let _ = cx.global_mut::<Daemon>().update_settings(settings);
            cx.notify();
            return;
        }
        self.flash(false, t("settings.saved").into(), cx);
    }

    /// Writes or removes the OS startup entry. On failure the switch stays
    /// as it was and the error shows under the row.
    fn set_autostart(&mut self, on: bool, cx: &mut Context<Self>) {
        match yaltri_platform::autostart::set_enabled(on) {
            Ok(()) => {
                self.autostart = yaltri_platform::autostart::is_enabled();
                self.autostart_error = None;
                // Keep the config in step (the OS entry stays the truth).
                let daemon = cx.global_mut::<Daemon>();
                let mut settings = daemon.settings.clone();
                settings.general.start_with_system = on;
                if let Err(e) = daemon.update_settings(settings) {
                    tracing::warn!("saving start_with_system failed: {e:#}");
                }
            }
            Err(e) => {
                self.autostart_error = Some(tf("settings.general.autostart_failed", &[("error", &format!("{e:#}"))]).into());
            }
        }
        cx.notify();
    }

    /// Master switch: applies right away so the shortcut is (un)registered.
    fn set_tool_enabled(&mut self, tool: ToolId, on: bool, cx: &mut Context<Self>) {
        self.row_mut(tool).enabled = on;
        self.save(cx);
    }

    fn check_updates(&mut self, cx: &mut Context<Self>) {
        if self.update == UpdateState::Checking {
            return;
        }
        self.update = UpdateState::Checking;
        cx.notify();
        let task = cx.background_executor().spawn(async { update::check() });
        cx.spawn(async move |this, cx| {
            let state = match task.await {
                Ok(None) => UpdateState::UpToDate,
                Ok(Some(v)) => UpdateState::Available(v),
                Err(e) => {
                    tracing::warn!("update check failed: {e:#}");
                    UpdateState::Failed
                }
            };
            let _ = this.update(cx, |this, cx| {
                this.update = state;
                cx.notify();
            });
        })
        .detach();
    }

    /// The message under a tool's shortcut field, if any.
    fn shortcut_message(&self, tool: ToolId, cx: &App) -> Option<SharedString> {
        let row = self.row(tool);
        row.refused.clone().or_else(|| row.hotkey.read(cx).error()).or_else(|| {
            cx.global::<Daemon>().hotkey_errors.iter().find(|e| e.tool == tool && row.enabled).map(|e| match &e.kind {
                HotkeyErrorKind::InUse => t("settings.shortcut.in_use").into(),
                HotkeyErrorKind::Invalid(_) => t("settings.shortcut.invalid").into(),
                HotkeyErrorKind::SharedWithPowerToys(module) => {
                    tf("settings.shortcut.powertoys", &[("module", module)]).into()
                }
            })
        })
    }
}

/// "1 game" / "4 games".
fn games_count(n: usize) -> String {
    if n == 1 { t("settings.games.count_one").into() } else { tf("settings.games.count", &[("n", &n.to_string())]) }
}

/// `1893440` -> `1.9M`.
fn compact(n: usize) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => format!("{:.0}K", n as f64 / 1e3),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
}

fn ago(at: SystemTime) -> String {
    let secs = at.elapsed().map_or(0, |d| d.as_secs());
    match secs {
        0..60 => t("time.just_now").into(),
        60..3600 => tf("time.minutes_ago", &[("n", &(secs / 60).to_string())]),
        3600..86400 => tf("time.hours_ago", &[("n", &(secs / 3600).to_string())]),
        _ => tf("time.days_ago", &[("n", &(secs / 86400).to_string())]),
    }
}

/// (dot color, main line, detail line) for the file index.
fn index_status(daemon: &Daemon, p: &Palette) -> (gpui_kit::Hsla, String, String) {
    let Some(s) = daemon.index_status() else {
        return (p.muted, t("settings.index.off").into(), String::new());
    };
    match s.phase {
        Phase::Loading => (p.accent, t("settings.index.loading").into(), String::new()),
        Phase::Scanning { found } => {
            let main = tf("settings.index.scanning", &[("found", &compact(found))]);
            let detail = if s.entries > 0 { t("settings.index.scanning_detail").into() } else { String::new() };
            (p.accent, main, detail)
        }
        Phase::Ready => {
            let mut detail = tf("settings.index.size", &[("mib", &(s.memory_bytes / (1 << 20)).to_string())]);
            if let (Some(at), Some(took)) = (s.last_scan, s.last_scan_took) {
                detail += " · ";
                detail += &tf(
                    "settings.index.last_scan",
                    &[("ago", &ago(at)), ("secs", &format!("{:.0}", took.as_secs_f32()))],
                );
            }
            if s.watched.is_empty() {
                detail += " · ";
                detail += t("settings.index.no_watch");
            }
            (p.success, tf("settings.index.ready", &[("count", &compact(s.entries))]), detail)
        }
    }
}

fn section_header(title: &str, description: Option<&str>, toggle: Option<AnyElement>, p: &Palette) -> Div {
    div()
        .pt(px(20.))
        .pb(px(10.))
        .flex()
        .items_center()
        .gap(px(12.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .ts(20., 28.)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.foreground)
                        .child(title.to_owned()),
                )
                .when_some(description, |d, text| {
                    d.child(div().ts(12., 16.).text_color(p.muted).child(text.to_owned()))
                }),
        )
        .children(toggle)
}

/// Card that holds a tool's settings; dimmed and inert while the tool is off.
fn card(enabled: bool, p: &Palette) -> Div {
    div()
        .relative()
        .px(px(16.))
        .pb(px(4.))
        .flex()
        .flex_col()
        .rounded(px(R_MD))
        .bg(p.surface)
        .border_1()
        .border_color(p.border)
        .when(!enabled, |d| d.opacity(0.45))
}

/// Covers a card so nothing inside reacts while its tool is off.
fn blocker() -> impl IntoElement {
    div().id("blocker").absolute().top_0().left_0().size_full().occlude()
}

fn group_label(label: &str, right: Option<String>, p: &Palette) -> Div {
    div()
        .pt(px(14.))
        .pb(px(4.))
        .flex()
        .items_center()
        .border_t_1()
        .border_color(p.border)
        .child(
            div()
                .flex_1()
                .ts(11., 14.)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(p.muted)
                .child(label.to_uppercase()),
        )
        .when_some(right, |d, r| d.child(div().text_size(px(12.)).text_color(p.muted).child(r)))
}

/// A labelled row with a control on the right; `first` rows have no divider.
fn setting_row(min_h: f32, first: bool, label: &str, hint: Option<&str>, p: &Palette) -> Div {
    div()
        .min_h(px(min_h))
        .flex()
        .items_center()
        .gap(px(12.))
        .when(!first, |d| d.border_t_1().border_color(p.border))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().ts(14., 20.).text_color(p.foreground).child(label.to_owned()))
                .when_some(hint, |d, h| d.child(div().ts(12., 16.).text_color(p.muted).child(h.to_owned()))),
        )
}

impl SettingsView {
    fn shortcut_row(&self, tool: ToolId, p: &Palette, cx: &App) -> Div {
        let message = self.shortcut_message(tool, cx);
        let wayland = cx.global::<Daemon>().session.is_wayland();
        div()
            .py(px(12.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .w(px(80.))
                            .flex_none()
                            .text_size(px(13.))
                            .text_color(p.muted)
                            .truncate()
                            .child(t("settings.shortcut")),
                    )
                    .child(self.row(tool).hotkey.clone()),
            )
            .when_some(message, |d, m| d.child(div().ml(px(92.)).ts(12., 16.).text_color(p.danger).child(m)))
            .when(wayland, |d| {
                d.child(
                    div()
                        .ml(px(92.))
                        .ts(12., 16.)
                        .text_color(p.muted)
                        .child(tf("settings.shortcut.wayland", &[("command", &format!("yaltri toggle {tool}"))])),
                )
            })
    }

    fn launcher_section(&self, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let daemon = cx.global::<Daemon>();
        let enabled = self.row(ToolId::Launcher).enabled;
        let (dot, main, detail) = index_status(daemon, p);
        let counts = daemon.game_counts();
        let total: usize = Launcher::PLATFORM
            .iter()
            .filter(|l| l.enabled_in(&self.games))
            .map(|l| counts.get(l).copied().unwrap_or(0))
            .sum();

        let toggle = ui::switch("launcher-enabled", enabled, p)
            .on_click(cx.listener(move |this, _, _, cx| this.set_tool_enabled(ToolId::Launcher, !enabled, cx)))
            .into_any_element();

        let games = Launcher::PLATFORM.iter().copied().enumerate().map(|(ix, launcher)| {
            let on = launcher.enabled_in(&self.games);
            let count = counts.get(&launcher).copied().unwrap_or(0);
            let (text, color) = if !on {
                (t("settings.games.off").to_owned(), p.muted)
            } else if count > 0 {
                (games_count(count), p.foreground)
            } else {
                (t("settings.games.none").to_owned(), p.muted)
            };
            setting_row(40., ix == 0, launcher.display_name(), None, p)
                .child(div().flex_none().text_size(px(12.)).text_color(color).child(text))
                .child(ui::switch(("game", ix), on, p).on_click(cx.listener(move |this, _, _, cx| {
                    launcher.set_in(&mut this.games, !on);
                    cx.notify();
                })))
        });
        let games_block = div()
            .relative()
            .flex()
            .flex_col()
            .when(!self.apps_enabled, |d| {
                d.opacity(0.45)
                    .child(div().pt(px(4.)).ts(12., 16.).text_color(p.muted).child(t("settings.games.needs_apps")))
            })
            .children(games)
            .when(!self.apps_enabled, |d| d.child(blocker()));

        let apps_on = self.apps_enabled;
        let files_on = self.files_enabled;
        let body = card(enabled, p)
            .child(self.shortcut_row(ToolId::Launcher, p, cx))
            .child(group_label(t("settings.group.sources"), None, p))
            .child(setting_row(44., true, t("settings.sources.apps"), None, p).child(
                ui::switch("apps-enabled", apps_on, p).on_click(cx.listener(move |this, _, _, cx| {
                    this.apps_enabled = !apps_on;
                    cx.notify();
                })),
            ))
            .child(setting_row(44., false, t("settings.sources.files"), None, p).child(
                ui::switch("files-enabled", files_on, p).on_click(cx.listener(move |this, _, _, cx| {
                    this.files_enabled = !files_on;
                    cx.notify();
                })),
            ))
            .child(
                div()
                    .min_h(px(52.))
                    .py(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .border_t_1()
                    .border_color(p.border)
                    .child(div().size(px(8.)).flex_none().rounded(px(4.)).bg(dot))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().ts(13., 18.).text_color(p.foreground).truncate().child(main))
                            .when(!detail.is_empty(), |d| {
                                d.child(div().ts(12., 16.).text_color(p.muted).child(detail))
                            }),
                    )
                    .child(
                        ui::button("rescan", ButtonKind::Secondary, p)
                            .child(ui::icon("refresh-cw", 14., p.foreground))
                            .child(t("settings.rescan"))
                            .on_click(cx.listener(|_, _, _, cx| cx.global::<Daemon>().rescan_files())),
                    ),
            )
            .child(group_label(
                t("settings.group.games"),
                Some(games_count(total)),
                p,
            ))
            .child(games_block)
            .when(!enabled, |d| d.child(blocker()));

        vec![
            section_header(tool_name(ToolId::Launcher), Some(t("settings.launcher.description")), Some(toggle), p)
                .into_any_element(),
            body.into_any_element(),
        ]
    }

    fn general_section(&self, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        // Each language in its own name; "auto" follows the OS locale.
        let autostart = self.autostart;
        let languages: [(&str, &str); 3] =
            [("auto", t("settings.general.language.system")), ("tr", "Türkçe"), ("en", "English")];
        let current = languages.iter().find(|(key, _)| *key == self.language).map_or(languages[0].1, |(_, name)| *name);

        let language_menu = self.language_menu.then(|| {
            let items = languages.into_iter().enumerate().map(|(ix, (key, name))| {
                let selected = key == self.language;
                div()
                    .id(("language-item", ix))
                    .h(px(32.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|s| s.bg(p.list_hover))
                    .child(
                        div().size(px(16.)).flex_none().when(selected, |d| d.child(ui::icon("check", 16., p.accent_strong))),
                    )
                    .child(div().text_size(px(13.)).child(name))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.language = key.to_owned();
                        this.language_menu = false;
                        this.save(cx);
                    }))
            });
            let trigger = self.language_trigger?;
            Some(ui::dropdown(
                trigger,
                ui::dropdown_list("language-list", trigger, p).children(items),
            ))
        })
        .flatten();
        let language_button = div()
            .relative()
            .flex_none()
            .child(
                div()
                    .id("language")
                    .w(px(160.))
                    .h(px(32.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .rounded(px(R_SM))
                    .border_1()
                    .border_color(p.border)
                    .bg(p.surface_raised)
                    .text_size(px(13.))
                    .cursor_pointer()
                    .hover(|s| s.border_color(p.border_strong))
                    .child(div().truncate().child(current))
                    .child(ui::icon("chevron-down", 14., p.muted))
                    .child(ui::track_bounds(cx.weak_entity(), |this: &mut Self, b| this.language_trigger = Some(b)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.language_menu = !this.language_menu;
                        cx.notify();
                    })),
            )
            .children(language_menu);

        let segments = ThemeChoice::ALL.into_iter().map(|choice| {
            let (icon, label) = match choice {
                ThemeChoice::System => ("monitor", t("settings.general.theme.system")),
                ThemeChoice::Dark => ("moon", t("settings.general.theme.dark")),
                ThemeChoice::Light => ("sun", t("settings.general.theme.light")),
            };
            ui::segment(choice.key(), icon, label, choice == self.theme, p).on_click(cx.listener(move |this, _, _, cx| {
                if this.theme != choice {
                    this.theme = choice;
                    this.save(cx);
                }
            }))
        });

        let body = div()
            .px(px(16.))
            .flex()
            .flex_col()
            .rounded(px(R_MD))
            .bg(p.surface)
            .border_1()
            .border_color(p.border)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        setting_row(56., true, t("settings.general.autostart"), Some(t("settings.general.autostart_hint")), p)
                            .child(ui::switch("autostart", autostart, p).on_click(cx.listener(move |this, _, _, cx| {
                                this.set_autostart(!autostart, cx)
                            }))),
                    )
                    .when_some(self.autostart_error.clone(), |d, e| {
                        d.child(div().pb(px(12.)).ts(12., 16.).text_color(p.danger).child(e))
                    }),
            )
            .child(
                setting_row(56., false, t("settings.general.language"), Some(t("settings.general.language_hint")), p)
                    .child(language_button),
            )
            .child(
                setting_row(56., false, t("settings.general.theme"), Some(t("settings.general.theme_hint")), p)
                    .child(ui::segmented_track(p).children(segments)),
            );

        vec![section_header(t("settings.general.title"), None, None, p).into_any_element(), body.into_any_element()]
    }

    fn picker_section(&self, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let enabled = self.row(ToolId::Picker).enabled;
        let toggle = ui::switch("picker-enabled", enabled, p)
            .on_click(cx.listener(move |this, _, _, cx| this.set_tool_enabled(ToolId::Picker, !enabled, cx)))
            .into_any_element();

        let format_menu = self.format_menu.then(|| {
            let items = Format::ALL.into_iter().enumerate().map(|(ix, f)| {
                let selected = f == self.copy_format;
                div()
                    .id(("format-item", ix))
                    .h(px(32.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|s| s.bg(p.list_hover))
                    .child(
                        div()
                            .size(px(16.))
                            .flex_none()
                            .when(selected, |d| d.child(ui::icon("check", 16., p.accent_strong))),
                    )
                    .child(div().font_family(theme::mono_font()).text_size(px(13.)).child(f.label()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.copy_format = f;
                        this.format_menu = false;
                        cx.notify();
                    }))
            });
            let trigger = self.format_trigger?;
            Some(ui::dropdown(
                trigger,
                ui::dropdown_list("format-list", trigger, p).children(items),
            ))
        })
        .flatten();

        let open_editor = self.picker_open_editor;
        let format_button = div().relative().flex_none().child(
            div()
                .id("copy-format")
                .w(px(140.))
                .h(px(32.))
                .px(px(10.))
                .flex()
                .items_center()
                .justify_between()
                .rounded(px(R_SM))
                .border_1()
                .border_color(p.border)
                .bg(p.surface_raised)
                .font_family(theme::mono_font())
                .text_size(px(13.))
                .cursor_pointer()
                .hover(|s| s.border_color(p.border_strong))
                .child(self.copy_format.label())
                .child(ui::icon("chevron-down", 14., p.muted))
                .child(ui::track_bounds(cx.weak_entity(), |this: &mut Self, b| this.format_trigger = Some(b)))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.format_menu = !this.format_menu;
                    cx.notify();
                })),
        );
        let body = card(enabled, p)
            .child(self.shortcut_row(ToolId::Picker, p, cx))
            .child(group_label(t("settings.group.after_pick"), None, p))
            .child(
                setting_row(
                    56.,
                    true,
                    t("settings.picker.copy_format"),
                    Some(t("settings.picker.copy_format_hint")),
                    p,
                )
                .child(format_button.children(format_menu)),
            )
            .child(setting_row(48., false, t("settings.picker.open_editor"), None, p).child(
                ui::switch("picker-editor", open_editor, p).on_click(cx.listener(move |this, _, _, cx| {
                    this.picker_open_editor = !open_editor;
                    cx.notify();
                })),
            ))
            .when(!enabled, |d| d.child(blocker()));

        vec![
            section_header(tool_name(ToolId::Picker), Some(t("settings.picker.description")), Some(toggle), p)
                .into_any_element(),
            body.into_any_element(),
        ]
    }

    fn about_section(&self, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let config_path = cx.global::<Daemon>().paths.settings_file();
        let config_dir = config_path.parent().map(|d| d.to_path_buf());
        let mut version = tf("settings.about.version", &[("version", update::VERSION)]);
        let status = match &self.update {
            UpdateState::Idle => None,
            UpdateState::Checking => Some(t("settings.about.checking").to_owned()),
            UpdateState::UpToDate => Some(t("settings.about.up_to_date").to_owned()),
            UpdateState::Available(v) => Some(tf("settings.about.available", &[("version", v)])),
            UpdateState::Failed => Some(t("settings.about.check_failed").to_owned()),
        };
        if let Some(status) = status {
            version += " · ";
            version += &status;
        }

        let link = |id: &'static str, label: &'static str, url: &'static str| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(px(4.))
                .text_size(px(12.))
                .text_color(p.foreground)
                .cursor_pointer()
                .child(div().border_b_1().border_color(p.border_strong).child(label))
                .child(ui::icon("external-link", 12., p.muted))
                .on_click(move |_, _, _| {
                    let _ = open::that_detached(url);
                })
        };

        let update_button = if matches!(self.update, UpdateState::Available(_)) {
            ui::button("update", ButtonKind::Primary, p)
                .child(ui::icon("download", 14., p.accent_fg))
                .child(t("settings.about.download"))
                .on_click(|_, _, _| {
                    let _ = open::that_detached(update::RELEASES_URL);
                })
        } else {
            ui::button("update", ButtonKind::Secondary, p)
                .child(ui::icon("refresh-cw", 14., p.foreground))
                .child(t("settings.about.check"))
                .on_click(cx.listener(|this, _, _, cx| this.check_updates(cx)))
        };

        let body = div()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .rounded(px(R_MD))
            .bg(p.surface)
            .border_1()
            .border_color(p.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(img("brand/app-icon.svg").size(px(40.)).flex_none())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().ts(15., 20.).font_weight(FontWeight::SEMIBOLD).child("Yaltri"))
                            .child(div().ts(12., 16.).text_color(p.muted).truncate().child(version)),
                    )
                    .child(update_button),
            )
            .child(div().ts(13., 19.).text_color(p.muted).italic().child(t("settings.about.tagline")))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .pt(px(12.))
                    .border_t_1()
                    .border_color(p.border)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().ts(12., 16.).text_color(p.muted).child(t("settings.about.config_file")))
                            .child(
                                div()
                                    .ts(12., 18.)
                                    .font_family(theme::mono_font())
                                    .text_color(p.foreground)
                                    .truncate()
                                    .child(config_path.display().to_string()),
                            ),
                    )
                    .child(
                        ui::button("open-folder", ButtonKind::Secondary, p)
                            .child(ui::icon("folder-open", 14., p.foreground))
                            .child(t("settings.about.open_folder"))
                            .on_click(move |_, _, _| {
                                if let Some(dir) = &config_dir {
                                    let _ = open::that_detached(dir);
                                }
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(16.))
                    .child(link("source", t("settings.about.source"), update::REPO_URL))
                    .child(link("licenses", t("settings.about.licenses"), update::LICENSES_URL)),
            );

        vec![section_header(t("settings.about.title"), None, None, p).into_any_element(), body.into_any_element()]
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::of(window);

        // Shortcut fields turn red while their shortcut has a problem.
        let flags: Vec<(Entity<ShortcutInput>, bool)> =
            self.rows.iter().map(|r| (r.hotkey.clone(), self.shortcut_message(r.tool, cx).is_some())).collect();
        for (input, conflict) in flags {
            if input.read(cx).is_conflict() != conflict {
                input.update(cx, |h, cx| h.set_conflict(conflict, cx));
            }
        }

        let warning = self.warning.clone().map(|w| {
            div()
                .mt(px(16.))
                .p(px(12.))
                .rounded(px(R_MD))
                .bg(p.danger_bg)
                .border_1()
                .border_color(p.danger)
                .ts(12., 16.)
                .text_color(p.danger)
                .child(w)
        });

        // Each section is a header followed by its body; remember where each
        // tool's header lands among the scroll's children.
        let first = usize::from(warning.is_some());
        let mut starts: Vec<(ToolId, usize)> = Vec::new();
        let mut sections: Vec<AnyElement> = Vec::new();
        sections.extend(self.general_section(p, cx));
        if is_implemented(ToolId::Launcher) {
            starts.push((ToolId::Launcher, first + sections.len()));
            sections.extend(self.launcher_section(p, cx));
        }
        if is_implemented(ToolId::Picker) {
            starts.push((ToolId::Picker, first + sections.len()));
            sections.extend(self.picker_section(p, cx));
        }
        sections.extend(self.about_section(p, cx));
        if let Some(tool) = self.scroll_to.take()
            && let Some(&(_, ix)) = starts.iter().find(|(t, _)| *t == tool)
        {
            self.scroll.scroll_to_top_of_item(ix);
        }

        let scroll = div()
            .id("settings-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .px(px(24.))
            .pb(px(24.))
            .flex()
            .flex_col()
            .children(warning)
            .children(sections);

        let toast = self.toast.clone().map(|(is_error, text)| {
            div().absolute().left_0().right_0().bottom(px(56. + 12.)).flex().justify_center().child(if is_error {
                div()
                    .h(px(32.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .rounded(px(R_MD))
                    .bg(p.danger_bg)
                    .border_1()
                    .border_color(p.danger)
                    .text_color(p.danger)
                    .text_size(px(13.))
                    .shadow(p.shadow_md.clone())
                    .child(text)
            } else {
                ui::toast(text, None, p)
            })
        });

        let footer = div()
            .h(px(56.))
            .flex_none()
            .pl(px(24.))
            .pr(px(16.))
            .flex()
            .items_center()
            .justify_end()
            .border_t_1()
            .border_color(p.border)
            .rounded_b(px(ui::FRAME_INNER_RADIUS))
            .bg(p.background)
            .child(
                ui::button("save", ButtonKind::Primary, p)
                    .px(px(16.))
                    .child(t("settings.save"))
                    .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
            );

        let body = div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(p.border)
            .child(scroll)
            .child(footer)
            .children(toast);

        // The system title (taskbar, Alt+Tab) follows a language change too.
        window.set_window_title(t("settings.title"));
        // While a dropdown is open, a transparent layer under it takes the
        // wheel (the page must not scroll away from the trigger) and closes
        // it on a click outside.
        let dropdown_open = self.language_menu || self.format_menu;
        ui::window_frame(t("settings.title").into(), p, body).relative().when(dropdown_open, |d| {
            d.child(
                div()
                    .id("dropdown-blocker")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .occlude()
                    .on_scroll(|_, _, cx| cx.stop_propagation())
                    .on_mouse_down(
                        gpui_kit::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.language_menu = false;
                            this.format_menu = false;
                            cx.notify();
                        }),
                    ),
            )
        })
    }
}
