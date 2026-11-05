#!/bin/sh
# fjo installer — builds the rust binary and puts it on PATH.
# Intended to be run by the fjo agent skill when `fjo --version` fails.
set -eu

if ! command -v cargo >/dev/null 2>&1; then
    echo "error: cargo not found; install rust via https://rustup.rs first" >&2
    exit 1
fi

# plugin root = three levels above: scripts → fjo → skills → root
plugin_root="$(cd "$(dirname "$0")/../../.." && pwd)"

echo "==> building fjo (release profile)"
(cd "$plugin_root" && cargo build --release)

dest="${CARGO_HOME:-$HOME/.cargo}/bin"
prefix="${DESTDIR:-}"
if [ -n "$prefix" ]; then
    dest="$prefix/usr/local/bin"
fi

mkdir -p "$dest"
cp "$plugin_root/target/release/fjo" "$dest/fjo"
chmod +x "$dest/fjo"

echo "==> installed to $dest/fjo"
"$dest/fjo" --version
