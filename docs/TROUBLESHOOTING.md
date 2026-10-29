# Troubleshooting & FAQ

The most common problems, in plain language. Each answer tells you what to do, not what to know.

---

## Downloading and opening

### Where is the download link?

On the release page: **<https://github.com/goldggs/lumaglyph/releases/latest>**. Scroll down until
you see the heading **Assets**. The files you want are listed there. Pick the one for your computer:

| Computer | File |
| :--- | :--- |
| Windows | `lumaglyph-windows-x86_64.zip` |
| Mac (M1/M2/M3/M4) | `LumaGlyph-macos-arm64-app.zip` |
| Mac (Intel) | `LumaGlyph-macos-x86_64-app.zip` |
| Linux | `lumaglyph-linux-x86_64.tar.gz` |

Full walkthrough: **[INSTALL.md](INSTALL.md)**.

### Windows says "Windows protected your PC"

Click **More info**, then **Run anyway**. Windows shows this for any app that did not come from the
Microsoft Store. LumaGlyph is a single portable `.exe`; it does not install itself anywhere.

### The file is a .zip, what now?

Right-click it and choose **Extract All** (Windows) or just double-click it (macOS). Inside is the
app itself. Run the app, not the zip.

### macOS says the app is from an unidentified developer, or "damaged"

**Right-click** the app, choose **Open**, then confirm **Open**. You only do this once. If macOS
still refuses, run this in Terminal once, then open the app normally:

```bash
xattr -dr com.apple.quarantine /Applications/LumaGlyph.app
```

### Mac with Apple Silicon will not open the arm64 build

Make sure you downloaded the file ending in `-arm64-app.zip` for M-series Macs, and `-x86_64-app.zip`
for Intel Macs. Check with **Apple menu > About This Mac**.

### Linux says "Permission denied"

Run `chmod +x lumaglyph` once in the folder where you extracted it, then `./lumaglyph`.

### Linux: "error while loading shared libraries: libxkbcommon.so.0" (or similar)

Your system is missing windowing libraries. On Debian/Ubuntu:

```bash
sudo apt install libxkbcommon0 libwayland-client0 libxcb-render0 libxcb-shape0 libgl1
```

On Fedora: `sudo dnf install libxkbcommon wayland-libs libxcb mesa-libGL`.

---

## Using the app

### The window opens completely empty. Is it broken?

No - that is intentional. LumaGlyph opens like a fresh text editor rather than preloading sample
images. Press `Ctrl + O` (or `Cmd + O`) to open a file, or drag one onto the window.

### I do not see the Inspector or toolbar

Both are hidden until there is something to inspect. Open an image or video, or press `Ctrl + B`.
If you pressed `Tab` earlier, you are in Zen mode - press `Tab` again.

### My ASCII art looks wrong, blown out, or weirdly colored

Press **Ctrl + Shift + R** (Reset to Default). That restores the calibrated, unboosted defaults:
contrast 1.0, saturation 1.0, gamma 1.0, brightness 0.0, all effects off. You can also click
**Reset to Default** at the bottom of the Settings navigation rail.

### Where is the theme footer that used to be on the status bar?

It was removed on purpose. The status strip only shows real information.

### How do I make the art more detailed?

Drag **Columns** up in the Inspector, and/or set **Glyph Size** to 28 px. Higher column counts mean
more characters, which costs a little more CPU.

### The art is too slow with a huge video

Lower the **Live Preview Decode Height Cap** in `Settings > Optimisations` (for example to 640p) and
lower **Columns** a little. Exports stay full quality regardless.

---

## Video

### Video will not open / plays nothing

LumaGlyph uses `ffmpeg` to decode video. Install it:

- Windows: download a build from ffmpeg.org and add its `bin` folder in
  `Settings > Export & Video > Custom ffmpeg folder`.
- macOS: `brew install ffmpeg`
- Linux: `sudo apt install ffmpeg` (Debian/Ubuntu) or `sudo dnf install ffmpeg` (Fedora)

Images never need ffmpeg.

### Playback stutters

1. Lower **Live Preview Decode Height Cap** to 480p or 640p in `Settings > Optimisations`.
2. Turn off **Analyze audio beat envelope** if you do not need audio-reactive grading.
3. Set **ASCII Engine Worker Threads** to `0` (automatic) so all cores are used.
4. Reduce **Columns**.

### No audio reaction / the beat does not drive the art

The envelope is computed when the video opens. If you turned
**Analyze audio beat envelope** off, re-enable it and re-open the video. The video also needs an
audio track; silent clips have no envelope.

### I cannot find the pause button

It is on the video itself now, at the bottom left of the picture, next to the timeline. `Space` also
works. If you do not see it, move the mouse over the canvas.

### Scrubbing jumps instead of playing from that point

Scrubbing seeks the decoder to the exact time you drop the handle, which is what makes it instant on
long files. Playback continues from there - no re-decoding from the start.

### The exported frame is not the frame I see

**ASCII Frame (PNG)** saves the rendered ASCII art. **Source Video Frame (PNG)** saves the original
video frame. If they differ, it is because the ASCII version is rendered at your export scale - see
`Settings > Export & Video`.

---

## Performance and power

### LumaGlyph is using CPU while I am in another window

Open `Settings > Optimisations` and make sure **Go idle when the window loses focus** and
**Pause video decoding while unfocused** are both on (they are on by default). The pane shows the
current state: ACTIVE or IDLE.

### The laptop gets warm while playing video

Lower the decode cap, lower the FPS cap (`Settings > Rendering & Quality`), and limit worker threads.

### It uses a lot of memory with big videos

Preview decoding is capped at 640 px by default and the frame queue holds only a couple of frames,
so memory stays flat. If you raised the decode cap to 1080p, drop it back down. Turning on
**Release scratch buffers after 10 seconds of idle** also helps.

---

## Updates

### How do I update?

`Help > Check GitHub Releases...`, or `Settings > About & Build > Check Releases`. If a newer version
exists, **Update & Relaunch** downloads it, verifies it, replaces the binary, and restarts.

### Will it update itself without asking?

No. Update checks are opt-in and off by default. Nothing leaves your machine unless you click a
release check yourself.

### Is the download safe?

Every release is built by GitHub Actions from this repository's public source. The updater verifies
the release tag, the exact file for your OS and CPU, a constant-time SHA-256 comparison against the
release checksum file, and the executable's own header before installing. If anything does not
match, the install is refused.

---

## Still stuck?

Open an issue at **<https://github.com/goldggs/lumaglyph/issues>** and include the output of
**Settings > About & Build > Copy Build Info**. That gives the maintainer your version, platform,
GPU, and ffmpeg version in one paste.
