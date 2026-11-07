#!/usr/bin/env bash
# Builds a self-contained khole-analyze binary (only system libs are linked dynamically).
#
# Prerequisites: brew install eigen libsamplerate pkg-config
# FFT uses Apple's Accelerate framework, so fftw is not needed.
# Usage: ./build.sh [/path/to/essentia-source]   (default: the vendor/essentia submodule)
#
# Output: ../../src-tauri/binaries/khole-analyze-<target-triple> (Tauri sidecar naming).
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ESSENTIA_SRC="${1:-$HERE/../../vendor/essentia}"
if [ ! -f "$ESSENTIA_SRC/waf" ]; then
  echo "Essentia source not found at $ESSENTIA_SRC (run: git submodule update --init vendor/essentia)" >&2
  exit 1
fi
PREFIX="$HERE/.essentia-static"
BREW="$(brew --prefix)"
TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
OUT_DIR="$HERE/../../src-tauri/binaries"

# 1. Static, minimal Essentia (no ffmpeg/taglib/chromaprint/gaia/tensorflow, no Python).
if [ ! -f "$PREFIX/lib/libessentia.a" ]; then
  pushd "$ESSENTIA_SRC" >/dev/null
  python3 waf configure --build-static --lightweight=libsamplerate \
    --fft=ACCELERATE --prefix="$PREFIX"
  python3 waf -j"$(sysctl -n hw.ncpu)"
  python3 waf install
  popd >/dev/null
fi

# 2. Link the CLI fully static against Homebrew's .a files.
mkdir -p "$OUT_DIR"
clang++ -std=c++17 -O2 \
  -I"$PREFIX/include" -I"$BREW/include/eigen3" -I"$BREW/include" \
  "$HERE/main.cpp" \
  "$PREFIX/lib/libessentia.a" \
  "$BREW/opt/libsamplerate/lib/libsamplerate.a" \
  -framework Accelerate \
  -o "$OUT_DIR/khole-analyze-$TRIPLE"

echo "built: $OUT_DIR/khole-analyze-$TRIPLE"
otool -L "$OUT_DIR/khole-analyze-$TRIPLE"
