//! The long-lived tray process. Owns settings, the tray icon, global hotkeys
//! and the tool windows; every request arrives as a `Command`.

use std::sync::{Arc, Mutex};

use anyhow::Result;
use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui_kit::{App, AnyWindowHandle, Global, QuitMode};
use yaltri_apps::AppsProvider;
use yaltri_calc::CalcProvider;
use yaltri_core::paths::Paths;
use yaltri_core::{Command, History, SearchProvider, Settings, ToolId};
use yaltri_files::{FileIndexer, FilesProvider, Scope};
use yaltri_platform::CommandSender;
use yaltri_platform::hotkeys::{HotkeyError, Hotkeys};
use yaltri_platform::session::Session;
use yaltri_platform::tray::{self, Tray};

use crate::{launcher, settings_view};

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
    pub settings_window: Option<AnyWindowHandle>,
    tx: CommandSender,
    tray: Option<Tray>,
    hotkeys: Option<Hotkeys>,
    files: Option<(Scope, FileIndexer)>,
    apps: Option<Arc<AppsProvider>>,
}

impl Global for Daemon {}

pub fn run(paths: Paths, session: Session, initial: Option<Command>) -> Result<()> {
    let loaded = Settings::load(&paths.settings_file())?;
    let (tx, mut rx) = mpsc::unbounded::<Command>();

    // Bind the IPC socket before the UI so a racing second instance sees us.
    yaltri_platform::ipc::serve(tx.clone())?;

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        // Tray app: GPUI's default on Windows/Linux quits when the last
        // window closes. Only `Command::Quit` may end the process.
        .with_quit_mode(QuitMode::Explicit)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);

            let history = History::load(&paths.history_file());
            let mut daemon = Daemon {
                paths,
                session,
                settings: loaded.settings,
                settings_warning: loaded
                    .recovered_from
                    .map(|e| format!("Ayar dosyası bozuktu, varsayılanlara dönüldü (yedek: settings.toml.bak): {e}")),
                hotkey_errors: Vec::new(),
                providers: Arc::new(Vec::new()),
                history: Arc::new(Mutex::new(history)),
                launcher_window: None,
                settings_window: None,
                tx: tx.clone(),
                tray: None,
                hotkeys: None,
                files: None,
                apps: None,
            };
            daemon.start_platform_services();
            daemon.rebuild_search();
            cx.set_global(daemon);

            if let Some(cmd) = initial {
                handle(cmd, cx);
            }

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
        match Tray::spawn(self.tx.clone(), self.tray_menu()) {
            Ok(t) => self.tray = Some(t),
            Err(e) => tracing::error!("tray unavailable: {e:#}"),
        }
        if self.session.supports_global_hotkeys() {
            match Hotkeys::new(self.tx.clone()) {
                Ok(h) => self.hotkeys = Some(h),
                Err(e) => tracing::error!("{e:#}"),
            }
        } else {
            tracing::info!("no global hotkey support in this session; bind `yaltri toggle <tool>` in the desktop settings");
        }
        self.apply_hotkeys();
    }

    fn tray_menu(&self) -> Vec<Option<tray::MenuEntry>> {
        let enabled: Vec<ToolId> = ToolId::ALL
            .into_iter()
            .filter(|t| self.settings.is_enabled(*t) && is_implemented(*t))
            .collect();
        tray::menu_for(&enabled)
    }

    fn apply_hotkeys(&mut self) {
        let Some(hotkeys) = self.hotkeys.as_mut() else {
            return;
        };
        let bindings: Vec<(ToolId, String)> = ToolId::ALL
            .into_iter()
            .filter(|t| self.settings.is_enabled(*t) && is_implemented(*t))
            .map(|t| (t, self.settings.hotkey(t).to_owned()))
            .collect();
        self.hotkey_errors = hotkeys.apply(&bindings);
    }

    /// Persists `settings` and re-applies everything that depends on them.
    pub fn update_settings(&mut self, settings: Settings) -> Result<()> {
        settings.save(&self.paths.settings_file())?;
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
            self.apps = Some(Arc::new(AppsProvider::start()));
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

    pub fn index_status(&self) -> Option<yaltri_files::Status> {
        self.files.as_ref().map(|(_, i)| i.status())
    }

    pub fn rescan_files(&self) {
        if let Some((_, indexer)) = &self.files {
            indexer.rescan();
        }
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

/// Tools that have a UI yet. The picker is planned for milestone 2.
pub fn is_implemented(tool: ToolId) -> bool {
    matches!(tool, ToolId::Launcher)
}

fn handle(cmd: Command, cx: &mut App) {
    tracing::debug!(%cmd, "command");
    match cmd {
        Command::Toggle(ToolId::Launcher) => {
            if cx.global::<Daemon>().settings.launcher.enabled {
                launcher::toggle(cx);
            }
        }
        Command::Toggle(ToolId::Picker) => {
            tracing::info!("color picker is not implemented yet");
        }
        Command::Settings => settings_view::open(cx),
        Command::Quit => {
            tracing::info!("quit requested");
            cx.quit();
        }
    }
}
