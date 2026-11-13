//! System tray icon with a small menu. Menu item ids are the wire form of the
//! `Command` they trigger, so one parser handles tray, IPC and CLI.
//!
//! - Windows: `tray-icon` (needs the main-thread message loop GPUI runs).
//! - Linux: `ksni` (StatusNotifierItem over D-Bus, own thread, no GTK). GNOME
//!   shows it through the AppIndicator extension Ubuntu ships by default.

use anyhow::Result;
use yaltri_core::{Command, ToolId};

use crate::CommandSender;

/// One entry in the tray menu.
#[derive(Clone, Debug)]
pub struct MenuEntry {
    pub label: String,
    pub command: Command,
}

/// Default menu for the given enabled tools.
pub fn menu_for(enabled: &[ToolId]) -> Vec<Option<MenuEntry>> {
    let mut items: Vec<Option<MenuEntry>> = enabled
        .iter()
        .map(|t| Some(MenuEntry { label: t.display_name().into(), command: Command::Toggle(*t) }))
        .collect();
    if !items.is_empty() {
        items.push(None);
    }
    items.push(Some(MenuEntry { label: "Ayarlar".into(), command: Command::Settings }));
    items.push(None);
    items.push(Some(MenuEntry { label: "Çıkış".into(), command: Command::Quit }));
    items
}

pub use imp::Tray;

#[cfg(windows)]
mod imp {
    use super::*;
    use anyhow::Context;
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

    pub struct Tray {
        icon: TrayIcon,
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
            // Left click opens the launcher, right click shows the menu.
            TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    let _ = tx.unbounded_send(Command::Toggle(ToolId::Launcher));
                }
            }));

            let icon = Icon::from_rgba(crate::icon::rgba(), crate::icon::SIZE, crate::icon::SIZE)
                .context("building tray icon")?;
            let icon = TrayIconBuilder::new()
                .with_tooltip("Yaltri")
                .with_icon(icon)
                .with_menu(Box::new(build_menu(&menu)?))
                .with_menu_on_left_click(false)
                .build()
                .context("creating tray icon")?;
            Ok(Self { icon })
        }

        pub fn set_menu(&mut self, menu: Vec<Option<MenuEntry>>) -> Result<()> {
            self.icon.set_menu(Some(Box::new(build_menu(&menu)?)));
            Ok(())
        }
    }

    fn build_menu(entries: &[Option<MenuEntry>]) -> Result<Menu> {
        let menu = Menu::new();
        for entry in entries {
            match entry {
                Some(e) => menu.append(&MenuItem::with_id(e.command.to_string(), &e.label, true, None))?,
                None => menu.append(&PredefinedMenuItem::separator())?,
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
            vec![ksni::Icon {
                width: crate::icon::SIZE as i32,
                height: crate::icon::SIZE as i32,
                data: crate::icon::argb(),
            }]
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
            let handle = SniTray { tx, menu }
                .spawn()
                .context("starting tray (GNOME needs the AppIndicator extension)")?;
            Ok(Self { handle })
        }

        pub fn set_menu(&mut self, menu: Vec<Option<MenuEntry>>) -> Result<()> {
            self.handle.update(|t| t.menu = menu);
            Ok(())
        }
    }
}
