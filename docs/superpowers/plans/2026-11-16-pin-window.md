# Pin Window Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a third tool, `pin`, whose shortcut (default Super+Shift+T) toggles always-on-top for the focused window on Windows, GNOME, KDE, X11 and Hyprland.

**Architecture:** `yaltri-core` gets `ToolId::Pin` + `PinSettings`. `yaltri-platform` gets `pin.rs` (`toggle_focused()`, one backend per desktop) and `gnome_keys.rs` learns "builtin" bindings that write a Mutter keybinding (`toggle-above`) instead of a custom shortcut. The app wires the command, settings section and i18n; the tray skips the tool.

**Tech Stack:** Rust 2024, windows-sys (Win32 window APIs), x11rb (EWMH), gsettings / dbus-send / hyprctl subprocesses, GPUI (settings UI).

Spec: `docs/superpowers/specs/2026-11-16-pin-window-design.md`

---

### Task 1: Core — `ToolId::Pin` and `PinSettings`

**Files:** Modify `crates/yaltri-core/src/tool.rs`, `crates/yaltri-core/src/settings.rs`

- [ ] Test first (`tool.rs`): `"top".parse::<ToolId>() == Ok(ToolId::Pin)`, `"ontop"` likewise; round trip already loops over `ALL`.
- [ ] Test first (`settings.rs`): `Settings::default().pin == PinSettings { enabled: true, hotkey: "Super+Shift+T" }`; default settings have no hotkey conflicts.
- [ ] Run `cargo test -p yaltri-core` → fails to compile.
- [ ] Add `ToolId::Pin` (`as_str` = `"pin"`, display `"Üstte Tut"`, aliases `pin|top|ontop|alwaysontop`), `ALL: [ToolId; 3]`.
- [ ] Add `PinSettings { enabled: bool, hotkey: String }` (`#[serde(default)]`), field `pin` on `Settings`, arms in `is_enabled`/`set_enabled`/`hotkey`/`set_hotkey`.
- [ ] `cargo test -p yaltri-core` → pass. Commit `feat(core): add the pin tool id and its settings`.

### Task 2: Platform — `pin::toggle_focused`

**Files:** Create `crates/yaltri-platform/src/pin.rs`; modify `lib.rs`

- [ ] Tests first: `is_shell_class("Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd")` true, `"Notepad"` false; `parse_dbus_int("method return ...\n   int32 7\n") == Some(7)`; `linux_backend(wayland, desktop, hyprland)` picks Gnome for `ubuntu:GNOME` (both sessions), Hyprland when the signature is set, Kwin for Wayland `KDE`, X11 for any X11 non-GNOME, Unsupported for Wayland `sway`.
- [ ] Windows: `GetForegroundWindow`; skip null, shell classes (`GetClassNameW`), own pid (`GetWindowThreadProcessId`); read `GWL_EXSTYLE & WS_EX_TOPMOST`; `SetWindowPos(HWND_NOTOPMOST | HWND_TOPMOST, …, SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE)`; failure → `Err` with `GetLastError` (elevated windows land here).
- [ ] X11: `_NET_ACTIVE_WINDOW` on root; skip 0, `_NET_WM_WINDOW_TYPE_DESKTOP/_DOCK`, own `_NET_WM_PID`; send `_NET_WM_STATE` client message `[2 (toggle), _NET_WM_STATE_ABOVE, 0, 1, 0]` to root with SUBSTRUCTURE_REDIRECT|NOTIFY.
- [ ] KWin: write `yaltri-pin.js` (`const w = workspace.activeWindow || workspace.activeClient; if (w) w.keepAbove = !w.keepAbove;`) to temp dir; `dbus-send` unload stale `yaltri-pin`, `loadScript` → id, run `/Scripting/Script<id>` (Plasma 6) falling back to `/<id>` (Plasma 5), unload.
- [ ] Hyprland: `hyprctl dispatch pin active`, non-`ok` output → `Err`.
- [ ] GNOME: return `Ok` with a debug log (Mutter handles the key). Unsupported: `Err("…not supported on this desktop")`.
- [ ] `cargo test -p yaltri-platform pin` → pass; `cargo check --target x86_64-unknown-linux-gnu -p yaltri-platform` if the target is installed. Commit `feat(platform): toggle always-on-top for the focused window`.

### Task 3: GNOME — builtin keybindings in `gnome_keys`

**Files:** Modify `crates/yaltri-platform/src/gnome_keys.rs`

- [ ] Tests first: state file in old format (`[{"schema":…}]`) still loads; new `State { taken, builtins }` round trips.
- [ ] `Binding.command: String` → `Binding.action: Action` with `Action::Command(String)` and `Action::Builtin { schema: &'static str, key: &'static str }`; `Action::KEEP_ABOVE` = `org.gnome.desktop.wm.keybindings` / `toggle-above`.
- [ ] Persist `builtins: Vec<SetBuiltin { schema, key, previous: Vec<String> }>` next to `taken`.
- [ ] `apply`: take (skipping keys of wanted and already-set builtins) → bind command bindings → `set_builtins` (release unwanted to `previous`, remember `previous` on first set, write `[accel]`) → give back → save.
- [ ] `cargo test` (linux-only module: rely on CI / cross check). Commit `feat(gnome): bind the pin shortcut to gnome's own keep-above action`.

### Task 4: App wiring

**Files:** Modify `crates/yaltri/src/daemon.rs`, `settings_view.rs`, `locales/en.toml`, `locales/tr.toml`

- [ ] `is_implemented` includes `Pin`.
- [ ] `handle`: `Toggle(Pin)` when enabled → `pin::toggle_focused()` on the background executor; log errors.
- [ ] Tray: tools without a tray icon (`Pin`) are skipped.
- [ ] GNOME: `apply_gnome_shortcuts` maps `Pin` → `Action::KEEP_ABOVE`, others → `Action::Command`.
- [ ] Settings: `tool_name(Pin)`, `pin_section` (switch + shortcut row), registered in `starts`.
- [ ] Locales: `tool.pin`, `settings.pin.description` in en/tr.
- [ ] `cargo build -p yaltri && cargo test --workspace` → pass. Commit `feat(pin): add the always-on-top tool to the app`.

### Task 5: Docs + manual verification

- [ ] README.md / README.tr.md tools section: one bullet each.
- [ ] Manual Windows test: release build, Notepad pinned/unpinned with Super+Shift+T, settings section renders.
- [ ] Push branch, open PR (Conventional Commit title `feat: keep any window on top with one shortcut`).
