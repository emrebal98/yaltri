# Yaltri

**English** · [Türkçe](README.tr.md)

*Yaltrı-* (Old Turkic): to shine.

Yaltri is a small helper that waits quietly in your system tray. Press a shortcut, find what
you need, and it gets out of your way.

- **Search:** Apps, files and games in one box. Results appear as you type.
- **Calculate:** `2^10`, `5 km to mi`, `=pi`. Press Enter to copy the result.
- **Pick colors:** Grab a color from anywhere on screen and copy it in the format you want.

Looks the same on Windows and Linux (Ubuntu, Arch). A single file: no installer, no internet connection.

[![Release](https://img.shields.io/github/v/release/emrebal98/yaltri?include_prereleases)](https://github.com/emrebal98/yaltri/releases)
[![CI](https://github.com/emrebal98/yaltri/actions/workflows/ci.yml/badge.svg)](https://github.com/emrebal98/yaltri/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

---

## Installation

### Windows

1. Download [**yaltri-x86_64-pc-windows-msvc.zip**](https://github.com/emrebal98/yaltri/releases/latest/download/yaltri-x86_64-pc-windows-msvc.zip).
2. Extract it to any folder.
3. Run `yaltri.exe`. The tray icon appears.

> On first launch Windows may warn about an unknown publisher: **More info → Run anyway**.

To start with Windows: `Win+R` → `shell:startup` → put a shortcut to `yaltri.exe` in that folder.

### Linux

```sh
curl -LO https://github.com/emrebal98/yaltri/releases/latest/download/yaltri-x86_64-unknown-linux-gnu.tar.gz
tar xzf yaltri-x86_64-unknown-linux-gnu.tar.gz
install -Dm755 yaltri-x86_64-unknown-linux-gnu/yaltri ~/.local/bin/yaltri
yaltri
```

If a library is missing:

```sh
sudo apt install libxkbcommon-x11-0 libvulkan1 mesa-vulkan-drivers   # Ubuntu
sudo pacman -S libxkbcommon-x11 vulkan-icd-loader                    # Arch
```

On GNOME the tray icon needs the AppIndicator extension (preinstalled on Ubuntu).

To start on login:

```sh
mkdir -p ~/.config/autostart
printf '[Desktop Entry]\nType=Application\nName=Yaltri\nExec=%s\n' "$HOME/.local/bin/yaltri" \
  > ~/.config/autostart/yaltri.desktop
```

---

## Usage

| Shortcut | Action |
|---|---|
| `Super+Space` | Open search |
| `Super+Shift+C` | Open the color picker |

Change shortcuts in the **Settings** window (right-click the tray icon → Settings).

### Search

| Key | |
|---|---|
| `↑` `↓` | Move through results |
| `Enter` | Open |
| `Ctrl+Enter` | Open the containing folder |
| `Esc` | Close |

- **Apps:** Start menu and store apps on Windows, installed desktop apps on Linux.
- **Files:** Yaltri keeps its own index of file names and finds them instantly, even across millions of files.
  The first scan can take a few minutes; after that it follows changes on its own.
- **Games:** Steam, Xbox / PC Game Pass, Epic Games, Riot Games, GOG, Ubisoft Connect, EA app;
  Steam, Heroic and Lutris on Linux. Games start through their own launcher.
- **Math:** Arithmetic, unit conversion and constants. Things you open often move to the top.

### Color picker

- The color's code and name follow the cursor.
- **Mouse wheel** opens the magnifier and zooms in and out.
- **Left click** picks the color and copies it. **Right click** or `Esc` cancels.
- The editor that opens after a pick has your color history, lighter and darker shades and 9 formats:
  HEX, RGB, HSL, HSV, OKLCH, CMYK, HWB, Decimal, Float.
  Star (★) a format to copy it directly on the next pick.

### Command line

```sh
yaltri                    # start in the background
yaltri toggle launcher    # open / close search
yaltri toggle picker      # open the color picker
yaltri settings           # open settings
yaltri quit               # quit
```

### Wayland

Wayland does not let apps register their own global shortcuts. Add one in your desktop settings
with the command `yaltri toggle launcher`:

- **GNOME:** Settings → Keyboard → Keyboard Shortcuts → Custom Shortcuts
- **KDE:** System Settings → Shortcuts → Custom Shortcuts
- **Hyprland:** `bind = SUPER, SPACE, exec, yaltri toggle launcher`

---

## Settings and files

|  | Windows | Linux |
|---|---|---|
| Settings | `%APPDATA%\yaltri\config\settings.toml` | `~/.config/yaltri/settings.toml` |
| Index, history, logs | `%LOCALAPPDATA%\yaltri\data` | `~/.local/share/yaltri` |

**To uninstall:** choose **Quit** from the tray menu, then delete the `yaltri` binary and both folders.

---

## Development

Contributions are welcome. Open an [issue](https://github.com/emrebal98/yaltri/issues) for bugs and ideas.

### Setup

1. Install [Rust](https://rustup.rs) (stable).
2. On Linux, install the build dependencies:
   ```sh
   # Ubuntu / Debian
   sudo apt install build-essential pkg-config libxkbcommon-dev libxkbcommon-x11-dev \
     libwayland-dev libx11-dev libx11-xcb-dev libxcb1-dev libfontconfig-dev \
     libfreetype-dev libvulkan-dev libdbus-1-dev
   # Arch
   sudo pacman -S base-devel libxkbcommon-x11 wayland libx11 libxcb fontconfig \
     freetype2 vulkan-icd-loader dbus
   ```
3. Clone and run:
   ```sh
   git clone https://github.com/emrebal98/yaltri
   cd yaltri
   cargo run -p yaltri
   ```

### Common commands

```sh
cargo run -p yaltri                      # debug build, logs in the terminal
cargo test --workspace                   # tests
cargo clippy --workspace --all-targets   # lint
cargo build --release -p yaltri          # target/release/yaltri(.exe)
```

Set `YALTRI_LOG=debug` for verbose logs.

### Project layout

```
crates/
  yaltri/           the app: search window, color picker, editor, settings (GPUI)
  yaltri-core/      settings, commands, search plumbing, history
  yaltri-calc/      calculator
  yaltri-files/     file index and change tracking
  yaltri-apps/      app and game discovery, icons
  yaltri-color/     color conversions, formats, color names
  yaltri-platform/  tray, global shortcuts, screen capture, OS integration
```

The UI is built with [GPUI](https://gpui.rs). GPUI is pre-1.0, so its version is pinned in
`Cargo.toml` (`gpui-kit = "=0.9.1"`); test carefully when upgrading.

### Documentation languages

Each README translation lives next to this file as `README.<lang>.md` (for example `README.tr.md`).
To add one, copy `README.md`, translate it, and add a link to the language line at the top of every README.

### Releasing

1. Bump `[workspace.package] version` in `Cargo.toml` and commit.
2. Push a tag:
   ```sh
   git tag v0.2.0
   git push origin v0.2.0
   ```

GitHub Actions builds the Windows and Linux packages and uploads them to
[Releases](https://github.com/emrebal98/yaltri/releases). Tags like `v0.2.0-beta.1` are published as pre-releases.

---

## License

[MIT](LICENSE)
