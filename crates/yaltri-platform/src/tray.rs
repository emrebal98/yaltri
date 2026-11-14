//! System tray icon with a small menu. Menu item ids are the wire form of the
//! `Command` they trigger, so one parser handles tray, IPC and CLI.
//!
//! - Windows: `tray-icon` (needs the main-thread message loop GPUI runs).
//!   Left click opens search, right click the menu.
//! - Linux: `ksni` (StatusNotifierItem over D-Bus, own thread, no GTK). GNOME
//!   shows it through the AppIndicator extension Ubuntu ships by default and
//!   opens the menu on any click; menu icons are dropped there.

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
            self.menu
                .iter()
                .map(|entry| match entry {
                    None => ksni::MenuItem::Separator,
                    Some(e) => {
                        let cmd = e.command;
                        StandardItem {
                            label: e.label.clone(),
                            // dbusmenu wants [["Control", "Alt", "Space"]].
                            shortcut: e
                                .shortcut
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

    impl Tray {
        pub fn spawn(tx: CommandSender, menu: Vec<Option<MenuEntry>>) -> Result<Self> {
            let handle =
                SniTray { tx, menu }.spawn().context("starting tray (GNOME needs the AppIndicator extension)")?;
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
