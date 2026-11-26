#!/usr/bin/env bash
# Build battwarn.exe for Windows and package the installer.
#
#   ./build.sh
#
# Requirements: rustup, the msvc target, cargo-xwin (with its SDK cache
# populated), and makensis for the setup exe.

set -euo pipefail
cd "$(dirname "$0")"

TARGET=x86_64-pc-windows-msvc

echo "==> engine checks"
rustc -O tools/engine_check.rs -o /tmp/battwarn-engine-check
/tmp/battwarn-engine-check

echo
echo "==> building $TARGET"
cargo xwin build --release --target "$TARGET"

mkdir -p dist
cp "target/$TARGET/release/battwarn.exe" dist/battwarn.exe

echo
echo "==> packaging installer"
if command -v makensis >/dev/null 2>&1; then
    (cd installer && makensis -V2 battwarn.nsi)
else
    echo "    makensis not found, skipping setup.exe"
fi

echo
ls -lh dist/
