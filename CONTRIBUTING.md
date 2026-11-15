# Contributing

**English** · [Türkçe](CONTRIBUTING.tr.md)

Contributions are welcome. Open an [issue](https://github.com/emrebal98/yaltri/issues) for bugs and ideas.

## Setup

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

## Common commands

```sh
cargo run -p yaltri                      # debug build, logs in the terminal
cargo test --workspace                   # tests
cargo clippy --workspace --all-targets   # lint
cargo build --release -p yaltri          # target/release/yaltri(.exe)
```

Set `YALTRI_LOG=debug` for verbose logs.

## Project layout

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

## Documentation languages

Each README translation lives next to `README.md` as `README.<lang>.md` (for example `README.tr.md`);
this guide works the same way (`CONTRIBUTING.<lang>.md`).
To add one, copy `README.md`, translate it, and add a link to the language line at the top of every README.

## Contributing workflow

- Every change goes on its own branch and lands through a pull request; nothing is pushed to `main` directly.
- Commit messages and PR titles follow [Conventional Commits](https://www.conventionalcommits.org), all lowercase:
  `type(scope): summary`, for example `feat(launcher): filter results by kind` or `fix(tray): follow the system theme`.
  Types: `feat`, `fix`, `refactor`, `perf`, `docs`, `test`, `ci`, `chore`. The scope is the area: `launcher`, `picker`, `settings`, `tray`, `files`, `apps`…
- CI (clippy + tests on Windows and Linux) runs on pull requests; a lint job checks the PR title and commit subjects.
- Turn on the local check once per clone, so a wrong commit message is refused right away:
  ```sh
  git config core.hooksPath .githooks
  ```

## Releasing

Releases are prepared by [release-please](https://github.com/googleapis/release-please) from the PR titles on `main`:

- After a `feat`, `fix` or `perf` PR is merged, a **`chore(main): release x.y.z`** PR opens (or updates) with the
  next version, the new `CHANGELOG.md` entry and the bumped versions in `Cargo.toml` / `Cargo.lock`.
  `feat` bumps the middle number (`0.2.0 → 0.3.0`), `fix` and `perf` the last one (`0.2.0 → 0.2.1`).
  `docs`, `chore`, `ci`, `refactor` and `test` do not make a release on their own.
- Keep merging PRs; the release PR collects them. Nothing is released until you merge it.
- Merging the release PR tags the version, builds the Windows and Linux packages, attaches them to the
  [release](https://github.com/emrebal98/yaltri/releases) and publishes it with the changelog as its notes.

To try the build without releasing: **Actions → Release → Run workflow**.
