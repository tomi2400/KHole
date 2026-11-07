# KHole

A fast, keyboard-driven triage tool for DJs on macOS: open a folder of new promos, listen to
each track for a few seconds, and send it to a destination folder, keep it, or trash it with
a single key.

> Status: early development (milestone M1 – project skeleton). Not usable yet.

## Principles

- **Nothing is ever lost.** Deleting means moving to the macOS Trash, name conflicts get a
  `name (2).ext` suffix instead of overwriting, and every action can be undone.
- **Only the folders you pick are touched.**
- BPM and key come from file tags, or from a bundled [Essentia](https://essentia.upf.edu/)
  based analyzer when tags are missing.

## Tech stack

- [Tauri 2](https://tauri.app/) with a Rust backend (file operations, SQLite, waveform peaks)
- React + TypeScript + [shadcn/ui](https://ui.shadcn.com/) + Tailwind CSS
- `khole-analyze`: a small C++ sidecar on top of Essentia for BPM/key detection
  (`sidecar/khole-analyze/`). It reads mono f32le PCM on stdin and prints JSON.

## Building

Requirements: macOS on Apple Silicon, Xcode command line tools, Rust (stable), Node 24,
Homebrew.

```sh
git clone --recurse-submodules https://github.com/tomi2400/KHole.git
cd KHole
# (existing clone: git submodule update --init vendor/essentia)

brew install eigen libsamplerate pkgconf
sidecar/khole-analyze/build.sh   # builds static Essentia once, then the sidecar

npm install
npm run tauri dev                # or: npm run tauri build
```

The Essentia source lives in the `vendor/essentia` git submodule. Its own nested
submodules (test audio and models) are not needed.

## License

KHole is free software under the [GNU General Public License v3.0 or later](LICENSE).

The analyzer links [Essentia](https://github.com/MTG/essentia), which is licensed under the
AGPL-3.0. Essentia's pre-trained models are not used.
