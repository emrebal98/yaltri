# Yaltri

**English** · [Türkçe](README.tr.md)

*Yaltrı-* (Old Turkic): to shine.

Yaltri is a small helper that waits quietly in your system tray. Press a shortcut, find what
you need, and it gets out of your way.

- **Search:** Apps, files and games in one box. Results appear as you type.
- **Calculate:** `2^10`, `5 km to mi`, `=pi`. Press Enter to copy the result.
- **Pick colors:** Grab a color from anywhere on screen and copy it in the format you want.

Looks the same on Windows and Linux (Ubuntu, Arch). A single small program, no admin rights; it goes online only when you check for updates.

[![Release](https://img.shields.io/github/v/release/emrebal98/yaltri?include_prereleases)](https://github.com/emrebal98/yaltri/releases)
[![CI](https://github.com/emrebal98/yaltri/actions/workflows/ci.yml/badge.svg)](https://github.com/emrebal98/yaltri/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

---

## Installation

### Windows

Download [**yaltri-windows-x86_64.exe**](https://github.com/emrebal98/yaltri/releases/latest/download/yaltri-windows-x86_64.exe) and run it.
That's it: Yaltri installs itself for your user (no admin rights), adds itself to the Start menu and appears in the tray.
To update, download and run the new version the same way.

> On first launch Windows may warn about an unknown publisher: **More info → Run anyway**.

### Linux

```sh
curl -fsSL https://github.com/emrebal98/yaltri/releases/latest/download/install.sh | sh
```

The script puts Yaltri in `~/.local/bin`, starts it, and tells you the exact command if a system library is missing.
Run it again to update.

<details>
<summary>Manual install</summary>

```sh
curl -LO https://github.com/emrebal98/yaltri/releases/latest/download/yaltri-linux-x86_64.tar.gz
tar xzf yaltri-linux-x86_64.tar.gz
install -Dm755 yaltri-linux-x86_64/yaltri ~/.local/bin/yaltri
yaltri
```

</details>

If a library is missing:

```sh
sudo apt install libxkbcommon-x11-0 libvulkan1 mesa-vulkan-drivers   # Ubuntu
sudo pacman -S libxkbcommon-x11 vulkan-icd-loader                    # Arch
```

On GNOME the tray icon needs the AppIndicator extension (preinstalled on Ubuntu).
Yaltri adds itself to the app menu.

Yaltri starts with your system from the first launch on; turn this off under **Settings → General → Start with system**.

---

## Usage

| Shortcut | Action |
|---|---|
| `Super+Shift+F` | Open search |
| `Super+Shift+C` | Open the color picker |

Change shortcuts in the **Settings** window (right-click the tray icon → Settings).

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
  Star (★) a format to copy it directly on the next pick.

### Command line

```sh
yaltri                    # start in the background
yaltri toggle launcher    # open / close search
yaltri toggle picker      # open the color picker
yaltri settings           # open settings
yaltri quit               # quit
yaltri uninstall          # uninstall (Windows)
```

### Shortcuts on Linux

- **GNOME (X11 and Wayland):** Yaltri adds its shortcuts to GNOME's custom shortcuts by itself and keeps
  them in sync with Settings. If GNOME already uses the same keys (for example `Super+Space` switches the
  keyboard layout), Yaltri takes them while it runs and gives them back when you change the shortcut or quit.
- **Other Wayland desktops** do not let apps register global shortcuts. Add one in your desktop settings
  with the command `yaltri toggle launcher` (or `yaltri toggle picker`):
  - **KDE:** System Settings → Shortcuts → Custom Shortcuts
  - **Hyprland:** `bind = SUPER SHIFT, F, exec, yaltri toggle launcher`

---

## Settings and files

|  | Windows | Linux |
|---|---|---|
| Settings | `%APPDATA%\yaltri\config\settings.toml` | `~/.config/yaltri/settings.toml` |
| Index, history, logs | `%LOCALAPPDATA%\yaltri\data` | `~/.local/share/yaltri` |

**To uninstall on Windows:** Settings → Apps → Installed apps → Yaltri → **Uninstall**. It asks whether to delete
your settings and history too.

**To uninstall on Linux:** turn off **Start with system** in Settings, choose **Quit** from the tray menu, then delete
`~/.local/bin/yaltri`, both folders above, `~/.local/share/applications/yaltri.desktop` and
`~/.local/share/icons/hicolor/scalable/apps/yaltri.svg`.

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

### Contributing workflow

- Every change goes on its own branch and lands through a pull request; nothing is pushed to `main` directly.
- Commit messages and PR titles follow [Conventional Commits](https://www.conventionalcommits.org), all lowercase:
  `type(scope): summary`, for example `feat(launcher): filter results by kind` or `fix(tray): follow the system theme`.
  Types: `feat`, `fix`, `refactor`, `perf`, `docs`, `test`, `ci`, `chore`. The scope is the area: `launcher`, `picker`, `settings`, `tray`, `files`, `apps`…
- CI (clippy + tests on Windows and Linux) runs on pull requests; a lint job checks the PR title and commit subjects.
- Turn on the local check once per clone, so a wrong commit message is refused right away:
  ```sh
  git config core.hooksPath .githooks
  ```

### Releasing

Releases are prepared by [release-please](https://github.com/googleapis/release-please) from the PR titles on `main`:

- After a `feat`, `fix` or `perf` PR is merged, a **`chore(main): release x.y.z`** PR opens (or updates) with the
  next version, the new `CHANGELOG.md` entry and the bumped versions in `Cargo.toml` / `Cargo.lock`.
  `feat` bumps the middle number (`0.2.0 → 0.3.0`), `fix` and `perf` the last one (`0.2.0 → 0.2.1`).
  `docs`, `chore`, `ci`, `refactor` and `test` do not make a release on their own.
- Keep merging PRs; the release PR collects them. Nothing is released until you merge it.
- Merging the release PR tags the version, builds the Windows and Linux packages, attaches them to the
  [release](https://github.com/emrebal98/yaltri/releases) and publishes it with the changelog as its notes.

To try the build without releasing: **Actions → Release → Run workflow**.

---

## License

[MIT](LICENSE). Icons from [Lucide](https://lucide.dev) ([ISC](assets/icons/LICENSE)).
