//! Windows: captures every key press system-wide while a shortcut is being
//! recorded (low-level keyboard hook, like PowerToys).
//!
//! Without it, combinations another program registered (PowerToys'
//! `Win+Shift+C`, the shell's `Win+E`) never reach our window, so the
//! recorder would silently see nothing. All keys are swallowed while the
//! grab is active, so nothing fires and a lone Win release cannot open
//! the Start menu.

use std::sync::Mutex;

use anyhow::{Result, bail};
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, HHOOK, KBDLLHOOKSTRUCT, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

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

type Sink = Box<dyn Fn(Grabbed) + Send>;

#[derive(Default)]
struct State {
    sink: Option<Sink>,
    ctrl: bool,
    alt: bool,
    shift: bool,
    win: bool,
}

static STATE: Mutex<State> =
    Mutex::new(State { sink: None, ctrl: false, alt: false, shift: false, win: false });

/// Active grab; dropping it removes the hook.
pub struct KeyGrab {
    hook: HHOOK,
}

impl KeyGrab {
    /// Installs the hook on the calling thread, which must pump messages
    /// (GPUI's main thread does). `sink` is called on that thread.
    pub fn start(sink: impl Fn(Grabbed) + Send + 'static) -> Result<Self> {
        {
            let mut state = STATE.lock().unwrap();
            *state = State { sink: Some(Box::new(sink)), ..State::default() };
        }
        // SAFETY: installs a hook procedure defined in this module; the
        // module handle of our own executable is valid for its lifetime.
        let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), GetModuleHandleW(std::ptr::null()), 0) };
        if hook.is_null() {
            STATE.lock().unwrap().sink = None;
            bail!("installing keyboard hook failed");
        }
        Ok(Self { hook })
    }
}

impl Drop for KeyGrab {
    fn drop(&mut self) {
        // SAFETY: `hook` came from SetWindowsHookExW and is removed once.
        unsafe { UnhookWindowsHookEx(self.hook) };
        STATE.lock().unwrap().sink = None;
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
        && state.sink.is_some()
    {
        handle(&mut state, info.vkCode, down);
        // Swallow: the key must not reach PowerToys, the shell or any app.
        return 1;
    }
    // SAFETY: pass everything else on unchanged.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

fn handle(state: &mut State, vk: u32, down: bool) {
    let modifier = match vk {
        0x10 | 0xA0 | 0xA1 => Some(&mut state.shift),
        0x11 | 0xA2 | 0xA3 => Some(&mut state.ctrl),
        0x12 | 0xA4 | 0xA5 => Some(&mut state.alt),
        0x5B | 0x5C => Some(&mut state.win),
        _ => None,
    };
    if let Some(flag) = modifier {
        *flag = down;
        let event = Grabbed::Modifiers { ctrl: state.ctrl, alt: state.alt, shift: state.shift, win: state.win };
        if let Some(sink) = &state.sink {
            sink(event);
        }
        return;
    }
    if !down {
        return;
    }
    let any = state.ctrl || state.alt || state.shift || state.win;
    let name = vk_name(vk);
    let event = match (name.as_deref(), any) {
        (Some("escape"), false) => Grabbed::Cancel,
        (Some("backspace" | "delete"), false) => Grabbed::Clear,
        (Some(name), _) => Grabbed::Combo(keys::spec_from_press(state.ctrl, state.alt, state.shift, state.win, name)),
        (None, _) => Grabbed::Combo(Err(RecordError::UnsupportedKey)),
    };
    if let Some(sink) = &state.sink {
        sink(event);
    }
}

/// Virtual-key code to the GPUI-style key name `keys` understands.
fn vk_name(vk: u32) -> Option<std::borrow::Cow<'static, str>> {
    use std::borrow::Cow;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex};

    fn run(events: &[(u32, bool)]) -> Vec<Grabbed> {
        let got = Arc::new(StdMutex::new(Vec::new()));
        let sink_got = got.clone();
        let mut state = State { sink: Some(Box::new(move |e| sink_got.lock().unwrap().push(e))), ..State::default() };
        for &(vk, down) in events {
            handle(&mut state, vk, down);
        }
        let out = got.lock().unwrap().clone();
        out
    }

    #[test]
    fn records_win_shift_c() {
        let events = run(&[(0x5B, true), (0xA0, true), (0x43, true), (0x43, false)]);
        assert_eq!(events.last(), Some(&Grabbed::Combo(Ok(format!("{}+Shift+C", keys::SUPER_NAME)))));
        assert!(matches!(events[1], Grabbed::Modifiers { win: true, shift: true, .. }));
    }

    #[test]
    fn escape_and_backspace_alone_cancel_and_clear() {
        assert_eq!(run(&[(0x1B, true)]), [Grabbed::Cancel]);
        assert_eq!(run(&[(0x08, true)]), [Grabbed::Clear]);
        // With a modifier they are ordinary keys.
        assert_eq!(run(&[(0xA2, true), (0x1B, true)]).last(), Some(&Grabbed::Combo(Ok("Ctrl+Escape".into()))));
    }

    #[test]
    fn released_modifiers_no_longer_count() {
        let events = run(&[(0xA2, true), (0xA2, false), (0x43, true)]);
        assert_eq!(events.last(), Some(&Grabbed::Combo(Err(RecordError::NeedsModifier))));
    }

    #[test]
    fn every_mapped_vk_parses_as_a_hotkey() {
        for vk in (0x08..=0xDE).filter(|vk| vk_name(*vk).is_some()) {
            let name = vk_name(vk).unwrap();
            let spec = keys::spec_from_press(true, true, false, false, &name).unwrap();
            assert!(crate::hotkeys::parse(&spec).is_ok(), "vk {vk:#x} -> {spec}");
        }
    }
}
