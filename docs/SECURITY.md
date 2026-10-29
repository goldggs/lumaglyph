# Security

LumaGlyph is a local desktop tool. It converts files on your machine, and it does not
want your data. This page exists so you can check that instead of believing it —
every claim below is backed either by a line you can read in the source, or by a test
that fails if the claim stops being true.

The audit lives in `tests/security_and_provenance_audit.rs`. It reads the source tree
and `Cargo.toml` and asserts the properties described here. Run it yourself:

```bash
cargo test --release --test security_and_provenance_audit -- --nocapture
```

The tail of that run prints the full inventory: every network reference, every
`unsafe` block, every atomically-written file.

---

## The short version

| Question | Answer |
| :--- | :--- |
| Does it send telemetry? | No. There is no analytics, crash reporting, or usage tracking code. Zero. |
| Does it phone home? | Only when *you* ask it to check for updates. The URL is fixed, HTTPS-only, and GitHub-only. |
| Can it read my other files? | It reads the file you open, its own preferences, and its own fonts and icons. Nothing else is scanned. |
| Does it run shell commands? | Two places, both deliberate: `ffmpeg` (video, with an argument vector, never a shell string) and the self-updater (to relaunch the new binary). |
| Is it signed? | Not yet — see [Signing](#code-signing-status). Builds are reproducible from CI and published with SHA-256 sums. |
| What is the dependency count? | 17 runtime crates, plus one Windows-only build dependency. Every one is listed and justified [below](#supply-chain). |

---

## Network

Exactly one module opens a socket: `src/updater.rs`.

- **No sockets anywhere else.** The audit greps every source file for `TcpStream`,
  `UdpSocket`, `TcpListener`, `unix_socket`, and the `ureq` agent constructor, and
  fails if either appears outside `updater.rs`.
- **No telemetry symbols.** `telemetry.rs` is *local* process measurement — it reads
  this process's own memory and CPU, which is what the header stats display. There is
  no collector, no endpoint, no batching, no identifier. The audit asserts the
  absence of `analytics`, `sentry`, `crash_report`, `amplitude`, `datadog`, and
  similar.
- **Offline by default.** The app launches and works with no network interface. The
  update check is a menu item, never a startup task, and never a background timer.

### The updater's rules

The updater is the most dangerous code in any self-updating app, so it is the most
constrained. All of this is asserted by tests in `remote_update_security`:

1. **HTTPS only.** A plaintext `http://` URL is rejected outright.
2. **GitHub hosts only.** The host must be `github.com`, `api.github.com`,
   `objects.githubusercontent.com`, or `*.githubusercontent.com`. There is no
   configurable endpoint, so there is no way to point the updater elsewhere.
3. **No explicit ports.** `github.com:8443` is rejected, because it would let a
   crafted URL reach a listener that is not GitHub.
4. **No userinfo in the URL.** `https://github.com@evil.example/` parses as host
   `evil.example` — the audit asserts userinfo is never accepted.
5. **Redirects are followed only within that host set**, with a bounded count, so a
   redirect cannot walk the client off GitHub.
6. **SHA-256 is verified before extraction.** The digest is checked against the
   `.sha256` asset, and only then is the archive opened.
7. **Archive members are validated.** Entries with absolute paths, `..` components, or
   symlink flags are refused before anything is written. Extraction cannot escape the
   target directory.
8. **No downgrade.** A release older than the running version is refused.
9. **Installing is a separate, explicit step.** Downloading never overwrites the
   running binary by itself; the replace-and-relaunch path verifies the digest again
   before swapping the file.

The one `http://` string in the codebase is inside `EndpointPolicy::loopback_mock`,
which exists so the tests can point at `127.0.0.1`. The audit asserts no production
code path calls it.

---

## Subprocesses

Two modules spawn processes. Both are visible in the UI before they run.

**`src/video.rs` — `ffmpeg`.** Video transcoding is delegated to `ffmpeg` because
reimplementing an H.264 decoder is not the job of an ASCII renderer. The important
details:

- Arguments are passed as a **vector**, never a shell string, so a filename containing
  `; rm -rf ~` is treated as a filename.
- The audit asserts the codebase contains no `sh -c`, `bash -c`, `cmd /C`,
  `powershell`, or `system(` call anywhere.
- `ffmpeg` is discovered on `PATH` or via the path you configured in Settings. If it is
  missing, the app says so and video features are disabled. It never downloads a
  binary on its own.

**`src/updater.rs` — relaunch after update.** After the new binary replaces the old
one, the updater starts it. This is the documented behaviour: a verified self-update
must rebuild and relaunch, not leave you with a dead window.

---

## Filesystem

LumaGlyph writes to exactly these locations:

| Path | Contents |
| :--- | :--- |
| `<config>/lumaglyph/preferences.json` | Your settings. The directory is namespaced, so it cannot collide with another app's `preferences.json`. |
| The output path you choose | Exports (PNG/SVG/HTML/ANS/TXT) and video frames. |
| The install directory | Only during a verified self-update. |

- **Nothing is written next to your source images** unless you ask for that output
  path.
- **Preferences are written atomically** (write to a temporary file, then rename), so
  a crash mid-save cannot leave a corrupt config that prevents startup.
- **No history, no recent-files list, no cache of your images.** Close the app and no
  record of what you opened remains.

---

## Memory safety

The project is `#![forbid(unsafe_code)]`-adjacent in spirit but not in the letter:
there are a small number of `unsafe` blocks, each one necessary, each one annotated.

| Module | Why `unsafe` is needed |
| :--- | :--- |
| `src/telemetry.rs` | Reading this process's resident memory and CPU time requires platform syscalls on Linux, macOS and Windows. |
| `src/main.rs` | Two Win32 calls so the app does not spawn a console window when launched from Explorer, and so it can attach to a parent console when run from a terminal. |
| `src/gui.rs` | Passing GLSL source strings to the GPU backend, which takes a raw pointer. |

The audit counts these and **fails if any `unsafe` block lacks a `SAFETY:` comment
explaining the invariant that makes it sound.** Six blocks were missing one before
this rule existed; they have them now. If a new one appears without a note, CI fails.

Verified by `security_and_provenance_audit`:

- Every `unsafe` block is in the allowlist above — no new module can quietly acquire
  one.
- Every `unsafe` block is preceded by a `SAFETY:` note.
- No raw pointer arithmetic on user data, no `transmute`, no `slice::from_raw_parts`
  on a length that came from a file.

---

## Panic surface

A panic in a GUI app is a crash the user sees. The engine is written to refuse bad
input rather than abort on it:

- Malformed, truncated and zero-sized images are rejected at the boundary.
- Non-finite and out-of-range parameters are clamped before they reach the maths.
  `engine_stress_and_fuzz` sweeps every slider from end to end with `NaN` and `inf`.
- The CLI is fuzzed with empty strings, negative numbers, absurd column counts and
  corrupt files, and must exit non-zero with a message — never with `panicked at` or a
  stack trace. `cli_contract_matrix` asserts this for every subcommand.
- Release builds use `panic = "abort"`, so a bug becomes a clean exit rather than a
  half-unwound state.

---

## Supply chain

**17 runtime dependencies**, plus one build-only crate used to embed the Windows icon.
Short enough to read in an afternoon, and chosen to be widely used rather than novel:

| Crate | Why it is here |
| :--- | :--- |
| `clap` | Command-line parsing. |
| `dirs` | The correct per-OS config directory. |
| `eframe`, `egui` | Native window, GPU surface, immediate-mode widgets. |
| `flate2` | Decompressing release archives. |
| `fontdue` | Rasterising the bundled glyphs into the atlas. |
| `gif` | Animated GIF input and output. |
| `image` | Decoding PNG/JPG/WebP/BMP and writing PNG. |
| `rayon` | Parallel row conversion. This is where the throughput comes from. |
| `rfd` | Native file dialogs. |
| `serde`, `serde_json` | Preferences and the GitHub API response. |
| `sha2` | Verifying update digests. |
| `tar` | Extracting the Linux/macOS release archive. |
| `ureq` | The single HTTPS client, used only by the updater. |
| `zip` | Extracting the Windows release archive. |
| `winresource` | Embedding the icon and version info into the Windows `.exe` (build-only). |

Asserted by the audit:

- The dependency list matches that allowlist **exactly** — adding a crate means
  changing the test, on purpose.
- **No wildcard versions** (`*`). Every dependency is pinned to a minor version.
- `Cargo.lock` is committed, contains well over a hundred locked packages, and more
  than 95% of them carry a `checksum` line, so a tampered crate cannot slip in
  unnoticed.
- Release profile is `strip = true`, `debug = 0`, `panic = "abort"` — no symbols, no
  debug info, nothing to leak your build paths.
- **Builds happen in GitHub Actions**, on runners that never see a developer's home
  directory. (`build-info` is asserted not to contain `$HOME` or `/home/` for the same
  reason.)

---

## Provenance and licensing

- **MIT licensed.** `LICENSE` is present and the crate declares `license = "MIT"`.
- **Bundled fonts carry their licences.** IBM Plex Sans and JetBrains Mono ship with
  their OFL texts next to them in `assets/fonts/`. The audit checks the files exist.
- **Every release asset is published with a `.sha256`** sum, and the updater verifies
  it before extracting.
- **Reproducible from CI.** The workflow files are in `.github/workflows/`; the same
  commands run locally as on the runner.

---

## Code signing status

Windows and macOS builds are **not yet signed**. Signing requires either a paid
certificate or acceptance into a free programme for open-source projects, and the
application is in progress — see `docs/SIGNING.md` for the exact plan and the current
state.

Until then, you will see the usual operating-system warnings for an unsigned binary.
What you can do instead of trusting the signature:

1. **Download from the GitHub Releases page only**, and check the `.sha256` sum.
2. **Build it yourself** — `cargo build --release --locked` on your own machine, from
   the tag you want. There is nothing to trust but the source.
3. **Read this page's claims against the tests.** Every paragraph above maps to a test
   you can run in under a minute.

---

## Reporting something

If you find a problem, open an issue at
<https://github.com/goldggs/lumaglyph/issues> with the output of
`lumaglyph build-info` (it contains the version, target triple and toolchain, and
nothing about you). For anything you would rather not post publicly, say so in the
issue title and a private channel will be arranged.

There is no bug bounty. There is a genuine thank-you in the release notes.
