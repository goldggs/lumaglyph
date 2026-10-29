# Building LumaGlyph From Source

For developers. If you only want to *use* LumaGlyph, you do not need any of this - see
**[INSTALL.md](INSTALL.md)** instead.

---

## Requirements

- Rust stable 1.75 or newer (`rustup` recommended)
- Linux only: windowing development headers (`libxkbcommon-dev`, `libwayland-dev`,
  `libxcb-render0-dev`, `libxcb-shape0-dev`, `libxcb-xfixes0-dev`, `libxcursor-dev`, `libxi-dev`,
  `libxinerama-dev`, `libxrandr-dev`, `libgl1-mesa-dev`, `libegl1-mesa-dev`)
- Optional for video: `ffmpeg` / `ffprobe` on `PATH` (or pointed at from Settings at runtime)
- Windows: MSVC toolchain plus the Windows SDK (`rc.exe` is used to embed the `.ico` and version
  resources)

## Build

```bash
git clone https://github.com/goldggs/lumaglyph.git
cd lumaglyph

cargo build --release --locked
# binary: target/release/lumaglyph (or lumaglyph.exe on Windows)
```

## Run

```bash
cargo run --release                 # open the empty workstation
cargo run --release -- studio --settings rendering
cargo run --release -- image photo.jpg --cols 180 --out art.png
rustup run stable cargo run --release -- build-info
```

## Test

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --release
```

The suite is large and deliberately adversarial: character-shape calibration across atlas heights,
every color mode, every theme x accent x corner combination for WCAG contrast, `lg2:` Look Code
fuzzing, exporters, streaming ffmpeg end-to-end transcoding, and an eight-layer remote-update
security harness (URL parsing, redirect allowlists, checksum confusion, archive traversal, PE/ELF/
Mach-O header validation, atomic swap and rollback).

One test is network-bound and ignored by default:

```bash
cargo test --release --test remote_update_security -- --ignored
```

## Project layout

| Path | Contents |
| :--- | :--- |
| `src/engine.rs` | 8D shape-vector + structure-tensor ASCII conversion, rasterization |
| `src/charset.rs` | Character set presets and glyph ordering |
| `src/color.rs` | Color modes, tri-tone palettes, OLED background handling |
| `src/glyph_atlas.rs` | Font calibration into per-glyph coverage and orientation vectors |
| `src/video.rs` | Streaming ffmpeg decode, seeking, audio envelope, frame extraction |
| `src/export.rs` | PNG / TXT / ANSI / SVG / HTML exporters |
| `src/gui.rs` | eframe/egui workstation: canvas, inspector, settings, video transport |
| `src/theme.rs` | Studio themes, accents, corner geometry, embedded fonts |
| `src/updater.rs` | Preferences model and the verified release checker/installer |
| `src/cli.rs` | clap CLI: image, video, probe, bench, look, build-info, update |
| `build.rs` | Git SHA, toolchain metadata, Windows PE icon + version resources |
| `assets/icons/` | Generated `.ico`, `.icns`, and PNG icon set |
| `assets/linux/`, `assets/macos/` | `.desktop` entry and `Info.plist` bundle template |
| `tests/` | Unit, matrix, fuzz, and end-to-end suites |

## Icons

`assets/icons/lumaglyph.ico` (7 resolutions, 32-bit) is compiled into `lumaglyph.exe` by `build.rs`
via `winresource`, and `lumaglyph-256.png` is embedded at compile time for the runtime window icon
(`src/icon.rs`). `lumaglyph.icns` and the `.desktop` file are packaged into the macOS and Linux
archives by the release workflow. Regenerating them only requires replacing the PNGs in
`assets/icons/`.

## Release pipeline

Pushing a `vX.Y.Z` tag triggers `.github/workflows/release.yml`, which builds:

| Platform | Artifacts |
| :--- | :--- |
| `ubuntu-22.04` | `lumaglyph-linux-x86_64.tar.gz`, `lumaglyph-linux-x86_64-desktop.tar.gz` |
| `macos-15` | `lumaglyph-macos-{arm64,x86_64}.tar.gz`, `LumaGlyph-macos-{arm64,x86_64}-app.zip` |
| `windows-latest` | `lumaglyph-windows-x86_64.zip` |

Each job remaps build paths (`--remap-path-prefix`), asserts the binary contains no runner paths,
verifies the Windows PE subsystem is `IMAGE_SUBSYSTEM_WINDOWS_GUI` (2) so no console window opens,
verifies the embedded `.rsrc` icon table, and writes deterministic archives (fixed mtime, uid/gid 0,
empty user/group names). `SHA256SUMS.txt` is consolidated across all platforms before publishing.

## Benchmarks

```bash
cargo run --release -- bench --frames 240
```

Reported reference numbers on a mid-range desktop: 528 FPS (1.894 ms/frame) at 180x51 cells,
7.3 MB CLI RSS, 32.6 MB idle GUI RSS with a hardware GPU.
