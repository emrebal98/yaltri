//! System tray icon with a small menu. Menu item ids are the wire form of the
//! `Command` they trigger, so one parser handles tray, IPC and CLI.
//!
//! - Windows: `tray-icon` (needs the main-thread message loop GPUI runs).
//!   Left click opens search, right click the menu.
//! - Linux: `ksni` (StatusNotifierItem over D-Bus, own thread, no GTK). GNOME
//!   shows it through the AppIndicator extension Ubuntu ships by default and
//!   opens the menu on any click. That extension draws menu icons but ignores
//!   the dbusmenu `shortcut` property, so on GNOME the shortcut goes into the
//!   label instead.

use anyhow::Result;
use yaltri_core::{Command, ToolId};

use crate::CommandSender;

/// One entry in the tray menu; the caller supplies localized labels.
#[derive(Clone, Debug)]
pub struct MenuEntry {
    pub label: String,
    pub command: Command,
    /// Lucide SVG source drawn in the menu's text color (Windows).
    pub icon_svg: Option<&'static str>,
    /// Current shortcut, shown right-aligned in the menu.
    pub shortcut: Option<String>,
}

pub use imp::Tray;

#[cfg(windows)]
mod imp {
    use super::*;
    use anyhow::Context;
    use tray_icon::menu::{IconMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

    pub struct Tray {
        icon: TrayIcon,
        light_taskbar: bool,
    }

    /// The small-icon size Windows uses for the tray at the current DPI.
    fn tray_icon_size() -> u32 {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSMICON};
        // SAFETY: plain metric query.
        let size = unsafe { GetSystemMetrics(SM_CXSMICON) };
        if size > 0 { size as u32 } else { 16 }
    }

    fn tray_icon(light_taskbar: bool) -> Result<Icon> {
        let size = tray_icon_size();
        Icon::from_rgba(crate::icon::tray_rgba(size, light_taskbar), size, size).context("building tray icon")
    }

    impl Tray {
        /// `menu`: `None` entries are separators.
        pub fn spawn(tx: CommandSender, menu: Vec<Option<MenuEntry>>) -> Result<Self> {
            let menu_tx = tx.clone();
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                if let Ok(cmd) = event.id.0.parse::<Command>() {
                    let _ = menu_tx.unbounded_send(cmd);
                }
            }));
            TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } =
                    event
                {
                    let _ = tx.unbounded_send(Command::Toggle(ToolId::Launcher));
                }
            }));

            let light_taskbar = crate::icon::taskbar_is_light();
            let icon = TrayIconBuilder::new()
                .with_tooltip("Yaltri")
                .with_icon(tray_icon(light_taskbar)?)
                .with_menu(Box::new(build_menu(&menu)?))
                .with_menu_on_left_click(false)
                .build()
                .context("creating tray icon")?;
            Ok(Self { icon, light_taskbar })
        }

        pub fn set_menu(&mut self, menu: Vec<Option<MenuEntry>>) -> Result<()> {
            self.icon.set_menu(Some(Box::new(build_menu(&menu)?)));
            Ok(())
        }

        /// Re-reads the taskbar theme and swaps the icon if it changed.
        pub fn refresh_theme(&mut self) -> Result<()> {
            let light = crate::icon::taskbar_is_light();
            if light != self.light_taskbar {
                self.icon.set_icon(Some(tray_icon(light)?))?;
                self.light_taskbar = light;
            }
            Ok(())
        }
    }

    /// Lets Win32 popup menus follow the system light/dark setting. Without
    /// this they are always light. Uses uxtheme's unnamed exports (ordinal
    /// 135 SetPreferredAppMode(AllowDark), 136 FlushMenuThemes) like other
    /// Win32 apps do; does nothing where they are missing.
    fn follow_system_menu_theme() {
        use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
        let name: Vec<u16> = "uxtheme.dll\0".encode_utf16().collect();
        // SAFETY: the ordinals have these signatures since Windows 10 1903;
        // older builds simply lack them (null) or treat 1 as `true`.
        unsafe {
            let lib = LoadLibraryW(name.as_ptr());
            if lib.is_null() {
                return;
            }
            if let Some(f) = GetProcAddress(lib, 135usize as *const u8) {
                let set_preferred_app_mode: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(f);
                set_preferred_app_mode(1);
            }
            if let Some(f) = GetProcAddress(lib, 136usize as *const u8) {
                let flush_menu_themes: unsafe extern "system" fn() = std::mem::transmute(f);
                flush_menu_themes();
            }
        }
    }

    fn build_menu(entries: &[Option<MenuEntry>]) -> Result<Menu> {
        follow_system_menu_theme();
        // Native menus follow the apps theme.
        let ink = if crate::icon::apps_use_light_theme() { "#17181B" } else { "#ECEDEF" };
        let menu = Menu::new();
        for entry in entries {
            let Some(e) = entry else {
                menu.append(&PredefinedMenuItem::separator())?;
                continue;
            };
            // A tab right-aligns the shortcut in Win32 menus.
            let text = match &e.shortcut {
                Some(s) if !s.is_empty() => format!("{}\t{s}", e.label),
                _ => e.label.clone(),
            };
            let icon = e
                .icon_svg
                .and_then(|svg| crate::icon::svg_rgba(svg, 16, Some(ink)))
                .and_then(|rgba| tray_icon::menu::Icon::from_rgba(rgba, 16, 16).ok());
            match icon {
                Some(icon) => {
                    menu.append(&IconMenuItem::with_id(e.command.to_string(), text, true, Some(icon), None))?
                }
                None => menu.append(&MenuItem::with_id(e.command.to_string(), text, true, None))?,
            }
        }
        Ok(menu)
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use anyhow::Context;
    use ksni::blocking::{Handle, TrayMethods};

    pub struct Tray {
        handle: Handle<SniTray>,
    }

    struct SniTray {
        tx: CommandSender,
        menu: Vec<Option<MenuEntry>>,
        /// GNOME: the shell's UI font, to line the shortcuts up.
        font: Option<Vec<u8>>,
    }

    impl ksni::Tray for SniTray {
        fn id(&self) -> String {
            "yaltri".into()
        }
        fn title(&self) -> String {
            "Yaltri".into()
        }
        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            // Panels are assumed dark: the white mark.
            [16u32, 24, 32]
                .into_iter()
                .map(|size| ksni::Icon {
                    width: size as i32,
                    height: size as i32,
                    data: crate::icon::argb(crate::icon::tray_rgba(size, false)),
                })
                .collect()
        }
        fn activate(&mut self, _x: i32, _y: i32) {
            let _ = self.tx.unbounded_send(Command::Toggle(ToolId::Launcher));
        }
        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            use ksni::menu::StandardItem;
            let gnome = is_gnome();
            // Shell menus follow the panel, which we assume dark.
            let ink = if crate::icon::taskbar_is_light() { "#17181B" } else { "#ECEDEF" };
            let face = self.font.as_deref().and_then(|data| ttf_parser::Face::parse(data, 0).ok());
            let with_shortcut: Vec<&str> = self
                .menu
                .iter()
                .flatten()
                .filter(|e| e.shortcut.as_deref().is_some_and(|s| !s.is_empty()))
                .map(|e| e.label.as_str())
                .collect();
            let pads = shortcut_pads(&with_shortcut, |text| face.as_ref().and_then(|f| text_width(f, text)));
            self.menu
                .iter()
                .map(|entry| match entry {
                    None => ksni::MenuItem::Separator,
                    Some(e) => {
                        let cmd = e.command;
                        let shortcut = e.shortcut.as_deref().filter(|s| !s.is_empty());
                        StandardItem {
                            label: match (gnome, shortcut) {
                                (true, Some(s)) => {
                                    let pad = with_shortcut.iter().position(|l| *l == e.label).map_or(MIN_GAP, |i| pads[i]);
                                    format!("{}{}{s}", e.label, " ".repeat(pad))
                                }
                                _ => e.label.clone(),
                            },
                            icon_data: e
                                .icon_svg
                                .and_then(|svg| crate::icon::svg_png(svg, 16, Some(ink)))
                                .unwrap_or_default(),
                            // dbusmenu wants [["Control", "Alt", "Space"]].
                            shortcut: shortcut
                                .filter(|_| !gnome)
                                .iter()
                                .map(|spec| {
                                    spec.split('+')
                                        .map(|part| match part.trim() {
                                            "Ctrl" => "Control".to_owned(),
                                            "Win" => "Super".to_owned(),
                                            other => other.to_owned(),
                                        })
                                        .collect()
                                })
                                .collect(),
                            activate: Box::new(move |this: &mut Self| {
                                let _ = this.tx.unbounded_send(cmd);
                            }),
                            ..Default::default()
                        }
                        .into()
                    }
                })
                .collect()
        }
    }

    fn is_gnome() -> bool {
        std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.split(':').any(|d| d.eq_ignore_ascii_case("gnome")))
    }

    /// Spaces between the longest label and its shortcut.
    pub(super) const MIN_GAP: usize = 4;

    /// Spaces after each label so the shortcuts that follow start in one
    /// column. GNOME menus draw no shortcut column and a tab is one space
    /// wide there, so we pad with spaces: measured in the shell font when
    /// `width` can (off by at most half a space), else guessed as two
    /// spaces per letter.
    pub(super) fn shortcut_pads(labels: &[&str], width: impl Fn(&str) -> Option<f32>) -> Vec<usize> {
        let measured: Option<Vec<f32>> = labels.iter().map(|l| width(l)).collect();
        match (measured, width(" ")) {
            (Some(widths), Some(space)) if space > 0. => {
                let target = widths.iter().copied().fold(0., f32::max);
                widths.iter().map(|w| MIN_GAP + ((target - w) / space).round() as usize).collect()
            }
            _ => {
                let longest = labels.iter().map(|l| l.chars().count()).max().unwrap_or(0);
                labels.iter().map(|l| MIN_GAP + 2 * (longest - l.chars().count())).collect()
            }
        }
    }

    /// Advance width of `text` in font units; `None` if a glyph is missing.
    fn text_width(face: &ttf_parser::Face, text: &str) -> Option<f32> {
        text.chars().map(|c| face.glyph_index(c).and_then(|g| face.glyph_hor_advance(g)).map(f32::from)).sum()
    }

    /// The font file behind GNOME's interface font (`'Ubuntu 11'`).
    fn shell_font() -> Option<Vec<u8>> {
        let run = |cmd: &str, args: &[&str]| {
            let out = std::process::Command::new(cmd).args(args).output().ok()?;
            out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        };
        let name = run("gsettings", &["get", "org.gnome.desktop.interface", "font-name"])?;
        let name = name.trim_matches('\'');
        // Drop the trailing size.
        let family = name.rsplit_once(' ').filter(|(_, size)| size.parse::<f32>().is_ok()).map_or(name, |(f, _)| f);
        let file = run("fc-match", &["-f", "%{file}", family])?;
        std::fs::read(file).ok()
    }

    impl Tray {
        pub fn spawn(tx: CommandSender, menu: Vec<Option<MenuEntry>>) -> Result<Self> {
            let font = if is_gnome() { shell_font() } else { None };
            let handle =
                SniTray { tx, menu, font }.spawn().context("starting tray (GNOME needs the AppIndicator extension)")?;
            Ok(Self { handle })
        }

        pub fn set_menu(&mut self, menu: Vec<Option<MenuEntry>>) -> Result<()> {
            self.handle.update(|t| t.menu = menu);
            Ok(())
        }

        pub fn refresh_theme(&mut self) -> Result<()> {
            Ok(())
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::imp::{MIN_GAP, shortcut_pads};

    #[test]
    fn measured_labels_pad_to_the_widest() {
        // Space 2 units wide; labels 10 and 16 units.
        let width = |t: &str| Some(match t { " " => 2., "Search" => 10., "Color picker" => 16., _ => 0. });
        assert_eq!(shortcut_pads(&["Search", "Color picker"], width), [MIN_GAP + 3, MIN_GAP]);
    }

    #[test]
    fn unmeasurable_labels_fall_back_to_letter_count() {
        assert_eq!(shortcut_pads(&["Ab", "Abcd"], |_| None), [MIN_GAP + 4, MIN_GAP]);
    }
}

