<p align="center"><img src="design/brand/app-icon.svg" width="112" height="112" alt="Yaltri icon"></p>

<h1 align="center">Yaltri</h1>

<p align="center">
  <b>A small toolbox that waits in your system tray.</b><br>
  Press a shortcut, use the tool, and it gets out of your way.
</p>

<p align="center">
  <a href="https://github.com/emrebal98/yaltri/releases"><img src="https://img.shields.io/github/v/release/emrebal98/yaltri?include_prereleases" alt="Release"></a>
  <a href="https://github.com/emrebal98/yaltri/actions/workflows/ci.yml"><img src="https://github.com/emrebal98/yaltri/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0-blue.svg" alt="License: GPL-3.0"></a>
</p>

<p align="center"><b>English</b> · <a href="README.tr.md">Türkçe</a></p>

Looks the same on Windows and Linux (Ubuntu, Arch). A single small program, no admin rights; it goes online only when you check for updates.
*Yaltrı-* is Old Turkic for "to shine".

---

## Tools

### 🔎 Search · `Super+Shift+F`

Apps, files and games in one box, with a calculator built in: `2^10`, `5 km to mi`, `=pi`.
Results appear as you type; `Enter` opens the result or copies the answer.

<p align="center"><img src="docs/media/launcher.gif" width="680" alt="The Yaltri search box: 2^10 gives 1024, and /a note lists Notepad, Sticky Notes and OneNote"></p>

### 🎨 Color picker · `Super+Shift+C`

Grab a color from anywhere on screen, zoom in with the mouse wheel to hit the exact pixel,
and copy it as HEX, RGB, HSL, OKLCH or five other formats.

<p align="center"><img src="docs/media/color-picker.gif" width="720" alt="Picking colors: the code and name follow the cursor, the mouse wheel opens a magnifier on a tile corner, and a click opens the editor with nine formats and the history"></p>

### 🔋 Battery

Your wireless mouse and earbuds' charge next to the clock. Click it to see each earbud.
Off until you switch it on in **Settings**.

Each tool can be switched off in **Settings**.

---

## Installation

### Windows

Download [**yaltri-windows-x86_64.exe**](https://github.com/emrebal98/yaltri/releases/latest/download/yaltri-windows-x86_64.exe) and run it.
That's it: Yaltri installs itself for your user (no admin rights), adds itself to the Start menu and appears in the tray.
Updates install themselves: see [Updates](#updates).

> On first launch Windows may warn about an unknown publisher: **More info → Run anyway**.

### Linux

```sh
curl -fsSL https://github.com/emrebal98/yaltri/releases/latest/download/install.sh | sh
```

The script puts Yaltri in `~/.local/bin` and starts it; Yaltri adds itself to the app menu and appears in the tray.
If a system library is missing, the script tells you the exact command.
Updates install themselves: see [Updates](#updates).

On GNOME the tray icon needs the AppIndicator extension (preinstalled on Ubuntu).

---

## Usage

| Shortcut | Action |
|---|---|
| `Super+Shift+F` | Open search |
| `Super+Shift+C` | Open the color picker |

Change shortcuts in the **Settings** window (right-click the tray icon → Settings).
Yaltri starts with your system from the first launch on; turn this off under **Settings → General → Start with system**.

### Search

| Key | |
|---|---|
| `↑` `↓` | Move through results |
| `Enter` | Open |
| `Ctrl+Enter` | Open the containing folder |
| `Tab` / `Shift+Tab` | Switch filter: All, Apps, Files, Games |
| `Backspace` in an empty box | Clear the filter |
| `Esc` | Close |

To filter while typing, start with a shorthand and a space: `/a ` apps, `/f ` files, `/g ` games
(`/u`, `/d`, `/o` work too). With a filter on and nothing typed, Yaltri lists everything in it,
for example all your games.

- **Apps:** Start menu and store apps on Windows, installed desktop apps on Linux.
- **Files:** Yaltri keeps its own index of file names and finds them instantly, even across millions of files.
  The first scan can take a few minutes; after that it follows changes on its own.
  On Linux, a home folder with very many folders is rescanned every hour instead of followed live,
  so Yaltri does not use up the system's file-watch limit that other programs need.
- **Games:** Steam, Xbox / PC Game Pass, Epic Games, Riot Games, GOG, Ubisoft Connect, EA app;
  Steam, Heroic and Lutris on Linux. Games start through their own launcher.
- **Math:** Arithmetic, unit conversion and constants. Things you open often move to the top.

### Color picker

- The color's code and name follow the cursor.
- **Mouse wheel** opens the magnifier and zooms in and out.
- **Left click** picks the color and copies it. **Right click** or `Esc` cancels.
- The editor that opens after a pick has your color history, lighter and darker shades and 9 formats:
  HEX, RGB, HSL, HSV, OKLCH, CMYK, HWB, Decimal, Float.
  The format a pick copies is set in **Settings → Color picker**.

### Battery

- Each device that is on gets its own tray icon with its level, refreshed every minute. The number turns
  green while charging and red at 20% or below. Clicking an icon shows every device; earbuds show each
  bud, or "in the case".
- Bluetooth mice, keyboards and headsets: the level Windows shows in its Bluetooth settings, or UPower's on
  Linux (which also covers Logitech receivers). A paired device that isn't connected is left out.
- Logitech mice, keyboards and headsets on a Unifying, Lightspeed or Bolt receiver, or plugged in directly
  (HID++; on Linux through UPower).
- Devices on their own USB dongle: ATK and Pulsar mice, SteelSeries Arctis GameBuds.
- Windows 11 puts new tray icons under the ^ arrow; drag them next to the clock to keep them in view.
- Linux needs a udev rule to read the dongles; **Settings › Battery** copies the command that adds it.

### Command line

```sh
yaltri                    # start in the background
yaltri --foreground       # stay in the terminal, with logs
yaltri toggle launcher    # open / close search
yaltri toggle picker      # open the color picker
yaltri toggle battery     # show the battery levels
yaltri settings           # open settings
yaltri quit               # quit
yaltri update             # install a downloaded update (or check for one)
yaltri uninstall          # uninstall (--purge: settings and history too)
```

### Shortcuts on Linux

- **GNOME (X11 and Wayland):** Yaltri adds its shortcuts to GNOME's custom shortcuts by itself and keeps
  them in sync with Settings. If GNOME already uses the same keys (for example `Super+Space` switches the
  keyboard layout), Yaltri takes them while it runs and gives them back when you change the shortcut or quit.
- **KDE Plasma, Hyprland and other Wayland desktops** with the global shortcuts portal: Yaltri registers its
  shortcuts with the desktop. The first time, the desktop asks you to approve them and may let you pick other
  keys; after that, change them in the desktop's own shortcut settings.
- **Sway and other desktops without that portal:** bind the command in your config, for example
  `bindsym $mod+Shift+f exec yaltri toggle launcher` (and `yaltri toggle picker`).

---

## Updates

Yaltri checks for a new version once a day and downloads it in the background. When it is ready,
**Restart to update** appears in the tray menu and under **Settings → About**; click it and Yaltri updates
and starts again in a few seconds. Turn the daily check off under **Settings → General**; the button in
Settings → About still checks on demand. On Linux this works for the copy `install.sh` installed; a copy
from a package manager is updated by it.

## Settings and files

|  | Windows | Linux |
|---|---|---|
| Settings | `%APPDATA%\yaltri\config\settings.toml` | `~/.config/yaltri/settings.toml` |
| Index, history, logs | `%LOCALAPPDATA%\yaltri\data` | `~/.local/share/yaltri` |

**To uninstall:** run `yaltri uninstall` (on Windows also Settings → Apps → Installed apps → Yaltri → **Uninstall**).
It removes everything Yaltri set up and asks whether to delete your settings and history too; `--purge` deletes them without asking.

---

## Development

Contributions are welcome: open an [issue](https://github.com/emrebal98/yaltri/issues) for bugs and ideas.
Setup, common commands, project layout, commit conventions and releases are in [CONTRIBUTING.md](CONTRIBUTING.md).

---

## License

Copyright (C) 2026 Yaltri contributors.

Yaltri is free software: you can use, study, share and change it under the terms of the
[GNU General Public License v3.0 or later](LICENSE). If you distribute a modified version, or a program
built from Yaltri's code, you must share its source under the same license.

Icons from [Lucide](https://lucide.dev) ([ISC](assets/icons/LICENSE)).
