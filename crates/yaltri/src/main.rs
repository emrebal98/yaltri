// Release builds are tray apps: no console window on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod color_editor;
mod daemon;
mod i18n;
mod launcher;
mod picker;
mod picker_zoom;
mod settings_view;
mod shortcut_input;
mod theme;
mod ui;
mod update;

use std::process::ExitCode;

use anyhow::Result;
use yaltri_core::Command;
use yaltri_core::paths::Paths;
use yaltri_platform::ipc::{self, Delivery};
use yaltri_platform::session::Session;

const USAGE: &str = "\
kullanım: yaltri [komut]

komutlar:
  (yok)              arka planda başlat (zaten çalışıyorsa bir şey yapmaz)
  toggle launcher    arama penceresini aç/kapat
  toggle picker      renk seçiciyi aç/kapat
  settings           ayarlar penceresini aç
  quit               çalışan yaltri'yi kapat

Wayland'da global kısayolu masaüstü ayarlarından bu komutlara bağlayın,
ör. GNOME: Ayarlar > Klavye > Özel Kısayollar > `yaltri toggle launcher`.";

fn main() -> ExitCode {
    // Must run before any thread exists (env mutation is not thread-safe).
    let session = Session::detect();
    force_xwayland_on_gnome(session);

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let command = match Command::from_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("hata: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    match run(session, command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("hata: {e:#}");
            tracing::error!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(session: Session, command: Option<Command>) -> Result<()> {
    // A daemon is already running: hand it the command and leave.
    if ipc::send_to_daemon(command)? == Delivery::Delivered {
        return Ok(());
    }
    if command == Some(Command::Quit) {
        return Ok(());
    }

    let paths = Paths::detect()?;
    let _log_guard = init_logging(&paths);
    tracing::info!(?session, version = env!("CARGO_PKG_VERSION"), "starting daemon");

    daemon::run(paths, session, command)
}

/// GNOME on Wayland has no layer-shell and does not let clients place or
/// raise windows. Like Ulauncher, run through XWayland where both work.
fn force_xwayland_on_gnome(session: Session) {
    if session == Session::WaylandGnome && std::env::var_os("DISPLAY").is_some() {
        // SAFETY: called first thing in main, before any other thread starts.
        unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
    }
}

fn init_logging(paths: &Paths) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{EnvFilter, fmt};

    let filter = EnvFilter::try_from_env("YALTRI_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let file = std::fs::create_dir_all(paths.log_dir())
        .ok()
        .map(|()| tracing_appender::rolling::daily(paths.log_dir(), "yaltri.log"));
    let (file_layer, guard) = match file {
        Some(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            (Some(fmt::layer().with_ansi(false).with_writer(writer)), Some(guard))
        }
        None => (None, None),
    };
    tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(fmt::layer().with_writer(std::io::stderr))
        .init();

    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("panic: {info}");
        default_panic(info);
    }));
    guard
}
