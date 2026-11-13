//! Starting programs and showing files in the system file manager.

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

/// Starts `program` detached from Yaltri (it keeps running if Yaltri exits).
pub fn run(program: &str, args: &[String], terminal: bool) -> Result<()> {
    let mut cmd = if terminal { terminal_command(program, args)? } else {
        let mut c = Command::new(program);
        c.args(args);
        c
    };
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        cmd.current_dir(home);
    }
    detach(&mut cmd);
    cmd.spawn().with_context(|| format!("starting {program}"))?;
    Ok(())
}

#[cfg(unix)]
fn detach(cmd: &mut Command) {
    use std::os::unix::process::CommandExt as _;
    // New session: closing Yaltri must not take the app down with it.
    cmd.process_group(0);
}

#[cfg(windows)]
fn detach(cmd: &mut Command) {
    use std::os::windows::process::CommandExt as _;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

/// Wraps a command in the user's terminal emulator (`Terminal=true` apps).
fn terminal_command(program: &str, args: &[String]) -> Result<Command> {
    #[cfg(windows)]
    {
        let mut c = Command::new("cmd.exe");
        c.arg("/C").arg("start").arg("").arg(program).args(args);
        Ok(c)
    }
    #[cfg(not(windows))]
    {
        let candidates: Vec<(String, &[&str])> = std::env::var("TERMINAL")
            .ok()
            .map(|t| (t, &["-e"][..]))
            .into_iter()
            .chain([
                ("x-terminal-emulator".to_string(), &["-e"][..]),
                ("gnome-terminal".to_string(), &["--"][..]),
                ("konsole".to_string(), &["-e"][..]),
                ("kitty".to_string(), &[][..]),
                ("alacritty".to_string(), &["-e"][..]),
                ("foot".to_string(), &[][..]),
                ("xterm".to_string(), &["-e"][..]),
            ])
            .collect();
        for (term, sep) in candidates {
            if which(&term) {
                let mut c = Command::new(term);
                c.args(sep).arg(program).args(args);
                return Ok(c);
            }
        }
        anyhow::bail!("no terminal emulator found (set $TERMINAL)")
    }
}

#[cfg(not(windows))]
fn which(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

/// Opens the folder containing `path` with the item selected where possible.
pub fn reveal(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        // `explorer /select,<path>` must be one argument with the comma.
        use std::os::windows::process::CommandExt as _;
        Command::new("explorer.exe")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn()
            .context("starting explorer")?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        // The freedesktop FileManager1 interface selects the file in Nautilus,
        // Dolphin, Nemo...; fall back to opening the parent folder.
        let uri = format!("file://{}", path.display());
        let selected = Command::new("dbus-send")
            .args([
                "--session",
                "--dest=org.freedesktop.FileManager1",
                "--type=method_call",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
                &format!("array:string:{uri}"),
                "string:",
            ])
            .status()
            .is_ok_and(|s| s.success());
        if !selected {
            let parent = path.parent().unwrap_or(path);
            open::that_detached(parent).context("opening folder")?;
        }
        Ok(())
    }
}
