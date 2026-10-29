# Quick start

This page assumes LumaGlyph is already installed. If it is not, see
**[How to Download & Install](INSTALL.md)** first.

---

## 1. Open the app

You will see a plain, empty window. That is correct.

| Action | Windows / Linux | macOS |
| :--- | :--- | :--- |
| Open an image | `Ctrl + O` | `Cmd + O` |
| Open a video | `Ctrl + Shift + O` | `Cmd + Shift + O` |
| Show / hide the grading inspector | `Ctrl + B` | `Cmd + B` |
| Open Settings | `Ctrl + ,` | `Cmd + ,` |
| Reset everything back to normal | `Ctrl + Shift + R` | `Cmd + Shift + R` |
| Open this documentation | `F1` | `F1` |

You can also **drag and drop** an image, a video, or a `.ttf` font file anywhere onto the window.

---

## 2. Watch your image turn into ASCII art

As soon as the file loads, LumaGlyph splits the picture into a grid and picks the character whose
shape best matches each patch. It is not a ten-character brightness ramp: every printable character
from `!` to `~` is treated as a shape, so curves and diagonals get characters that point the right
way.

Things to try right away:

- **Move the mouse over the canvas.** A readout in the bottom corner shows the exact character,
  its color, and the background at that cell.
- **Scroll** to zoom. **Drag** to pan once you are zoomed in. Press `0` to zoom back to fit.
- **Drag the Columns slider** in the inspector to change how many characters wide the art is.
- **Press `N`** for Neon Wireframe - edges only, no fills.
- **Press `B`** for true OLED `#000000` black.

---

## 3. Adjust the look (or reset it)

The **Inspector** panel on the left holds the grading controls: brightness, contrast, saturation,
gamma, edge weight, dithering, character set, color mode, and the effects toggles.

If you push things too far, press **Ctrl + Shift + R** (or click **Reset to Default** in Settings)
to put every ASCII parameter back at once.

---

## 4. Save your art

| What you want | How |
| :--- | :--- |
| PNG image of the ASCII art | `Ctrl + S` |
| Plain text of the characters | `File > Export Text...` |
| Colorful text for a terminal / Discord | `File > Export ANSI...` |
| SVG vector version | `File > Export SVG...` |
| Self-contained HTML page | `File > Export HTML...` |
| Copy the current "look recipe" | `Ctrl + Shift + C` |

The **Look Code** you copy with `Ctrl + Shift + C` is a short `lg2:` line that stores every grading
setting. Paste it into the Look Code box in the inspector to reproduce the exact look later, or
share it with someone else.

---

## 5. Play a video

Open a video file. The video plays directly on the canvas as live ASCII.

The playback controls sit in a **docked strip just beneath the video**:

- Play / pause button on the left, or press **Space**.
- Two small arrows to jump back / forward 5 seconds.
- A timeline bar you can drag to scrub anywhere in the clip.
- **Export Frame** on the right - save either the ASCII frame you are looking at, or the original
  full-resolution video frame as a PNG.

---

## 6. Make it yours

Open **Settings** (`Ctrl + ,`) and look at the tabs:

- **Appearance** - theme, accent color, interface density, corner rounding.
- **Engine & Defaults** - what a new file starts with: columns, charset, color mode, glyph size.
- **Rendering & Quality** - frame rate cap, VSync, anti-aliasing, texture filtering, supersampling.
- **Optimisations** - idle when unfocused, worker threads, preview decode resolution, audio
  analysis.
- **Export & Video** - PNG scale, video quality, custom ffmpeg location.
- **Shortcuts** - the full key map.
- **About & Build** - version, build info, update check, documentation links.

---

## Where to go next

- **[User Guide](USER-GUIDE.md)** - every control explained in detail.
- **[Troubleshooting & FAQ](TROUBLESHOOTING.md)** - if something looks wrong.
