#!/bin/sh
# Installs (or updates) Yaltri for the current user on Linux:
#   curl -fsSL https://github.com/emrebal98/yaltri/releases/latest/download/install.sh | sh
#
# Puts the binary in ~/.local/bin, tells you if a system library is missing,
# and starts Yaltri. Yaltri then adds itself to the app menu and to startup.
# No root needed.
set -eu

repo="emrebal98/yaltri"
asset="yaltri-x86_64-unknown-linux-gnu"
bin_dir="${XDG_BIN_HOME:-$HOME/.local/bin}"
bin="$bin_dir/yaltri"

say() { printf '%s\n' "$*"; }
fail() { say "yaltri: $*" >&2; exit 1; }

[ "$(uname -s)" = "Linux" ] || fail "this installer is for Linux; on Windows download yaltri.exe from https://github.com/$repo/releases/latest"
case "$(uname -m)" in
  x86_64 | amd64) ;;
  *) fail "no build for $(uname -m) yet (only x86_64)" ;;
esac
command -v curl >/dev/null 2>&1 || fail "curl is required"
command -v tar >/dev/null 2>&1 || fail "tar is required"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

say "Downloading Yaltri…"
curl -fsSL "https://github.com/$repo/releases/latest/download/$asset.tar.gz" -o "$tmp/yaltri.tar.gz" \
  || fail "download failed"
if command -v sha256sum >/dev/null 2>&1 \
  && curl -fsSL "https://github.com/$repo/releases/latest/download/$asset.tar.gz.sha256" -o "$tmp/sum" 2>/dev/null; then
  expected=$(cut -d' ' -f1 "$tmp/sum")
  actual=$(sha256sum "$tmp/yaltri.tar.gz" | cut -d' ' -f1)
  [ "$expected" = "$actual" ] || fail "checksum mismatch, download is damaged"
fi
tar xzf "$tmp/yaltri.tar.gz" -C "$tmp"

# An update replaces the running binary: ask the old one to quit first.
if [ -x "$bin" ]; then
  "$bin" quit >/dev/null 2>&1 || true
  sleep 1
fi
mkdir -p "$bin_dir"
install -m 755 "$tmp/$asset/yaltri" "$bin"
say "Installed $bin"

# Libraries Yaltri loads at run time (not visible to ldd).
missing=""
have_lib() { ldconfig -p 2>/dev/null | grep -q "$1" || [ -e "/usr/lib/$1" ] || [ -e "/usr/lib/x86_64-linux-gnu/$1" ] || [ -e "/usr/lib64/$1" ]; }
have_lib libxkbcommon-x11.so.0 || missing="$missing xkbcommon-x11"
have_lib libvulkan.so.1 || missing="$missing vulkan"
if [ -n "$missing" ]; then
  say ""
  say "Missing system libraries:$missing. Install them with:"
  id=""
  [ -r /etc/os-release ] && id=$(. /etc/os-release && printf '%s %s' "${ID:-}" "${ID_LIKE:-}")
  case "$id" in
    *arch*) say "  sudo pacman -S libxkbcommon-x11 vulkan-icd-loader" ;;
    *fedora* | *rhel*) say "  sudo dnf install libxkbcommon-x11 vulkan-loader mesa-vulkan-drivers" ;;
    *) say "  sudo apt install libxkbcommon-x11-0 libvulkan1 mesa-vulkan-drivers" ;;
  esac
fi

case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *) say "" ; say "Note: $bin_dir is not in your PATH; the app menu entry works anyway." ;;
esac

if [ -z "$missing" ] && { [ -n "${DISPLAY:-}" ] || [ -n "${WAYLAND_DISPLAY:-}" ]; }; then
  nohup "$bin" >/dev/null 2>&1 &
  say ""
  say "Yaltri is running in the tray. Search: Super+Shift+F · Color picker: Super+Shift+C"
fi
