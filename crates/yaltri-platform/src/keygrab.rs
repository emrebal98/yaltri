//! Captures every key press while a shortcut is being recorded, before the
//! desktop sees it.
//!
//! Without it, combinations another program or the shell owns never reach
//! our window, so the recorder would silently see nothing:
//! - Windows: PowerToys' `Win+Shift+C`, the shell's `Win+E`, and a lone Win
//!   release opens the Start menu. A low-level keyboard hook (like
//!   PowerToys) swallows every key while active.
//! - Linux X11: GNOME's window manager holds a passive grab on Super, so a
//!   Super press opens the Activities overview and never reaches the app.
//!   An active keyboard grab takes precedence over passive grabs.
//!   Wayland has no equivalent; the recorder falls back to in-window keys.

use std::borrow::Cow;

use crate::keys::{self, RecordError};

/// What the recorder learns from the grab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Grabbed {
    /// Currently held modifiers, for live display.
    Modifiers { ctrl: bool, alt: bool, shift: bool, win: bool },
    Combo(Result<String, RecordError>),
    Cancel,
    Clear,
}

/// One physical key, as the platform layers report it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Key {
    Ctrl,
    Alt,
    Shift,
    Win,
    /// GPUI-style key name `keys` understands; `None` if unsupported.
    Named(Option<Cow<'static, str>>),
}

type Sink = Box<dyn Fn(Grabbed) + Send>;

/// Held modifiers plus where to report; shared by the platform layers.
#[derive(Default)]
struct Recorder {
    sink: Option<Sink>,
    ctrl: bool,
    alt: bool,
    shift: bool,
    win: bool,
}

impl Recorder {
    fn new(sink: impl Fn(Grabbed) + Send + 'static) -> Self {
        Self { sink: Some(Box::new(sink)), ..Self::default() }
    }

    fn key(&mut self, key: Key, down: bool) {
        let modifier = match key {
            Key::Ctrl => Some(&mut self.ctrl),
            Key::Alt => Some(&mut self.alt),
            Key::Shift => Some(&mut self.shift),
            Key::Win => Some(&mut self.win),
            Key::Named(_) => None,
        };
        if let Some(flag) = modifier {
            // Held keys auto-repeat; only report real changes.
            if *flag != down {
                *flag = down;
                let event = Grabbed::Modifiers { ctrl: self.ctrl, alt: self.alt, shift: self.shift, win: self.win };
                self.emit(event);
            }
            return;
        }
        let Key::Named(name) = key else { return };
        if !down {
            return;
        }
        let any = self.ctrl || self.alt || self.shift || self.win;
        let event = match (name.as_deref(), any) {
            (Some("escape"), false) => Grabbed::Cancel,
            (Some("backspace" | "delete"), false) => Grabbed::Clear,
            (Some(name), _) => Grabbed::Combo(keys::spec_from_press(self.ctrl, self.alt, self.shift, self.win, name)),
            (None, _) => Grabbed::Combo(Err(RecordError::UnsupportedKey)),
        };
        self.emit(event);
    }

    fn emit(&self, event: Grabbed) {
        if let Some(sink) = &self.sink {
            sink(event);
        }
    }
}

pub use imp::KeyGrab;

#[cfg(windows)]
mod imp {
    use std::sync::Mutex;

    use anyhow::{Result, bail};
    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, HHOOK, KBDLLHOOKSTRUCT, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN,
        WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    use super::*;

    static STATE: Mutex<Option<Recorder>> = Mutex::new(None);

    /// Active grab; dropping it removes the hook.
    pub struct KeyGrab {
        hook: HHOOK,
    }

    impl KeyGrab {
        /// Installs the hook on the calling thread, which must pump messages
        /// (GPUI's main thread does). `sink` is called on that thread.
        pub fn start(sink: impl Fn(Grabbed) + Send + 'static) -> Result<Self> {
            *STATE.lock().unwrap() = Some(Recorder::new(sink));
            // SAFETY: installs a hook procedure defined in this module; the
            // module handle of our own executable is valid for its lifetime.
            let hook =
                unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), GetModuleHandleW(std::ptr::null()), 0) };
            if hook.is_null() {
                *STATE.lock().unwrap() = None;
                bail!("installing keyboard hook failed");
            }
            Ok(Self { hook })
        }
    }

    impl Drop for KeyGrab {
        fn drop(&mut self) {
            // SAFETY: `hook` came from SetWindowsHookExW and is removed once.
            unsafe { UnhookWindowsHookEx(self.hook) };
            *STATE.lock().unwrap() = None;
        }
    }

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code < 0 {
            // SAFETY: required pass-through for negative codes.
            return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
        }
        // SAFETY: for WH_KEYBOARD_LL, lparam points to a KBDLLHOOKSTRUCT.
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        let down = matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        let up = matches!(wparam as u32, WM_KEYUP | WM_SYSKEYUP);
        if (down || up)
            && let Ok(mut state) = STATE.try_lock()
            && let Some(recorder) = state.as_mut()
        {
            recorder.key(vk_key(info.vkCode), down);
            // Swallow: the key must not reach PowerToys, the shell or any app.
            return 1;
        }
        // SAFETY: pass everything else on unchanged.
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
    }

    pub(super) fn vk_key(vk: u32) -> Key {
        match vk {
            0x10 | 0xA0 | 0xA1 => Key::Shift,
            0x11 | 0xA2 | 0xA3 => Key::Ctrl,
            0x12 | 0xA4 | 0xA5 => Key::Alt,
            0x5B | 0x5C => Key::Win,
            _ => Key::Named(vk_name(vk)),
        }
    }

    /// Virtual-key code to the GPUI-style key name `keys` understands.
    pub(super) fn vk_name(vk: u32) -> Option<Cow<'static, str>> {
        let fixed = match vk {
            0x08 => "backspace",
            0x09 => "tab",
            0x0D => "enter",
            0x13 => "pause",
            0x1B => "escape",
            0x20 => "space",
            0x21 => "pageup",
            0x22 => "pagedown",
            0x23 => "end",
            0x24 => "home",
            0x25 => "left",
            0x26 => "up",
            0x27 => "right",
            0x28 => "down",
            0x2C => "printscreen",
            0x2D => "insert",
            0x2E => "delete",
            0xBA => ";",
            0xBB => "=",
            0xBC => ",",
            0xBD => "-",
            0xBE => ".",
            0xBF => "/",
            0xC0 => "`",
            0xDB => "[",
            0xDC => "\\",
            0xDD => "]",
            0xDE => "'",
            _ => "",
        };
        if !fixed.is_empty() {
            return Some(Cow::Borrowed(fixed));
        }
        match vk {
            0x30..=0x39 | 0x41..=0x5A => Some(Cow::Owned((vk as u8 as char).to_ascii_lowercase().to_string())),
            0x70..=0x87 => Some(Cow::Owned(format!("f{}", vk - 0x6F))),
            _ => None,
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::JoinHandle;
    use std::time::Duration;

    use anyhow::{Context as _, Result, bail};
    use x11rb::connection::Connection as _;
    use x11rb::protocol::Event;
    use x11rb::protocol::xproto::{ConnectionExt as _, GrabMode, GrabStatus};

    use super::*;

    /// Active X11 keyboard grab; dropping it releases the keyboard.
    pub struct KeyGrab {
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl KeyGrab {
        /// Grabs the keyboard for `window` (the recorder's own, focused X11
        /// window: grabbing on it sends no focus-out, so the window stays
        /// active). `sink` is called on a background thread.
        pub fn start(window: u32, sink: impl Fn(Grabbed) + Send + 'static) -> Result<Self> {
            let (conn, _) = x11rb::connect(None).context("connecting to X server")?;
            let (min, max) = (conn.setup().min_keycode, conn.setup().max_keycode);
            let mapping =
                conn.get_keyboard_mapping(min, max - min + 1)?.reply().context("reading the keyboard mapping")?;
            let per = mapping.keysyms_per_keycode as usize;
            // First (unshifted) keysym of each keycode.
            let keysym = move |code: u8| mapping.keysyms.get((code.checked_sub(min)? as usize) * per).copied();

            // The window manager may hold the keyboard for a moment.
            let mut status = GrabStatus::ALREADY_GRABBED;
            for _ in 0..20 {
                status = conn
                    .grab_keyboard(false, window, x11rb::CURRENT_TIME, GrabMode::ASYNC, GrabMode::ASYNC)?
                    .reply()
                    .context("grabbing the keyboard")?
                    .status;
                if status == GrabStatus::SUCCESS {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            if status != GrabStatus::SUCCESS {
                bail!("keyboard grab refused: {status:?}");
            }

            let stop = Arc::new(AtomicBool::new(false));
            let thread_stop = stop.clone();
            let thread = std::thread::spawn(move || {
                let mut recorder = Recorder::new(sink);
                while !thread_stop.load(Ordering::Relaxed) {
                    match conn.poll_for_event() {
                        Ok(Some(Event::KeyPress(e))) => recorder.key(keysym_key(keysym(e.detail).unwrap_or(0)), true),
                        Ok(Some(Event::KeyRelease(e))) => {
                            recorder.key(keysym_key(keysym(e.detail).unwrap_or(0)), false)
                        }
                        Ok(Some(_)) => {}
                        Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                        Err(e) => {
                            tracing::warn!("keyboard grab connection lost: {e}");
                            return;
                        }
                    }
                }
                let _ = conn.ungrab_keyboard(x11rb::CURRENT_TIME);
                let _ = conn.flush();
            });
            Ok(Self { stop, thread: Some(thread) })
        }
    }

    impl Drop for KeyGrab {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    pub(super) fn keysym_key(sym: u32) -> Key {
        match sym {
            0xffe1 | 0xffe2 => Key::Shift,
            0xffe3 | 0xffe4 => Key::Ctrl,
            // Alt_L/R and Meta_L/R.
            0xffe7..=0xffea => Key::Alt,
            0xffeb | 0xffec => Key::Win,
            _ => Key::Named(keysym_name(sym)),
        }
    }

    /// X keysym to the GPUI-style key name `keys` understands.
    pub(super) fn keysym_name(sym: u32) -> Option<Cow<'static, str>> {
        let fixed = match sym {
            0xff08 => "backspace",
            0xff09 => "tab",
            0xff0d | 0xff8d => "enter",
            0xff13 => "pause",
            0xff1b => "escape",
            0x20 => "space",
            0xff55 => "pageup",
            0xff56 => "pagedown",
            0xff57 => "end",
            0xff50 => "home",
            0xff51 => "left",
            0xff52 => "up",
            0xff53 => "right",
            0xff54 => "down",
            0xff61 => "printscreen",
            0xff63 => "insert",
            0xffff => "delete",
            0x3b => ";",
            0x3d => "=",
            0x2c => ",",
            0x2d => "-",
            0x2e => ".",
            0x2f => "/",
            0x60 => "`",
            0x5b => "[",
            0x5c => "\\",
            0x5d => "]",
            0x27 => "'",
            _ => "",
        };
        if !fixed.is_empty() {
            return Some(Cow::Borrowed(fixed));
        }
        match sym {
            0x30..=0x39 | 0x61..=0x7a => Some(Cow::Owned((sym as u8 as char).to_string())),
            0xffbe..=0xffd5 => Some(Cow::Owned(format!("f{}", sym - 0xffbd))),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn run(keys: &[(Key, bool)]) -> Vec<Grabbed> {
        let got = Arc::new(Mutex::new(Vec::new()));
        let sink_got = got.clone();
        let mut recorder = Recorder::new(move |e| sink_got.lock().unwrap().push(e));
        for (key, down) in keys {
            recorder.key(key.clone(), *down);
        }
        got.lock().unwrap().clone()
    }

    fn named(name: &'static str) -> Key {
        Key::Named(Some(Cow::Borrowed(name)))
    }

    #[test]
    fn records_win_shift_c() {
        let events = run(&[(Key::Win, true), (Key::Shift, true), (named("c"), true), (named("c"), false)]);
        assert_eq!(events.last(), Some(&Grabbed::Combo(Ok(format!("{}+Shift+C", keys::SUPER_NAME)))));
        assert!(matches!(events[1], Grabbed::Modifiers { win: true, shift: true, .. }));
    }

    #[test]
    fn escape_and_backspace_alone_cancel_and_clear() {
        assert_eq!(run(&[(named("escape"), true)]), [Grabbed::Cancel]);
        assert_eq!(run(&[(named("backspace"), true)]), [Grabbed::Clear]);
        // With a modifier they are ordinary keys.
        assert_eq!(
            run(&[(Key::Ctrl, true), (named("escape"), true)]).last(),
            Some(&Grabbed::Combo(Ok("Ctrl+Escape".into())))
        );
    }

    #[test]
    fn released_modifiers_no_longer_count() {
        let events = run(&[(Key::Ctrl, true), (Key::Ctrl, false), (named("c"), true)]);
        assert_eq!(events.last(), Some(&Grabbed::Combo(Err(RecordError::NeedsModifier))));
    }

    #[test]
    fn auto_repeated_modifiers_report_once() {
        let events = run(&[(Key::Win, true), (Key::Win, true), (Key::Win, true)]);
        assert_eq!(events.len(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn every_mapped_vk_parses_as_a_hotkey() {
        for vk in (0x08..=0xDE).filter(|vk| imp::vk_name(*vk).is_some()) {
            let name = imp::vk_name(vk).unwrap();
            let spec = keys::spec_from_press(true, true, false, false, &name).unwrap();
            assert!(crate::hotkeys::parse(&spec).is_ok(), "vk {vk:#x} -> {spec}");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn every_mapped_keysym_parses_as_a_hotkey() {
        let syms = (0x20..=0x7e).chain(0xff00..=0xffff);
        for sym in syms.filter(|s| imp::keysym_name(*s).is_some()) {
            let name = imp::keysym_name(sym).unwrap();
            let spec = keys::spec_from_press(true, true, false, false, &name).unwrap();
            assert!(crate::hotkeys::parse(&spec).is_ok(), "keysym {sym:#x} -> {spec}");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn super_and_letters_map_like_windows() {
        assert_eq!(imp::keysym_key(0xffeb), Key::Win);
        assert_eq!(imp::keysym_key(0xffe1), Key::Shift);
        assert_eq!(imp::keysym_key(0x63), named("c"));
        assert_eq!(imp::keysym_key(0xffc2), named("f5"));
        assert_eq!(imp::keysym_key(0x1a1), Key::Named(None));
    }
}
