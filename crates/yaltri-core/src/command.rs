//! Commands the running daemon understands. They arrive from the tray, from a
//! global hotkey, or over IPC from a second `yaltri <cmd>` invocation (which
//! is how Wayland users bind hotkeys: the compositor runs `yaltri toggle launcher`).

use std::fmt;
use std::str::FromStr;

use crate::ToolId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Show the tool if hidden, hide it if shown.
    Toggle(ToolId),
    /// Open (or focus) the settings window.
    Settings,
    /// Exit the daemon.
    Quit,
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Command::Toggle(tool) => write!(f, "toggle {tool}"),
            Command::Settings => f.write_str("settings"),
            Command::Quit => f.write_str("quit"),
        }
    }
}

impl FromStr for Command {
    type Err = String;

    /// Parses the wire / CLI form: `toggle <tool>`, `settings`, `quit`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split_whitespace();
        let verb = parts.next().ok_or("empty command")?;
        let cmd = match verb.to_ascii_lowercase().as_str() {
            "toggle" => {
                let tool = parts.next().ok_or("toggle needs a tool name")?;
                Command::Toggle(tool.parse()?)
            }
            "settings" => Command::Settings,
            "quit" => Command::Quit,
            other => return Err(format!("unknown command: {other}")),
        };
        if parts.next().is_some() {
            return Err(format!("too many arguments: {s}"));
        }
        Ok(cmd)
    }
}

impl Command {
    /// Parses process arguments (without argv[0]). `None` means "no command":
    /// start the daemon, or do nothing if it is already running.
    pub fn from_args<I, S>(args: I) -> Result<Option<Command>, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let joined = args
            .into_iter()
            .map(|a| a.as_ref().to_owned())
            .collect::<Vec<_>>()
            .join(" ");
        if joined.trim().is_empty() {
            Ok(None)
        } else {
            joined.parse().map(Some)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_displays() {
        for cmd in [
            Command::Toggle(ToolId::Launcher),
            Command::Toggle(ToolId::Picker),
            Command::Settings,
            Command::Quit,
        ] {
            assert_eq!(cmd.to_string().parse::<Command>().unwrap(), cmd);
        }
    }

    #[test]
    fn from_args() {
        assert_eq!(Command::from_args(Vec::<String>::new()).unwrap(), None);
        assert_eq!(
            Command::from_args(["toggle", "search"]).unwrap(),
            Some(Command::Toggle(ToolId::Launcher))
        );
        assert!(Command::from_args(["toggle"]).is_err());
        assert!(Command::from_args(["quit", "now"]).is_err());
    }
}
