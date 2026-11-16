//! Single-instance guard and command channel between `yaltri <cmd>`
//! invocations and the running daemon. Uses a named pipe on Windows and an
//! abstract-namespace Unix socket on Linux (no stale socket files).
//!
//! Wire format: one UTF-8 line per connection (`toggle launcher\n`, or
//! `ping\n` for "just check you're alive"); the daemon answers `ok\n` or
//! `err <message>\n`.

use std::io::{self, BufRead, BufReader, Write};
use std::thread;

use anyhow::{Context, Result, bail};
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Name, Stream, prelude::*};
use yaltri_core::Command;

use crate::CommandSender;

const PING: &str = "ping";

fn socket_name() -> io::Result<Name<'static>> {
    // Per-user name so two users on one machine do not talk to each other.
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "default".into());
    let user: String = user.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    format!("yaltri-{user}.sock").to_ns_name::<GenericNamespaced>().map(|n| n.into_owned())
}

/// Outcome of trying to reach an already running daemon.
#[derive(Debug, PartialEq, Eq)]
pub enum Delivery {
    /// A daemon is running and accepted the command.
    Delivered,
    /// Nobody is listening; the caller should become the daemon.
    NoDaemon,
}

/// Sends `cmd` (or a ping when `None`) to a running daemon.
pub fn send_to_daemon(cmd: Option<Command>) -> Result<Delivery> {
    let name = socket_name()?;
    let stream = match Stream::connect(name) {
        Ok(s) => s,
        Err(e) if matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused) => {
            return Ok(Delivery::NoDaemon);
        }
        Err(e) => return Err(e).context("connecting to running yaltri"),
    };
    allow_daemon_to_take_focus();
    let mut stream = BufReader::new(stream);
    let line = cmd.map_or_else(|| PING.to_owned(), |c| c.to_string());
    stream.get_mut().write_all(format!("{line}\n").as_bytes())?;
    let mut reply = String::new();
    if stream.read_line(&mut reply)? == 0 {
        return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
    }
    match reply.trim() {
        "ok" => Ok(Delivery::Delivered),
        other => bail!("daemon rejected command: {other}"),
    }
}

/// Windows only lets the foreground process raise windows. The CLI usually is
/// foreground (the user or a shortcut just started it), so hand that right to
/// the daemon before asking it to show a window.
/// Asks a running Yaltri to quit and waits until it is gone (or `timeout`
/// passes). Nothing running is fine.
pub fn quit_daemon(timeout: std::time::Duration) -> Result<()> {
    if send_to_daemon(Some(Command::Quit))? == Delivery::NoDaemon {
        return Ok(());
    }
    let start = std::time::Instant::now();
    loop {
        match send_to_daemon(None) {
            Ok(Delivery::NoDaemon) => return Ok(()),
            Ok(Delivery::Delivered) => {}
            // It exited holding our connection: on its way out.
            Err(e) if dropped_by_peer(&e) => {}
            Err(e) => return Err(e),
        }
        if start.elapsed() >= timeout {
            bail!("the running Yaltri did not quit");
        }
        thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Whether `e` is the daemon closing the connection before it answered,
/// as an exiting daemon does with the connections still queued for it.
fn dropped_by_peer(e: &anyhow::Error) -> bool {
    e.downcast_ref::<io::Error>().is_some_and(|e| {
        matches!(e.kind(), io::ErrorKind::ConnectionReset | io::ErrorKind::BrokenPipe | io::ErrorKind::UnexpectedEof)
    })
}

fn allow_daemon_to_take_focus() {
    #[cfg(windows)]
    // SAFETY: plain Win32 call without pointers.
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};
        AllowSetForegroundWindow(ASFW_ANY);
    }
}

/// Becomes the daemon: binds the socket and forwards incoming commands to
/// `tx` from a background thread. Fails with `AddrInUse` if another daemon
/// won the race.
pub fn serve(tx: CommandSender) -> Result<()> {
    let listener = ListenerOptions::new()
        .name(socket_name()?)
        .create_sync()
        .context("binding IPC socket")?;

    thread::Builder::new().name("yaltri-ipc".into()).spawn(move || {
        for conn in listener.incoming() {
            let conn = match conn {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(%e, "ipc accept failed");
                    continue;
                }
            };
            if let Err(e) = handle(conn, &tx) {
                tracing::warn!(%e, "ipc connection failed");
            }
            if tx.is_closed() {
                break;
            }
        }
    })?;
    Ok(())
}

fn handle(conn: Stream, tx: &CommandSender) -> io::Result<()> {
    let mut conn = BufReader::new(conn);
    let mut line = String::new();
    conn.read_line(&mut line)?;
    let line = line.trim();
    let reply = if line == PING {
        "ok".to_owned()
    } else {
        match line.parse::<Command>() {
            Ok(cmd) => {
                tracing::info!(%cmd, "ipc command");
                let _ = tx.unbounded_send(cmd);
                "ok".to_owned()
            }
            Err(e) => format!("err {e}"),
        }
    };
    conn.get_mut().write_all(format!("{reply}\n").as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dropped_connection_means_the_daemon_is_leaving() {
        for kind in [io::ErrorKind::ConnectionReset, io::ErrorKind::BrokenPipe, io::ErrorKind::UnexpectedEof] {
            assert!(dropped_by_peer(&io::Error::from(kind).into()), "{kind:?}");
        }
        let wrapped = anyhow::Error::from(io::Error::from(io::ErrorKind::ConnectionReset)).context("pinging");
        assert!(dropped_by_peer(&wrapped));
        assert!(!dropped_by_peer(&io::Error::from(io::ErrorKind::PermissionDenied).into()));
        assert!(!dropped_by_peer(&anyhow::anyhow!("daemon rejected command: err x")));
    }
}
