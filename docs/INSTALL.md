# How to Download & Install LumaGlyph

LumaGlyph is a **single app file**. There is no installer, no setup wizard, no account, and no
subscription. You download one zip, unzip it, and double-click the app.

You do **not** need to know how to use GitHub, and you do **not** need to type any commands.

---

## Step 1 - Open the download page

Open this link in your browser:

**<https://github.com/goldggs/lumaglyph/releases/latest>**

You will land on a page titled "Releases". This page always shows the newest version.

> If the page looks confusing: that is normal. GitHub is built for programmers. Ignore everything
> except the **Assets** list described in Step 2.

---

## Step 2 - Find the "Assets" list and pick your file

Scroll **down** on that page. Near the bottom you will see a small heading called **Assets** with a
list of files. That is the download area.

Pick the one line that matches your computer:

| Your computer | Download this file |
| :--- | :--- |
| **Windows** (10 or 11, any normal PC) | `lumaglyph-windows-x86_64.zip` |
| **Mac with Apple Silicon** (M1, M2, M3, M4) | `LumaGlyph-macos-arm64-app.zip` |
| **Mac with Intel chip** (older Macs) | `LumaGlyph-macos-x86_64-app.zip` |
| **Linux** (most distributions) | `lumaglyph-linux-x86_64.tar.gz` |

Not sure which Mac you have? Click the Apple menu, then **About This Mac**. If it says "Apple M1"
(or M2/M3/M4) pick `arm64`. If it says "Intel Core", pick `x86_64`.

There is also a `SHA256SUMS.txt` file in that list. You do not need it - it exists so advanced
users can verify downloads have not been tampered with.

---

## Step 3 - Open the app

### Windows

1. Right-click the downloaded `.zip` and choose **Extract All...**, then **Extract**.
2. Open the extracted folder and double-click **`lumaglyph.exe`**.
3. Windows may show a blue box saying **"Windows protected your PC"**. Click **More info**, then
   **Run anyway**. This happens for any app that is not sold through the Microsoft Store; it does
   not mean anything is wrong. Every release is built publicly by GitHub from the source in this
   repository.
4. The workstation window opens. No terminal window appears alongside it - that is intentional.

Tip: right-click `lumaglyph.exe` and choose **Send to > Desktop (create shortcut)** so it behaves
like any other app.

### macOS

1. Double-click the downloaded `.zip` - macOS unzips it into **`LumaGlyph.app`**.
2. Drag **`LumaGlyph.app`** into your **Applications** folder.
3. The first time, **right-click** (or Control-click) the app and choose **Open**, then click
   **Open** in the dialog. This is a one-time confirmation for apps downloaded outside the App
   Store. After that, a normal double-click works.

If macOS says the app "is damaged" because it was downloaded from the internet, that is the same
Gatekeeper check: right-click the app, choose **Open**, and confirm.

### Linux

1. Extract the archive (your file manager can do this, or run `tar -xzf lumaglyph-linux-x86_64.tar.gz`).
2. Inside you will find `lumaglyph`, plus `lumaglyph.desktop` and `lumaglyph.png` if you want a
   menu entry with the icon.
3. Make the binary executable and run it:

```bash
chmod +x lumaglyph
./lumaglyph
```

To add it to your application menu:

```bash
mkdir -p ~/.local/bin ~/.local/share/applications ~/.local/share/icons/hicolor/512x512/apps
cp lumaglyph ~/.local/bin/
cp lumaglyph.png ~/.local/share/icons/hicolor/512x512/apps/lumaglyph.png
cp lumaglyph.desktop ~/.local/share/applications/
```

If your distribution does not ship `ffmpeg` (used for video), install it with your package manager,
for example `sudo apt install ffmpeg` on Debian/Ubuntu or `sudo dnf install ffmpeg` on Fedora.
Images work without it.

---

## Step 4 - Check it opened correctly

You should see a calm, empty dark window with a slim menu bar at the top and a few keyboard hints in
the middle. That is the intended first-run state - LumaGlyph deliberately opens empty, like a fresh
text editor, instead of dumping sample images on you.

Press **Ctrl + O** (Windows/Linux) or **Cmd + O** (macOS) to open an image, or just drag one onto
the window. If you see colored ASCII characters appear, everything works.

---

## Updating to a newer version

Open **Help > Check GitHub Releases...** in the app, or run `lumaglyph update` from a terminal.
LumaGlyph verifies the download cryptographically, replaces the installed binary in place, and
relaunches into the new version automatically.

Nothing is sent to the internet unless you start a check yourself.

---

## Uninstalling

LumaGlyph does not install anything into your system.

- Delete `lumaglyph.exe` (Windows) or `LumaGlyph.app` (macOS) or the `lumaglyph` binary (Linux).
- Optionally delete your preferences file:
  - Windows: `%APPDATA%\lumaglyph\preferences.json`
  - macOS: `~/Library/Application Support/lumaglyph/preferences.json`
  - Linux: `~/.config/lumaglyph/preferences.json`

---

Still stuck? See **[Troubleshooting & FAQ](TROUBLESHOOTING.md)**.
