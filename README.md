<p align="center">
  <img src="assets/brand/banner.png" alt="LumaGlyph" width="100%" />
</p>

<p align="center">
  <a href="https://github.com/goldggs/lumaglyph/releases/latest"><b>Download</b></a>
  &nbsp;|&nbsp;
  <a href="docs/INSTALL.md">Install</a>
  &nbsp;|&nbsp;
  <a href="docs/QUICKSTART.md">Quick start</a>
  &nbsp;|&nbsp;
  <a href="docs/USER-GUIDE.md">User guide</a>
  &nbsp;|&nbsp;
  <a href="docs/TROUBLESHOOTING.md">Troubleshooting</a>
</p>

---

## Download and run it

You do not need a GitHub account, a terminal, or any developer tools for this.

**1.** Open **<https://github.com/goldggs/lumaglyph/releases/latest>**

**2.** Scroll down to the **Assets** heading and click the file for your computer:

| Your computer | Click this file |
| :--- | :--- |
| **Windows 10 / 11** | [`lumaglyph-windows-x86_64.zip`](https://github.com/goldggs/lumaglyph/releases/latest/download/lumaglyph-windows-x86_64.zip) |
| **Mac, Apple Silicon** (M1 / M2 / M3 / M4) | [`LumaGlyph-macos-arm64-app.zip`](https://github.com/goldggs/lumaglyph/releases/latest/download/LumaGlyph-macos-arm64-app.zip) |
| **Mac, Intel** | [`LumaGlyph-macos-x86_64-app.zip`](https://github.com/goldggs/lumaglyph/releases/latest/download/LumaGlyph-macos-x86_64-app.zip) |
| **Linux** | [`lumaglyph-linux-x86_64.tar.gz`](https://github.com/goldggs/lumaglyph/releases/latest/download/lumaglyph-linux-x86_64.tar.gz) |

**3.** Unzip it and open the app.

- **Windows:** extract the zip, then double-click `lumaglyph.exe`. Windows may say "Windows
  protected your PC" because the app is not signed yet; click **More info > Run anyway**. This is
  being fixed (see [code signing](#code-signing)).
- **macOS:** double-click the zip to get `LumaGlyph.app`, drag it to Applications, then
  **right-click > Open** the first time.
- **Linux:** extract, then `chmod +x lumaglyph && ./lumaglyph`.

The app opens to an empty window. Press `Ctrl + O` (`Cmd + O` on macOS), or drag an image onto it.

If any of that was unclear, [docs/INSTALL.md](docs/INSTALL.md) walks through it with screenshots of
where to click.

![LumaGlyph empty startup window](docs/screenshots/v036-fresh-window.png)

---

## What it does

LumaGlyph reads an image or video, splits it into a grid, and picks the ASCII character whose actual
shape best matches each patch. That is different from a brightness ramp: `!` through `~` are treated
as 95 different shapes, so a diagonal edge gets a diagonal character and a curve gets a curved one.
The result stays recognisable at small sizes, which is where ladders usually fall apart.

- Images and video, including long or 4K video, decoded through ffmpeg pipes in roughly 8 MB of RAM.
- Colour, not just density: TrueColor, dual-colour, tri-tone ramps, CRT amber, P1 green, ANSI-16,
  xterm-256, monochrome.
- Transparent and wireframe exports to PNG, SVG, HTML, GIF, VP9 `.webm` with alpha, and RGBA `.mov`.
- Audio-reactive grading: loud transients push edge lock and luminance.
- Offline by default. No account, no telemetry, no socket opened at startup.
- One binary per OS. No installer, no runtime, no Electron.

Built on the shape-vector engine described in [docs/DEVELOPING.md](docs/DEVELOPING.md#the-engine).

---

## Command line

Prebuilt binaries are on [GitHub Releases](https://github.com/goldggs/lumaglyph/releases).

```bash
lumaglyph                                   # empty workstation
lumaglyph photo.png                         # open an image
lumaglyph studio clip.mp4 --theme tungsten
lumaglyph studio --settings optimisations    # jump to a settings tab
```

Headless image conversion:

```bash
# OLED black, shadow gate, bloom
lumaglyph image input.jpg -o oled_art.png --cols 172 --oled-black --black-gate 0.06 --bloom

# transparent wireframe overlay with a tri-tone ramp
lumaglyph image input.jpg -o overlay.png --cols 168 --transparent --wireframe --tritone tokyo

# replay a shared lg2: look code
lumaglyph image input.jpg -o styled.png --look "lg2:cols=180,cs=genuine95,cm=tritone,oled=1,gate=0.050,bl=1,tri=2A0A52-E11D48-FDE047"
```

Headless video conversion (needs `ffmpeg` and `ffprobe` on `PATH`, or set a folder in
`Settings > Export & Video`):

```bash
lumaglyph video input.mp4 -o reactive_oled.mp4 --cols 164 --oled-black --audio-reactive 1.30 --tritone solar --bloom
lumaglyph video input.mp4 -o alpha_overlay.webm --cols 152 --transparent --wireframe --tritone tokyo
lumaglyph video input.mp4 -o alpha_overlay.mov  --cols 152 --transparent --wireframe
```

Updates and diagnostics:

```bash
lumaglyph build-info
lumaglyph check-update
lumaglyph update            # verify, install in place, relaunch
lumaglyph bench --cols 180 --iters 200
```

---

## Documentation

| Guide | Read it when |
| :--- | :--- |
| [1. Download & Install](docs/INSTALL.md) | You need to know which file to pick and how to open it. |
| [2. Quick Start](docs/QUICKSTART.md) | You want to open a file and save your first result. |
| [3. User Guide](docs/USER-GUIDE.md) | You want a specific panel or shortcut explained. |
| [4. Troubleshooting](docs/TROUBLESHOOTING.md) | Something will not open, stutters, or looks wrong. |
| [5. Building From Source](docs/DEVELOPING.md) | You want to compile it or work on the code. |

The same guides are in the app: press **F1**, or use the **Help** menu.

---

## Changelog

### v0.3.9

- Zooming out no longer turns the canvas into a lattice. Minification now samples mipmaps, so cells
  average together instead of dropping out; at minimum zoom 80% of the canvas was landing on or near
  black, now it is 17%, and the picture holds together.
- One divider everywhere. The status bar and canvas readout were drawing typed `|` characters while
  the header drew a hairline; all three now share the same drawn separator.

### v0.3.8

- Seek buttons were squeezed into tall, narrow triangles with a stretched hover box. They are wider
  now, with square hit boxes matching the play button.
- Header readouts are four fixed columns. Before, the whole line was one label, so a change from
  `156x44` to `1920x1080` dragged everything to its left along with it.
- Rewrote the copy across the app and these docs. Settings no longer explains itself in
  parentheses, and the changelog below is shorter.
- New banner.

![LumaGlyph transport strip and header readouts](docs/screenshots/v038-transport-stats.png)

### v0.3.7

- Transport strip is docked under the video instead of floating over the bottom rows.
- Seek icons redrawn as a centred pair; the old pair overlapped and looked smudged.
- Header readouts are fixed-width columns, so a number getting longer no longer shifts the others.
- Quieter wording throughout the settings window and About tab.

### v0.3.6

- On-video transport with timeline scrub, jump 5 s, and **Export Frame** (ASCII frame, source frame,
  or ANSI text).
- `Settings > Optimisations`: stop drawing and decoding when the window is in the background, release
  buffers after 10 s idle, worker-thread count, skip identical video frames, preview decode height,
  audio envelope toggle.
- `Settings > Rendering & Quality`: frame-rate cap, VSync, MSAA, texture filtering, 2x supersampling,
  smooth zoom, measured FPS.
- Six written guides, linked from the Help menu and F1.

### v0.3.5

- Native icons on all three platforms: `.ico` compiled into the PE resources, `.icns` in the macOS
  bundle, `.png` plus `.desktop` on Linux.

### v0.3.4

- Empty startup window, no sample files.
- One-click reset (`Ctrl + Shift + R`).
- Update & relaunch: verify the release, install in place, restart.

### v0.3.1

- Windows builds as a GUI subsystem binary, so no console window opens behind the app.
- Eight-layer verified updater: pinned repository, strict URL parsing, redirect inspection,
  downgrade guard, platform asset binding, SHA-256 before decompression, anti-traversal and
  anti-bomb extraction, native header check before the swap.

### Earlier

`v0.1.0` through `v0.3.0` covered the engine, colour modes, exports, look codes, themes, and the
grading workstation itself. Full history is in
[Releases](https://github.com/goldggs/lumaglyph/releases).

---

## Code signing

Windows builds are unsigned for now, so SmartScreen shows "Unknown publisher". Signing is planned
through the [SignPath Foundation](https://signpath.org) free program for open source. macOS builds
are unsigned too, which is why the first launch needs a right-click. Both are tracked in
[docs/INSTALL.md](docs/INSTALL.md#why-does-windows-warn-me).

---

## Building from source

```bash
git clone https://github.com/goldggs/lumaglyph.git
cd lumaglyph
cargo build --release --locked
```

Rust 1.80 or newer. Linux also needs the X11 and Wayland development headers listed in
[docs/DEVELOPING.md](docs/DEVELOPING.md).

---

## License

MIT. Bundled [IBM Plex Sans](assets/fonts/OFL-IBMPlexSans.txt) and
[JetBrains Mono](assets/fonts/OFL-JetBrainsMono.txt) are under the SIL Open Font License 1.1.
