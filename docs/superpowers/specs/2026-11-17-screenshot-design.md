# Screenshot tool — design

Date: 2026-11-17
Status: agreed in chat (user AFK, defaults chosen by Claude, listed below)

Part 1 of a 4-part screen toolkit:

1. **Screenshot** (this spec): freeze, select rectangle / window / full
   screen, copy or save.
2. Markup: drawing tools in the same toolbar (arrow, box, pen, text,
   highlight, blur, numbers).
3. Text extraction (OCR) on a selected area.
4. Video recording: deferred.

## User-facing behaviour

- Tool id `shot`, name "Ekran görüntüsü" / "Screenshot". CLI
  `yaltri toggle shot` (aliases `screenshot`, `capture`).
- Default shortcut **Super+Shift+S** (pattern Super+Shift+letter). On Windows
  this takes the key from the built-in snipping tool while Yaltri runs, the
  same way Super+Shift+C is taken today.
- Tray menu entry (camera icon). Pressing the shortcut again while the
  overlay is open cancels.

### Overlay

1. The desktop is captured once; every display gets a borderless topmost
   window showing its part, dimmed.
2. A mode bar sits at the top centre of each display: **Rectangle**,
   **Window**, **Full screen**, and close. Keys: `R`, `W`, `F`. Rectangle is
   the default every time.
3. Rectangle: drag to select. The size in pixels shows next to the
   selection. Dragging again outside the selection starts a new one.
4. Window: the window under the cursor is outlined; click selects it.
5. Full screen: the display under the cursor is outlined; click selects it.
6. The selection stays undimmed. A toolbar appears under it (above when there
   is no room): **Copy**, **Save**, close.
   - Copy (`Enter`, `Ctrl+C`): PNG to clipboard, close.
   - Save (`Ctrl+S`): PNG to the save folder **and** clipboard, close.
7. `Esc` or right click cancels at any point.

A selection lives on one display (one overlay window per display).

### Files

- Folder: setting `shot.save_dir`; empty means `Pictures/Screenshots`
  (created if missing).
- Name: `Yaltri_YYYY-MM-DD_HH-MM-SS.png` (local time), `_2`, `_3`… on clash.

### Settings section

Switch, shortcut, save folder row (path, **Change…** picks a folder,
**Open** opens it).

## Platforms

| Session | Capture | Overlay | Window mode |
|---|---|---|---|
| Windows | existing `screen::capture` (GDI, physical px) | ours | yes: visible, non-minimised, non-cloaked top-level windows in z-order, bounds from `DWMWA_EXTENDED_FRAME_BOUNDS`, own windows and shell windows skipped |
| X11 | existing `screen::capture` | ours | yes: `_NET_CLIENT_LIST_STACKING`, frame bounds via `_NET_FRAME_EXTENTS` |
| GNOME Wayland | portal `Screenshot`, non-interactive (GNOME asks permission once); the returned file is read and deleted | ours, through XWayland like the launcher | hidden: Wayland does not expose other windows' positions |
| Other Wayland (KDE, Hyprland…) | portal `Screenshot`, interactive: the desktop's own picker | none | desktop's |

On other Wayland desktops the picked image is copied and saved directly
(same file naming).

### Clipboard

GPUI writes images only as a registered "PNG" format on Windows (no
`CF_DIB`, so Paint and others cannot paste) and not at all on X11. Images go
through `arboard` instead (CF_DIBV5 + PNG on Windows, X11 selection with
INCR, Wayland data-control). One `arboard::Clipboard` lives in the daemon so
X11 keeps serving it.

## Code layout

- `yaltri-core`: `ToolId::Shot`, `ShotSettings { enabled, hotkey, save_dir }`;
  pure helpers `shot_file_name(time, existing)`.
- `yaltri-platform`:
  - `windows_list.rs`: `pub fn visible_windows() -> Vec<WinRect>` (Windows,
    X11), topmost first.
  - `screen.rs`: `capture_portal(interactive: bool)` (Linux).
  - `clipboard.rs`: `ImageClipboard` wrapper around `arboard`.
- `yaltri` app:
  - `shot.rs`: overlay windows, modes, selection, toolbar.
  - `shot_output.rs`: PNG encode, save path, clipboard; portal flow.
  - daemon/settings/tray/i18n/README wiring.

Pure logic (selection rect normalisation, toolbar placement, window hit
test, file naming) is unit tested.

## Out of scope

Markup (part 2), OCR (part 3), video (part 4), delayed capture, selection
resize handles, selections across displays, cursor in image.
