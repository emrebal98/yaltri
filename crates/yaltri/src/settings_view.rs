//! Settings window, opened from the tray: enable/disable tools and edit their
//! shortcuts. Edits are staged and written on "Kaydet".

use gpui_kit::component::{ActiveTheme as _, Disableable as _};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::prelude::FluentBuilder as _;
use std::time::{Duration, SystemTime};

use gpui_kit::{
    App, AppContext as _, Bounds, Context, Entity, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Task, TitlebarOptions, Window, WindowBounds, WindowOptions, div, px, size,
};
use yaltri_files::Phase;
use yaltri_core::{Settings, ToolId};
use yaltri_platform::hotkeys;

use crate::daemon::{Daemon, is_implemented};

pub fn open(cx: &mut App) {
    if let Some(handle) = cx.global::<Daemon>().settings_window
        && cx.windows().contains(&handle)
    {
        let _ = handle.update(cx, |_, window, _| window.activate_window());
        return;
    }

    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(560.), px(680.)), cx))),
        titlebar: Some(TitlebarOptions { title: Some("Yaltri Ayarları".into()), ..Default::default() }),
        app_id: Some("yaltri".into()),
        ..Default::default()
    };
    match gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| SettingsView::new(window, cx))) {
        Ok((handle, _)) => {
            cx.global_mut::<Daemon>().settings_window = Some(handle);
            let _ = handle.update(cx, |_, window, _| window.activate_window());
            cx.activate(true);
        }
        Err(e) => tracing::error!("opening settings failed: {e:#}"),
    }
}

struct ToolRow {
    tool: ToolId,
    enabled: bool,
    hotkey: Entity<InputState>,
}

pub struct SettingsView {
    rows: Vec<ToolRow>,
    files_enabled: bool,
    apps_enabled: bool,
    /// Feedback from the last save, or startup warnings. (`is_error`, text)
    status: Option<(bool, SharedString)>,
    /// Re-renders once a second so the index status stays live.
    _refresh: Task<()>,
}

impl SettingsView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let daemon = cx.global::<Daemon>();
        let settings = daemon.settings.clone();
        let mut status = daemon.settings_warning.clone().map(|w| (true, w.into()));
        if status.is_none() && !daemon.hotkey_errors.is_empty() {
            status = Some((true, hotkey_error_text(daemon).into()));
        }
        let rows = ToolId::ALL
            .into_iter()
            .map(|tool| {
                let value = settings.hotkey(tool).to_owned();
                let hotkey = cx.new(|cx| {
                    let mut state = InputState::new(window, cx).placeholder("ör. Alt+Space");
                    state.set_value(value, window, cx);
                    state
                });
                ToolRow { tool, enabled: settings.is_enabled(tool), hotkey }
            })
            .collect();
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
            status,
            _refresh: refresh,
        }
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let mut settings: Settings = cx.global::<Daemon>().settings.clone();
        for row in &self.rows {
            let spec = row.hotkey.read(cx).value().trim().to_owned();
            if row.enabled && !spec.is_empty() && hotkeys::parse(&spec).is_err() {
                self.status = Some((true, format!("{}: geçersiz kısayol '{spec}'", row.tool.display_name()).into()));
                cx.notify();
                return;
            }
            settings.set_enabled(row.tool, row.enabled);
            settings.set_hotkey(row.tool, spec);
        }
        settings.files.files_enabled = self.files_enabled;
        settings.files.apps_enabled = self.apps_enabled;
        if let Some((a, b)) = settings.hotkey_conflicts().first() {
            self.status = Some((
                true,
                format!("{} ve {} aynı kısayolu kullanıyor", a.display_name(), b.display_name()).into(),
            ));
            cx.notify();
            return;
        }

        let daemon = cx.global_mut::<Daemon>();
        self.status = Some(match daemon.update_settings(settings) {
            Ok(()) if daemon.hotkey_errors.is_empty() => (false, "Kaydedildi.".into()),
            Ok(()) => (true, hotkey_error_text(daemon).into()),
            Err(e) => (true, format!("Kaydedilemedi: {e:#}").into()),
        });
        cx.notify();
    }
}

fn hotkey_error_text(daemon: &Daemon) -> String {
    daemon
        .hotkey_errors
        .iter()
        .map(|e| format!("{}: {}", e.tool.display_name(), e.message))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `1893440` -> `1.893.440`
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push('.');
        }
        out.push(c);
    }
    out
}

fn ago(t: SystemTime) -> String {
    let secs = t.elapsed().map_or(0, |d| d.as_secs());
    match secs {
        0..60 => "az önce".into(),
        60..3600 => format!("{} dk önce", secs / 60),
        3600..86400 => format!("{} sa önce", secs / 3600),
        _ => format!("{} gün önce", secs / 86400),
    }
}

fn index_status_text(daemon: &Daemon) -> String {
    let Some(s) = daemon.index_status() else {
        return "Dosya araması kapalı.".into();
    };
    match s.phase {
        Phase::Loading => "Önceki index yükleniyor…".into(),
        Phase::Scanning { found } if s.entries > 0 => format!(
            "Taranıyor… {} öğe bulundu. Arama bu sırada önceki index'le ({} öğe) çalışıyor.",
            thousands(found),
            thousands(s.entries)
        ),
        Phase::Scanning { found } => format!("İlk tarama… {} öğe bulundu.", thousands(found)),
        Phase::Ready => {
            let mut text = format!("{} öğe · {} MiB", thousands(s.entries), s.memory_bytes / (1 << 20));
            if let (Some(at), Some(took)) = (s.last_scan, s.last_scan_took) {
                text += &format!(" · son tarama {} ({:.0} sn sürdü)", ago(at), took.as_secs_f32());
            }
            if s.watched.is_empty() {
                text += " · canlı izleme yok, periyodik taranıyor";
            }
            text
        }
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let daemon = cx.global::<Daemon>();
        let session = daemon.session;
        let settings_path = daemon.paths.settings_file().display().to_string();
        let index_text = index_status_text(daemon);

        let tool_rows = self.rows.iter().enumerate().map(|(ix, row)| {
            let implemented = is_implemented(row.tool);
            div()
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .border_1()
                .border_color(theme.border)
                .rounded(theme.radius)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_base()
                                .child(row.tool.display_name())
                                .when(!implemented, |d| {
                                    d.child(div().text_xs().text_color(theme.muted_foreground).child("yakında"))
                                }),
                        )
                        .child(
                            Switch::new(("enabled", ix))
                                .checked(row.enabled)
                                .disabled(!implemented)
                                .on_change(cx.listener(move |this, value: &bool, _, cx| {
                                    this.rows[ix].enabled = *value;
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().w(px(80.)).text_sm().text_color(theme.muted_foreground).child("Kısayol"))
                        .child(div().flex_1().child(Input::new(&row.hotkey))),
                )
                .when(session.is_wayland() && implemented, |d| {
                    d.child(div().text_xs().text_color(theme.muted_foreground).child(format!(
                        "Wayland: bu kısayol uygulanamaz. Masaüstü ayarlarında özel kısayol olarak \
                         `yaltri toggle {}` komutunu ekleyin.",
                        row.tool
                    )))
                })
        });

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(div().text_xl().child("Araçlar"))
            .children(tool_rows)
            .child(div().text_xl().child("Arama kaynakları"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .border_1()
                    .border_color(theme.border)
                    .rounded(theme.radius)
                    .child(
                        Switch::new("apps-enabled")
                            .label("Uygulamalar")
                            .checked(self.apps_enabled)
                            .on_change(cx.listener(|this, value: &bool, _, cx| {
                                this.apps_enabled = *value;
                                cx.notify();
                            })),
                    )
                    .child(
                        Switch::new("files-enabled")
                            .label("Dosya ve klasörler")
                            .checked(self.files_enabled)
                            .on_change(cx.listener(|this, value: &bool, _, cx| {
                                this.files_enabled = *value;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(div().flex_1().text_xs().text_color(theme.muted_foreground).child(index_text))
                            .child(
                                Button::new("rescan")
                                    .outline()
                                    .label("Yeniden tara")
                                    .on_click(cx.listener(|_, _, _, cx| cx.global::<Daemon>().rescan_files())),
                            ),
                    ),
            )
            .when_some(self.status.clone(), |d, (is_error, text)| {
                d.child(
                    div()
                        .text_sm()
                        .text_color(if is_error { theme.danger } else { theme.muted_foreground })
                        .child(text),
                )
            })
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_xs().text_color(theme.muted_foreground).child(settings_path))
                    .child(
                        Button::new("save")
                            .primary()
                            .label("Kaydet")
                            .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                    ),
            )
    }
}
