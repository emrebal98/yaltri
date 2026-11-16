# Pin window (always on top) — design

Date: 2026-11-16
Status: approved in brainstorming, pending spec review

## Goal

A third Yaltri tool: one shortcut keeps the focused window above all other
windows; pressing it again releases it. Works on every desktop Yaltri
supports (Windows, GNOME, KDE Plasma, other X11, Hyprland).

No visual marker: no border, no notification, no sound. The user pressed the
shortcut, so they know. The UI and code never mention other products by name
(existing PowerToys shortcut-clash detection stays as is).

## User-facing behaviour

- Tool id `pin`, display name "Üstte Tut" / "Always on Top" (i18n).
  CLI: `yaltri toggle pin` (aliases: `top`, `ontop`).
- Default shortcut **Super+Shift+T**, following the existing pattern
  (Super+Shift+F launcher, Super+Shift+C picker).
- Settings page: enable/disable + shortcut, same as the other tools. No other
  options.
- Pressing the shortcut toggles always-on-top of the window that has focus
  at that moment.
- Windows pinned by Yaltri stay pinned when Yaltri quits.

## Platform strategy

Where the desktop has a built-in "keep above" action, the desktop does the
work and Yaltri only configures the shortcut. Otherwise Yaltri receives the
shortcut and changes the window itself.

| Session | Who receives the key | Who toggles the window |
|---|---|---|
| Windows | Yaltri (existing `RegisterHotKey` / LL hook path) | `GetForegroundWindow` → `SetWindowPos(HWND_TOPMOST / HWND_NOTOPMOST)`, current state from `WS_EX_TOPMOST` |
| GNOME (X11 or Wayland) | Mutter | Yaltri writes the shortcut into `org.gnome.desktop.wm.keybindings toggle-above` |
| X11, non-GNOME | Yaltri (existing X11 grab) | Read `_NET_ACTIVE_WINDOW`, send `_NET_WM_STATE` client message toggling `_NET_WM_STATE_ABOVE` (x11rb) |
| KDE Plasma (Wayland) | Yaltri via GlobalShortcuts portal (existing) | Load a one-line KWin script over D-Bus: `workspace.activeWindow.keepAbove = !workspace.activeWindow.keepAbove` |
| Hyprland | Yaltri via GlobalShortcuts portal (existing) | `hyprctl dispatch pin active` (Hyprland's pin = floating + all workspaces; closest available) |
| Other Wayland (Sway, …) | user's own bind to `yaltri toggle pin` | Not supported: log a warning, do nothing |

### GNOME details

GNOME is detected by `XDG_CURRENT_DESKTOP` (gnome/unity/ubuntu), in both X11
and Wayland sessions, matching where `gnome_keys.rs` is already used.

`toggle-above` is a Mutter keybinding, empty by default. It acts on native
Wayland windows too, which no outside API can. Yaltri does not get the key
press at all on GNOME, so `yaltri toggle pin` is not registered as a GNOME
custom shortcut for this tool.

- Tool enabled: set `toggle-above` to the shortcut in GNOME accelerator form
  (`<Super><Shift>t`).
- Tool disabled, shortcut changed, or Yaltri quits: restore the value that
  was there before (normally `[]`).
- If another GNOME setting already holds the accelerator, take it out while
  Yaltri runs and give it back later, reusing the bookkeeping in
  `gnome_keys.rs` (what we took is written to a file, restored after a crash
  on the next start).

### Edge cases

- **Windows, elevated target:** a non-elevated process cannot change an
  admin window (UIPI). `SetWindowPos` fails; log it, do nothing else.
- **Nothing sensible focused:** desktop (`Progman`/`WorkerW`), taskbar
  (`Shell_TrayWnd`), or one of Yaltri's own windows → ignore. On X11 ignore
  the root window and `_NET_WM_WINDOW_TYPE_DESKTOP`/`_DOCK`.
- **KWin script:** load, run, unload each press, named `yaltri-pin`;
  unload a stale one with that name first. D-Bus failure → log.
- **`hyprctl` missing / fails:** log.

## Code layout

- `yaltri-core`
  - `tool.rs`: `ToolId::Pin`, `as_str` = `"pin"`, display name, aliases.
    `ALL` grows to 3.
  - `settings.rs`: `PinSettings { enabled: true, hotkey: "Super+Shift+T" }`,
    wired into `is_enabled` / `hotkey` / setters / `hotkey_conflicts`.
- `yaltri-platform`
  - new `pin.rs`: `pub fn toggle_focused() -> Result<()>`, one
    `cfg`/session branch per row of the table above (Windows, X11 EWMH,
    KWin, Hyprland). No GPUI.
  - `gnome_keys.rs`: handle the pin tool by writing `toggle-above` instead
    of a custom shortcut.
- `yaltri` (app)
  - `daemon.rs`: `Command::Toggle(ToolId::Pin)` → `pin::toggle_focused()`
    on a background thread (no window opens).
  - settings view + i18n strings for the new tool row.
  - tray menu: **no entry** for pin. Opening the tray menu moves focus to
    the taskbar, so a menu item would have no window to act on. The tray
    item list built from `ToolId::ALL` skips `Pin`.
  - `is_implemented` includes `Pin`.

## Testing

- Unit: `ToolId` round trip and aliases, default settings, hotkey conflict
  detection with three tools, GNOME accelerator conversion for
  `toggle-above`, focused-window filter (desktop/taskbar/own window).
- Manual, Windows: open Notepad, press Super+Shift+T, click another window →
  Notepad stays above; press again → it no longer does. Admin window → no
  crash, log line.
- Manual, Ubuntu (user): same check on a native Wayland app (e.g. Files) and
  an XWayland app; disable tool → `toggle-above` back to `[]`.
- KDE / Hyprland: type-checked here, tested when available.

## Out of scope

Border/highlight, notifications, sounds, list of pinned windows, opacity
control, per-app rules.
