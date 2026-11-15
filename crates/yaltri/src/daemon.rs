//! The long-lived tray process. Owns settings, the tray icon, global hotkeys
//! and the tool windows; every request arrives as a `Command`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui_kit::{AnyWindowHandle, App, Entity, Global, QuitMode, WeakEntity};
use yaltri_apps::AppsProvider;
use yaltri_calc::CalcProvider;
use yaltri_color::{ColorHistory, Rgb};
use yaltri_core::paths::Paths;
use yaltri_core::{Command, History, SearchProvider, Settings, ToolId};
use yaltri_files::{FileIndexer, FilesProvider, Scope};
use yaltri_platform::CommandSender;
use yaltri_platform::hotkeys::{HotkeyError, Hotkeys};
use yaltri_platform::session::Session;
use yaltri_platform::tray::{self, Tray};

use crate::color_editor::ColorEditor;
use crate::{launcher, picker, settings_view};

pub struct Daemon {
    pub paths: Paths,
    pub session: Session,
    pub settings: Settings,
    /// Problem loading settings at startup, shown in the settings window.
    pub settings_warning: Option<String>,
    pub hotkey_errors: Vec<HotkeyError>,
    pub providers: Arc<Vec<Box<dyn SearchProvider>>>,
    pub history: Arc<Mutex<History>>,
    pub launcher_window: Option<AnyWindowHandle>,
    /// The settings window and its view (the window's root is a wrapper).
    pub settings_window: Option<(AnyWindowHandle, WeakEntity<crate::settings_view::SettingsView>)>,
    /// One overlay per display while picking a color.
    pub picker_windows: Vec<AnyWindowHandle>,
    pub colors: ColorHistory,
    pub color_editor: Option<Entity<ColorEditor>>,
    pub color_editor_window: Option<AnyWindowHandle>,
    tx: CommandSender,
    tray: Option<Tray>,
    /// App theme the tray menu icons were drawn for.
    tray_apps_light: bool,
    hotkeys: Option<Hotkeys>,
    /// True while a shortcut is being recorded in settings.
    hotkeys_suspended: bool,
    /// GNOME desktops: our shortcuts live there as custom shortcuts (see
    /// `gnome_keys`), plus the specs last applied. `None` elsewhere.
    #[cfg(target_os = "linux")]
    gnome: Option<(yaltri_platform::gnome_keys::GnomeKeys, Option<Vec<String>>)>,
    /// Other Wayland desktops: shortcuts through the desktop portal
    /// (`portal_keys`), plus the bindings last sent. `None` elsewhere.
    #[cfg(target_os = "linux")]
    portal: Option<(yaltri_platform::portal_keys::PortalKeys, Option<Vec<yaltri_platform::portal_keys::PortalBinding>>)>,
    files: Option<(Scope, FileIndexer)>,
    apps: Option<Arc<AppsProvider>>,
}

impl Global for Daemon {}

const HOTKEY_RETRY: Duration = Duration::from_secs(20);
/// How often the tray checks the taskbar theme (no event for it).
const THEME_POLL: Duration = Duration::from_secs(5);

pub fn run(paths: Paths, session: Session, initial: Option<Command>) -> Result<()> {
    let first_run = !paths.settings_file().exists();
    let loaded = Settings::load(&paths.settings_file())?;
    if first_run {
        first_run_setup(&loaded.settings, &paths);
    }
    let (tx, mut rx) = mpsc::unbounded::<Command>();

    // Bind the IPC socket before the UI so a racing second instance sees us.
    yaltri_platform::ipc::serve(tx.clone())?;

    gpui_kit::application()
        .with_assets(crate::assets::YaltriAssets)
        // Tray app: GPUI's default on Windows/Linux quits when the last
        // window closes. Only `Command::Quit` may end the process.
        .with_quit_mode(QuitMode::Explicit)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            crate::i18n::set_language(&loaded.settings.general.language);
            crate::theme::set_choice(crate::theme::ThemeChoice::from_key(&loaded.settings.general.theme));
            sync_theme(cx);

            let history = History::load(&paths.history_file());
            let colors = ColorHistory::load(&paths.colors_file());
            #[cfg(target_os = "linux")]
            let gnome = yaltri_platform::gnome_keys::available()
                .then(|| (yaltri_platform::gnome_keys::GnomeKeys::load(paths.gnome_shortcuts_file()), None));
            let mut daemon = Daemon {
                paths,
                session,
                settings: loaded.settings,
                settings_warning: loaded.recovered_from.map(|e| e.to_string()),
                hotkey_errors: Vec::new(),
                providers: Arc::new(Vec::new()),
                history: Arc::new(Mutex::new(history)),
                launcher_window: None,
                settings_window: None,
                picker_windows: Vec::new(),
                colors,
                color_editor: None,
                color_editor_window: None,
                tx: tx.clone(),
                tray: None,
                tray_apps_light: yaltri_platform::icon::apps_use_light_theme(),
                hotkeys: None,
                hotkeys_suspended: false,
                #[cfg(target_os = "linux")]
                gnome,
                #[cfg(target_os = "linux")]
                portal: None,
                files: None,
                apps: None,
            };
            daemon.start_platform_services();
            daemon.rebuild_search();
            cx.set_global(daemon);

            if let Some(cmd) = initial {
                handle(cmd, cx);
            }

            // A shortcut held by another program (e.g. PowerToys) becomes
            // ours as soon as that program lets it go.
            cx.spawn(async move |cx| {
                loop {
                    cx.background_executor().timer(HOTKEY_RETRY).await;
                    cx.update(|cx| cx.global_mut::<Daemon>().retry_failed_hotkeys());
                }
            })
            .detach();

            cx.spawn(async move |cx| {
                loop {
                    cx.background_executor().timer(THEME_POLL).await;
                    cx.update(|cx| cx.global_mut::<Daemon>().refresh_tray_theme());
                }
            })
            .detach();

            cx.spawn(async move |cx| {
                while let Some(cmd) = rx.next().await {
                    cx.update(|cx| handle(cmd, cx));
                }
            })
            .detach();
        });
    Ok(())
}

impl Daemon {
    /// Tray + hotkeys. Both must be created on the main thread (Windows).
    fn start_platform_services(&mut self) {
        // Lets the dock, app grid and System Monitor find our icon.
        #[cfg(target_os = "linux")]
        if let Err(e) = yaltri_platform::autostart::install_app_entry(include_str!("../../../assets/brand/app-icon.svg"))
        {
            tracing::warn!("installing the application entry failed: {e:#}");
        }
        match Tray::spawn(self.tx.clone(), self.tray_menu()) {
            Ok(t) => self.tray = Some(t),
            Err(e) => tracing::error!("tray unavailable: {e:#}"),
        }
        if self.uses_gnome_shortcuts() {
            tracing::info!("shortcuts go through GNOME custom shortcuts");
        } else if self.session == Session::WaylandLayerShell {
            #[cfg(target_os = "linux")]
            {
                tracing::info!("shortcuts go through the desktop's global shortcuts portal");
                self.portal = Some((yaltri_platform::portal_keys::PortalKeys::start(self.tx.clone()), None));
            }
        } else if self.session.supports_global_hotkeys() {
            match Hotkeys::new(self.tx.clone()) {
                Ok(h) => self.hotkeys = Some(h),
                Err(e) => tracing::error!("{e:#}"),
            }
        } else {
            tracing::info!("no global hotkey support in this session; bind `yaltri toggle <tool>` in the desktop settings");
        }
        self.apply_hotkeys();
    }

    /// Search · Color picker · Settings, separator, Quit.
    fn tray_menu(&self) -> Vec<Option<tray::MenuEntry>> {
        use crate::i18n::t;
        let entry = |label: &str, command: Command, icon: &str, shortcut: Option<String>| {
            Some(tray::MenuEntry {
                label: label.to_owned(),
                command,
                icon_svg: crate::assets::svg(&format!("icons/{icon}.svg")),
                shortcut,
            })
        };
        let mut items: Vec<Option<tray::MenuEntry>> = ToolId::ALL
            .into_iter()
            .filter(|t| self.settings.is_enabled(*t) && is_implemented(*t))
            .map(|tool| {
                let icon = match tool {
                    ToolId::Launcher => "search",
                    ToolId::Picker => "pipette",
                };
                let spec = self.settings.hotkey(tool);
                let shortcut = (!spec.is_empty()
                    && (self.session.supports_global_hotkeys() || self.uses_gnome_shortcuts() || self.uses_portal_shortcuts()))
                    .then(|| yaltri_platform::keys::display_parts(spec).join("+"));
                entry(crate::settings_view::tool_name(tool), Command::Toggle(tool), icon, shortcut)
            })
            .collect();
        items.push(entry(t("tray.settings"), Command::Settings, "settings", None));
        items.push(None);
        items.push(entry(t("tray.quit"), Command::Quit, "power", None));
        items
    }

    /// Follows taskbar/app theme changes in the tray icon and menu.
    fn refresh_tray_theme(&mut self) {
        let apps_light = yaltri_platform::icon::apps_use_light_theme();
        let menu = (apps_light != self.tray_apps_light).then(|| self.tray_menu());
        self.tray_apps_light = apps_light;
        if let Some(tray) = self.tray.as_mut() {
            if let Err(e) = tray.refresh_theme() {
                tracing::warn!("updating tray icon failed: {e:#}");
            }
            if let Some(menu) = menu
                && let Err(e) = tray.set_menu(menu)
            {
                tracing::warn!("updating tray menu failed: {e:#}");
            }
        }
    }

    /// Releases every global hotkey (while a shortcut is being recorded, so
    /// the keys reach the recorder) or restores them from settings.
    pub fn set_hotkeys_suspended(&mut self, suspended: bool) {
        self.hotkeys_suspended = suspended;
        if suspended {
            if let Some(hotkeys) = self.hotkeys.as_mut() {
                hotkeys.apply(&[]);
            }
            #[cfg(target_os = "linux")]
            self.apply_portal_shortcuts(&[]);
        } else {
            self.apply_hotkeys();
        }
    }

    /// Other Wayland desktops: shortcuts go through the desktop portal.
    fn uses_portal_shortcuts(&self) -> bool {
        #[cfg(target_os = "linux")]
        return self.portal.is_some();
        #[cfg(not(target_os = "linux"))]
        false
    }

    /// GNOME: shortcuts are GNOME custom shortcuts, not our own grabs.
    fn uses_gnome_shortcuts(&self) -> bool {
        #[cfg(target_os = "linux")]
        return self.gnome.is_some();
        #[cfg(not(target_os = "linux"))]
        false
    }

    fn apply_hotkeys(&mut self) {
        let bindings: Vec<(ToolId, String)> = ToolId::ALL
            .into_iter()
            .filter(|t| self.settings.is_enabled(*t) && is_implemented(*t))
            .map(|t| (t, self.settings.hotkey(t).to_owned()))
            .filter(|(_, spec)| !spec.trim().is_empty())
            .collect();
        #[cfg(target_os = "linux")]
        if let Some(errors) = self.apply_gnome_shortcuts(&bindings) {
            self.set_hotkey_errors(errors);
            return;
        }
        #[cfg(target_os = "linux")]
        if self.apply_portal_shortcuts(&bindings) {
            return;
        }
        let Some(hotkeys) = self.hotkeys.as_mut() else {
            return;
        };
        let errors = hotkeys.apply(&bindings);
        self.set_hotkey_errors(errors);
    }

    /// GNOME: frees `bindings` from GNOME's own shortcuts and sets them up as
    /// custom shortcuts running `yaltri toggle <tool>`. `None` off GNOME.
    #[cfg(target_os = "linux")]
    fn apply_gnome_shortcuts(&mut self, bindings: &[(ToolId, String)]) -> Option<Vec<HotkeyError>> {
        let (gnome, applied) = self.gnome.as_mut()?;
        let specs: Vec<String> = bindings.iter().map(|(_, spec)| spec.clone()).collect();
        if applied.as_ref() == Some(&specs) {
            return Some(self.hotkey_errors.clone());
        }
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "yaltri".into());
        let exe = if exe.contains([' ', '\'', '"']) { format!("'{}'", exe.replace('\'', "'\\''")) } else { exe };
        let gnome_bindings: Vec<_> = bindings
            .iter()
            .map(|(tool, spec)| yaltri_platform::gnome_keys::Binding {
                id: tool.to_string(),
                name: format!("Yaltri: {}", crate::settings_view::tool_name(*tool)),
                spec: spec.clone(),
                command: format!("{exe} toggle {tool}"),
            })
            .collect();
        let failed = gnome.apply(&gnome_bindings);
        let errors: Vec<HotkeyError> = bindings
            .iter()
            .filter(|(tool, _)| failed.contains(&tool.to_string()))
            .map(|(tool, _)| HotkeyError { tool: *tool, kind: yaltri_platform::hotkeys::HotkeyErrorKind::InUse })
            .collect();
        // Failures are retried by `retry_failed_hotkeys`.
        *applied = errors.is_empty().then_some(specs);
        Some(errors)
    }

    /// Other Wayland desktops: sends `bindings` to the desktop portal when
    /// they changed (the desktop may ask the user to approve them). `false`
    /// when the portal is not in use.
    #[cfg(target_os = "linux")]
    fn apply_portal_shortcuts(&mut self, bindings: &[(ToolId, String)]) -> bool {
        let suspended = self.hotkeys_suspended;
        let Some((portal, applied)) = self.portal.as_mut() else {
            return false;
        };
        let wanted: Vec<_> = if suspended {
            Vec::new()
        } else {
            bindings
                .iter()
                .map(|(tool, spec)| yaltri_platform::portal_keys::PortalBinding {
                    id: tool.to_string(),
                    description: format!("Yaltri: {}", crate::settings_view::tool_name(*tool)),
                    spec: spec.clone(),
                    command: Command::Toggle(*tool),
                })
                .collect()
        };
        if applied.as_ref() != Some(&wanted) {
            portal.apply(wanted.clone());
            *applied = Some(wanted);
        }
        true
    }

    fn set_hotkey_errors(&mut self, errors: Vec<HotkeyError>) {
        if errors != self.hotkey_errors {
            for e in &errors {
                tracing::warn!(tool = %e.tool, kind = ?e.kind, "shortcut problem");
            }
            if errors.is_empty() {
                tracing::info!("all shortcuts active");
            }
        }
        self.hotkey_errors = errors;
    }

    fn retry_failed_hotkeys(&mut self) {
        if self.hotkeys_suspended || self.hotkey_errors.is_empty() {
            return;
        }
        self.apply_hotkeys();
    }

    /// Persists `settings` and re-applies everything that depends on them.
    pub fn update_settings(&mut self, settings: Settings) -> Result<()> {
        settings.save(&self.paths.settings_file())?;
        if settings.games != self.settings.games
            && let Some(apps) = &self.apps
        {
            apps.set_games(settings.games.clone());
        }
        if settings.general.language != self.settings.general.language {
            crate::i18n::set_language(&settings.general.language);
        }
        if settings.general.theme != self.settings.general.theme {
            crate::theme::set_choice(crate::theme::ThemeChoice::from_key(&settings.general.theme));
        }
        self.settings = settings;
        self.settings_warning = None;
        self.apply_hotkeys();
        self.rebuild_search();
        let menu = self.tray_menu();
        if let Some(tray) = self.tray.as_mut() {
            tray.set_menu(menu)?;
        }
        Ok(())
    }
}

impl Daemon {
    /// (Re)creates search sources to match settings. The file indexer is only
    /// restarted when its scope changed; the apps provider is kept once started.
    fn rebuild_search(&mut self) {
        let files = &self.settings.files;
        if files.files_enabled {
            let scope = Scope::from_settings(files);
            if self.files.as_ref().map(|(s, _)| s) != Some(&scope) {
                self.files = None; // stop the old indexer first
                tracing::info!(roots = ?scope.roots, "starting file indexer");
                let indexer = FileIndexer::start(scope.clone(), self.paths.file_index());
                self.files = Some((scope, indexer));
            }
        } else {
            self.files = None;
        }
        if files.apps_enabled && self.apps.is_none() {
            self.apps = Some(Arc::new(AppsProvider::start(self.paths.data_dir.join("icons"), self.settings.games.clone())));
        }

        let mut providers: Vec<Box<dyn SearchProvider>> = vec![Box::new(CalcProvider::new())];
        if files.apps_enabled
            && let Some(apps) = &self.apps
        {
            providers.push(Box::new(apps.clone()));
        }
        if let Some((_, indexer)) = &self.files {
            providers.push(Box::new(FilesProvider::new(indexer)));
        }
        self.providers = Arc::new(providers);
    }

    /// Games found per launcher (empty while apps are off or still loading).
    pub fn game_counts(&self) -> std::collections::HashMap<yaltri_apps::games::Launcher, usize> {
        self.apps.as_ref().map(|a| a.game_counts()).unwrap_or_default()
    }

    pub fn index_status(&self) -> Option<yaltri_files::Status> {
        self.files.as_ref().map(|(_, i)| i.status())
    }

    pub fn rescan_files(&self) {
        if let Some((_, indexer)) = &self.files {
            indexer.rescan();
        }
    }

    pub fn record_color(&mut self, color: Rgb) {
        self.colors.push(color);
        if let Err(e) = self.colors.save(&self.paths.colors_file()) {
            tracing::warn!("saving colors failed: {e:#}");
        }
    }

    pub fn forget_color(&mut self, color: Rgb) {
        self.colors.remove(color);
        if let Err(e) = self.colors.save(&self.paths.colors_file()) {
            tracing::warn!("saving colors failed: {e:#}");
        }
        self.save_colors();
    }

    pub fn clear_colors(&mut self) {
        self.colors.clear();
        self.save_colors();
    }

    fn save_colors(&self) {
    }

    /// Remembers that the user opened `key`, for ranking.
    pub fn record_use(&self, key: &str) {
        let mut history = self.history.lock().unwrap();
        history.record(key);
        if let Err(e) = history.save(&self.paths.history_file()) {
            tracing::warn!("saving history failed: {e:#}");
        }
    }
}

/// First start (no settings file yet): apply defaults that live outside the
/// settings file, then write the file so this runs only once.
fn first_run_setup(settings: &Settings, paths: &Paths) {
    if settings.general.start_with_system
        && let Err(e) = yaltri_platform::autostart::set_enabled(true)
    {
        tracing::warn!("enabling autostart failed: {e:#}");
    }
    if let Err(e) = settings.save(&paths.settings_file()) {
        tracing::warn!("writing default settings failed: {e:#}");
    }
}

/// Tools that have a UI. All of them, for now; new tools start here as false.
pub fn is_implemented(tool: ToolId) -> bool {
    matches!(tool, ToolId::Launcher | ToolId::Picker)
}

/// Light/dark from the OS. Re-read before every command (each one opens or
/// closes a window), so a theme change in system settings is picked up.
fn sync_theme(cx: &mut App) {
    crate::theme::sync_components(cx);
}

fn handle(cmd: Command, cx: &mut App) {
    tracing::debug!(%cmd, "command");
    sync_theme(cx);
    match cmd {
        Command::Toggle(ToolId::Launcher) => {
            if cx.global::<Daemon>().settings.launcher.enabled {
                launcher::toggle(cx);
            }
        }
        Command::Toggle(ToolId::Picker) => {
            if cx.global::<Daemon>().settings.picker.enabled {
                picker::toggle(cx);
            }
        }
        Command::Settings => settings_view::open(cx),
        Command::Quit => {
            tracing::info!("quit requested");
            #[cfg(target_os = "linux")]
            if let Some((gnome, _)) = cx.global_mut::<Daemon>().gnome.as_mut() {
                gnome.restore_all();
            }
            cx.quit();
        }
    }
}
