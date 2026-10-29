# Privacy

LumaGlyph collects nothing. There is no analytics, no crash reporting, no account, no
usage tracking, and no server belonging to this project. Your images and videos are
converted on your own machine and never leave it.

That is the whole policy. The rest of this page is the detail behind it, so you can
check rather than take it on trust.

---

## What the app does with your files

- It reads the image or video you open, and nothing else.
- It writes only to the output path you choose in the save dialog.
- Nothing is uploaded. The conversion runs locally on your CPU and GPU.

There is no history, no recent-files list, and no cache of what you opened. Close the
app and no record of your session remains.

---

## What is stored on your computer

One file:

| Platform | Location |
| :--- | :--- |
| Linux | `~/.config/lumaglyph/preferences.json` |
| macOS | `~/Library/Application Support/lumaglyph/preferences.json` |
| Windows | `%APPDATA%\lumaglyph\preferences.json` |

It holds your settings: theme, column count, colour mode, last window size. That is
all. It contains no identifying information, no file paths you have opened, and no
timestamps of your activity.

You can delete it at any time; the app recreates it with defaults on next launch.
There is a "Reset to Default" button in the settings window.

---

## When the app uses the network

Exactly one feature, and only when you trigger it: **Check for updates**, from the Help
menu or `lumaglyph check-update`.

- It sends one HTTPS request to the GitHub Releases API for this repository.
- It sends no identifiers, no machine information, no usage data and no telemetry. It is
  an ordinary web request to a public API.
- As with any web request, GitHub sees the IP address it came from and the usual HTTP
  headers. That is GitHub's processing, covered by [GitHub's privacy
  statement](https://docs.github.com/en/site-policy/privacy-policies/github-privacy-statement),
  not by this project.
- The app works fully offline. It never checks on startup, never checks on a timer, and
  never checks in the background.

Nothing else in the application opens a socket. This is verified by an automated audit
that fails if any other module gains network access; see
[docs/SECURITY.md](docs/SECURITY.md).

---

## The website and the repository

This project has no website and no server. Distribution happens entirely on GitHub:

- **Downloads** come from GitHub Releases.
- **Issues and discussions** are hosted by GitHub, so anything you post there is stored
  by GitHub under their privacy statement. Post only what you are happy to make public.
- **The source** is on GitHub. Reading it involves no data collection by this project.

If you email the maintainer, your address is used to reply to you and for nothing else.
It is not added to a list, because there is no list.

---

## Children

The app is a media conversion tool with no user-generated content, no accounts and no
communication features. It collects no personal data from anyone, of any age.

---

## Your rights

Data protection rules such as the GDPR give you rights to access, correct, export and
delete your personal data. This project holds none, so there is nothing to request. The
one file described above is on your own disk and under your own control.

If you believe the project holds something about you, ask, and the answer will be that
it does not.

---

## Changes

If a future version ever collects anything, this page will say so before that version is
released, and the change will be opt-in. The project's stated design constraint is to
work offline and go online only to check for a new release, so any departure from that
would be a visible, deliberate decision rather than a quiet one.

---

## Contact

Open an issue at <https://github.com/goldggs/lumaglyph/issues>, or contact the maintainer
at <https://github.com/goldggs>.
