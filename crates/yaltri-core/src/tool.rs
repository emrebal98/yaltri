use std::fmt;
use std::str::FromStr;

/// Every tool Yaltri ships. Tools are compiled in; there is no dynamic plugin
/// loading.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolId {
    Launcher,
    Picker,
    /// Keeps the focused window above all others.
    Pin,
}

impl ToolId {
    pub const ALL: [ToolId; 3] = [ToolId::Launcher, ToolId::Picker, ToolId::Pin];

    /// Stable machine name, used in config keys and on the command line.
    pub fn as_str(self) -> &'static str {
        match self {
            ToolId::Launcher => "launcher",
            ToolId::Picker => "picker",
            ToolId::Pin => "pin",
        }
    }

    /// Human readable name shown in the UI.
    pub fn display_name(self) -> &'static str {
        match self {
            ToolId::Launcher => "Arama",
            ToolId::Picker => "Renk Seçici",
            ToolId::Pin => "Üstte Tut",
        }
    }
}

impl fmt::Display for ToolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ToolId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "launcher" | "search" => Ok(ToolId::Launcher),
            "picker" | "color" | "colorpicker" => Ok(ToolId::Picker),
            "pin" | "top" | "ontop" | "alwaysontop" => Ok(ToolId::Pin),
            other => Err(format!("unknown tool: {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_aliases() {
        for id in ToolId::ALL {
            assert_eq!(id.as_str().parse::<ToolId>().unwrap(), id);
        }
        assert_eq!("Search".parse::<ToolId>().unwrap(), ToolId::Launcher);
        assert_eq!("top".parse::<ToolId>().unwrap(), ToolId::Pin);
        assert_eq!("OnTop".parse::<ToolId>().unwrap(), ToolId::Pin);
        assert!("nope".parse::<ToolId>().is_err());
    }
}
