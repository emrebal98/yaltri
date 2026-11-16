// Release builds are tray apps: no console window on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod color_editor;
mod daemon;
mod i18n;
#[cfg(windows)]
mod install;
#[cfg(target_os = "linux")]
mod uninstall_linux;
mod launcher;
mod markup;
mod notice;
mod picker;
mod picker_zoom;
mod shot;
mod shot_output;
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
  toggle pin         öndeki pencereyi üstte tut / bırak
  toggle shot        ekran görüntüsü al
  toggle ocr         seçilen alandaki metni kopyala
  settings           ayarlar penceresini aç
  quit               çalışan yaltri'yi kapat
  update             indirilen güncellemeyi kur ve yeniden başlat (yoksa denetle)
  uninstall          yaltri'yi kaldır; ayarlar ve geçmiş için sorar
  uninstall --purge  yaltri'yi ayarları ve geçmişiyle birlikte kaldır

seçenekler:
  --foreground       terminalden ayrılma, günlükleri burada göster

Kısayolları kendisi kuramadığı masaüstlerinde (ör. Sway) bu komutları
masaüstünün kısayol ayarlarına bağlayın: `yaltri toggle launcher`.";

fn main() -> ExitCode {
    // Must run before any thread exists (env mutation is not thread-safe).
    let session = Session::detect();
    force_xwayland_on_gnome(session);

    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let foreground = take_flag(&mut args, "--foreground");
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if args.first().is_some_and(|a| a == "uninstall") {
        return match args[1..] {
            [] => uninstall(false),
            [ref flag] if flag == "--purge" => uninstall(true),
            _ => {
                eprintln!("hata: `uninstall` yalnızca `--purge` alır

{USAGE}");
                ExitCode::from(2)
            }
        };
    }
    let command = match Command::from_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("hata: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    match run(session, command, foreground) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("hata: {e:#}");
            tracing::error!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

fn uninstall(purge: bool) -> ExitCode {
    i18n::use_saved_language();
    #[cfg(windows)]
    let result = install::uninstall(purge);
    #[cfg(target_os = "linux")]
    let result = uninstall_linux::uninstall(purge);
    #[cfg(not(any(windows, target_os = "linux")))]
    let result: Result<()> = {
        let _ = purge;
        Err(anyhow::anyhow!("`uninstall` is not available on this system"))
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("hata: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(session: Session, command: Option<Command>, foreground: bool) -> Result<()> {
    let paths = Paths::detect()?;
    // Logging is for the daemon (and installing it), not for commands
    // forwarded to one; it can be set up only once.
    let mut log_guard = None;

    // Started from a download: install, start the installed copy, leave.
    #[cfg(windows)]
    if command.is_none() {
        log_guard = Some(init_logging(&paths));
        if matches!(install::self_install(), install::Outcome::Handed) {
            return Ok(());
        }
    }

    // A daemon is already running: hand it the command and leave.
    if ipc::send_to_daemon(command)? == Delivery::Delivered {
        return Ok(());
    }
    if command == Some(Command::Quit) {
        return Ok(());
    }

    // Linux: started by hand in a terminal, the daemon would stop when the
    // terminal closes. Start a detached copy instead and give the terminal
    // back. (Windows release builds have no console to begin with.)
    #[cfg(target_os = "linux")]
    if !foreground && started_from_terminal() {
        start_detached(command)?;
        i18n::use_saved_language();
        println!("{}", i18n::t("cli.started"));
        return Ok(());
    }
    #[cfg(not(target_os = "linux"))]
    let _ = foreground;

    let _log_guard = log_guard.unwrap_or_else(|| init_logging(&paths));
    tracing::info!(?session, version = env!("CARGO_PKG_VERSION"), "starting daemon");

    daemon::run(paths, session, command)
}

/// Removes every `flag` from `args`; whether there was one.
fn take_flag(args: &mut Vec<String>, flag: &str) -> bool {
    let before = args.len();
    args.retain(|a| a != flag);
    args.len() != before
}

#[cfg(target_os = "linux")]
fn started_from_terminal() -> bool {
    use std::io::IsTerminal as _;
    std::io::stdin().is_terminal() || std::io::stdout().is_terminal() || std::io::stderr().is_terminal()
}

/// Starts this program again as the daemon, in its own process group with
/// no terminal attached, handing it `command`.
#[cfg(target_os = "linux")]
fn start_detached(command: Option<Command>) -> Result<()> {
    use anyhow::Context as _;
    let exe = std::env::current_exe().context("locating the yaltri executable")?;
    let exe = exe.to_str().context("yaltri's path is not valid UTF-8")?;
    let mut args = vec!["--foreground".to_owned()];
    if let Some(command) = command {
        // The wire form is also the command line: `toggle launcher`.
        args.extend(command.to_string().split(' ').map(str::to_owned));
    }
    yaltri_platform::launch::run(exe, &args, false)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreground_flag_is_taken_out_of_the_command() {
        let mut args: Vec<String> = ["toggle", "--foreground", "launcher"].map(String::from).into();
        assert!(take_flag(&mut args, "--foreground"));
        assert_eq!(args, ["toggle", "launcher"]);
        assert_eq!(Command::from_args(&args).unwrap(), Some(Command::Toggle(yaltri_core::ToolId::Launcher)));
        assert!(!take_flag(&mut args, "--foreground"));
    }

    #[test]
    fn every_command_round_trips_through_its_command_line() {
        for cmd in [Command::Toggle(yaltri_core::ToolId::Launcher), Command::Toggle(yaltri_core::ToolId::Picker), Command::Settings, Command::Quit, Command::Update] {
            let line = cmd.to_string();
            assert_eq!(Command::from_args(line.split(' ')).unwrap(), Some(cmd), "{line}");
        }
    }
}
