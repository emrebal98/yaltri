//! Windows: global shortcuts through a low-level keyboard hook, used for
//! combinations `RegisterHotKey` refuses because Windows itself or another
//! program owns them (e.g. `Win+Shift+C`, reserved by the shell). This is how
//! PowerToys can use such shortcuts.
//!
//! The hook runs on its own thread with its own message loop, so a busy UI
//! thread can never delay keyboard input system-wide.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::thread;

use anyhow::{Context, Result};
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, KBDLLHOOKSTRUCT, MSG, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
    WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};
use yaltri_core::settings::normalize_hotkey;
use yaltri_core::{Command, ToolId};

use crate::CommandSender;

/// A key plus the exact set of modifiers that must be held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Combo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub vk: u32,
}

/// `Win+Shift+C` -> Combo. `None` for keys we cannot map to a virtual key.
pub fn combo_from_spec(spec: &str) -> Option<Combo> {
    let normalized = normalize_hotkey(spec);
    let mut parts: Vec<&str> = normalized.split('+').collect();
    let key = parts.pop()?;
    let mut combo = Combo { ctrl: false, alt: false, shift: false, win: false, vk: vk_for(key)? };
    for m in parts {
        match m {
            "ctrl" => combo.ctrl = true,
            "alt" => combo.alt = true,
            "shift" => combo.shift = true,
            "super" => combo.win = true,
            _ => return None,
        }
    }
    Some(combo)
}

/// Lowercase key name (as stored in specs) to a Windows virtual-key code.
fn vk_for(key: &str) -> Option<u32> {
    let vk = match key {
        "backspace" => 0x08,
        "tab" => 0x09,
        "enter" => 0x0D,
        "pause" => 0x13,
        "escape" => 0x1B,
        "space" => 0x20,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        "end" => 0x23,
        "home" => 0x24,
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        "printscreen" => 0x2C,
        "insert" => 0x2D,
        "delete" => 0x2E,
        ";" => 0xBA,
        "=" => 0xBB,
        "," => 0xBC,
        "-" => 0xBD,
        "." => 0xBE,
        "/" => 0xBF,
        "`" => 0xC0,
        "[" => 0xDB,
        "\\" => 0xDC,
        "]" => 0xDD,
        "'" => 0xDE,
        _ => {
            let mut chars = key.chars();
            return match (chars.next(), chars.next()) {
                (Some(c), None) if c.is_ascii_alphanumeric() => Some(c.to_ascii_uppercase() as u32),
                _ => key.strip_prefix('f')?.parse::<u32>().ok().filter(|n| (1..=24).contains(n)).map(|n| 0x6F + n),
            };
        }
    };
    Some(vk)
}

static BINDINGS: Mutex<Vec<(Combo, ToolId)>> = Mutex::new(Vec::new());
static SENDER: Mutex<Option<CommandSender>> = Mutex::new(None);
/// Key whose press we swallowed, so its auto-repeat and release are
/// swallowed too. 0 = none.
static CONSUMED: AtomicU32 = AtomicU32::new(0);
/// Marks our own injected key events so the hook ignores them.
const INJECTED_MARK: usize = 0x5941_4C54; // "YALT"
/// An unassigned virtual key, injected so Windows does not treat a Win
/// press as "Win alone" and open the Start menu (PowerToys does the same).
const DUMMY_VK: u16 = 0xFF;

/// The hook thread; dropping it stops the thread and removes the hook.
pub struct HookThread {
    thread_id: u32,
}

impl HookThread {
    pub fn start(tx: CommandSender) -> Result<Self> {
        *SENDER.lock().unwrap() = Some(tx);
        let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, String>>();
        thread::Builder::new().name("yaltri-keyhook".into()).spawn(move || {
            // SAFETY: hook procedure defined below; module handle of our exe.
            let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), GetModuleHandleW(std::ptr::null()), 0) };
            if hook.is_null() {
                let _ = ready_tx.send(Err("SetWindowsHookExW failed".into()));
                return;
            }
            // SAFETY: plain thread id query.
            let _ = ready_tx.send(Ok(unsafe { GetCurrentThreadId() }));
            // The hook is serviced while this thread waits for messages.
            let mut msg: MSG = unsafe { std::mem::zeroed() };
            // SAFETY: standard message loop; ends on WM_QUIT.
            while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {}
            unsafe { UnhookWindowsHookEx(hook) };
        })?;
        let thread_id = ready_rx.recv().context("hook thread died")?.map_err(anyhow::Error::msg)?;
        Ok(Self { thread_id })
    }

    pub fn set_bindings(&self, bindings: Vec<(Combo, ToolId)>) {
        *BINDINGS.lock().unwrap() = bindings;
    }
}

impl Drop for HookThread {
    fn drop(&mut self) {
        BINDINGS.lock().unwrap().clear();
        // SAFETY: asks our own hook thread to leave its message loop.
        unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0) };
    }
}

fn pressed(vk: i32) -> bool {
    // SAFETY: plain key state query.
    unsafe { GetAsyncKeyState(vk) as u16 & 0x8000 != 0 }
}

/// Which binding (if any) a key press triggers, given the held modifiers.
fn matching(bindings: &[(Combo, ToolId)], vk: u32, held: Combo) -> Option<(Combo, ToolId)> {
    bindings
        .iter()
        .find(|(c, _)| c.vk == vk && c.ctrl == held.ctrl && c.alt == held.alt && c.shift == held.shift && c.win == held.win)
        .copied()
}

fn inject_dummy() {
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: DUMMY_VK, wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: INJECTED_MARK },
        },
    };
    let inputs = [key(0), key(KEYEVENTF_KEYUP)];
    // SAFETY: two well-formed keyboard inputs.
    unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32) };
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: lparam is a KBDLLHOOKSTRUCT for WH_KEYBOARD_LL when code >= 0.
    if code >= 0 && let Some(swallow) = handle(wparam as u32, unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) }) {
        if swallow {
            return 1;
        }
    }
    // SAFETY: pass the event on.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// `Some(true)` to swallow the event.
fn handle(message: u32, info: &KBDLLHOOKSTRUCT) -> Option<bool> {
    if info.dwExtraInfo == INJECTED_MARK {
        return None;
    }
    let vk = info.vkCode;
    match message {
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            if CONSUMED.load(Ordering::Relaxed) == vk {
                return Some(true); // auto-repeat of a shortcut key
            }
            let bindings = BINDINGS.try_lock().ok()?;
            let held = Combo {
                ctrl: pressed(0x11),
                alt: pressed(0x12),
                shift: pressed(0x10),
                win: pressed(0x5B) || pressed(0x5C),
                vk,
            };
            let (combo, tool) = matching(&bindings, vk, held)?;
            drop(bindings);
            if combo.win {
                inject_dummy();
            }
            CONSUMED.store(vk, Ordering::Relaxed);
            if let Ok(sender) = SENDER.try_lock()
                && let Some(tx) = sender.as_ref()
            {
                let _ = tx.unbounded_send(Command::Toggle(tool));
            }
            Some(true)
        }
        WM_KEYUP | WM_SYSKEYUP if CONSUMED.load(Ordering::Relaxed) == vk => {
            CONSUMED.store(0, Ordering::Relaxed);
            Some(true)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_specs_into_combos() {
        assert_eq!(
            combo_from_spec("Win+Shift+C"),
            Some(Combo { ctrl: false, alt: false, shift: true, win: true, vk: 0x43 })
        );
        assert_eq!(
            combo_from_spec("Ctrl+Alt+Space"),
            Some(Combo { ctrl: true, alt: true, shift: false, win: false, vk: 0x20 })
        );
        assert_eq!(combo_from_spec("Ctrl+F12").map(|c| c.vk), Some(0x7B));
        assert_eq!(combo_from_spec("Ctrl+PageUp").map(|c| c.vk), Some(0x21));
        assert_eq!(combo_from_spec("Ctrl+ş"), None);
    }

    #[test]
    fn matches_only_the_exact_modifier_set() {
        let combo = combo_from_spec("Win+Shift+C").unwrap();
        let bindings = [(combo, ToolId::Picker)];
        let held = |ctrl, shift, win| Combo { ctrl, alt: false, shift, win, vk: 0x43 };
        assert_eq!(matching(&bindings, 0x43, held(false, true, true)).map(|b| b.1), Some(ToolId::Picker));
        assert!(matching(&bindings, 0x43, held(false, false, true)).is_none(), "Win+C is not Win+Shift+C");
        assert!(matching(&bindings, 0x43, held(true, true, true)).is_none(), "extra Ctrl");
        assert!(matching(&bindings, 0x44, held(false, true, true)).is_none(), "other key");
    }

    #[test]
    fn every_recordable_spec_maps_to_a_vk() {
        let keys = ["space", "enter", "tab", "backspace", "delete", "insert", "home", "end", "pageup", "pagedown",
            "up", "down", "left", "right", "escape", "printscreen", "pause", "`", "\\", "[", "]", ",", "=", "-", ".",
            "'", ";", "/"];
        let mut all: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
        all.extend(('a'..='z').chain('0'..='9').map(String::from));
        all.extend((1..=24).map(|n| format!("f{n}")));
        for key in all {
            let spec = crate::keys::spec_from_press(true, false, false, true, &key).unwrap();
            assert!(combo_from_spec(&spec).is_some(), "{key} -> {spec}");
        }
    }
}
