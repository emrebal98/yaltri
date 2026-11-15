#!/bin/sh
# release-please bumps the workspace crates in Cargo.lock by name (see
# release-please-config.json). A crate missing from that list would leave
# Cargo.lock behind and break `--locked` builds, so the two lists must match.
cd "$(dirname "$0")/.." || exit 1
lock=$(sed -n 's/^name = "\(yaltri[^"]*\)"$/\1/p' Cargo.lock | sort)
config=$(grep -o "@.name.value=='[^']*'" release-please-config.json | sed "s/.*=='\(.*\)'/\1/" | sort)
if [ "$lock" != "$config" ]; then
  echo "✗ release-please-config.json does not list the same crates as Cargo.lock"
  echo "  Cargo.lock: $(echo $lock)"
  echo "  config:     $(echo $config)"
  exit 1
fi
