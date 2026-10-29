# Testing

Everything in LumaGlyph is tested in the open, by the same suite that runs on every
commit and every release. This page explains what is covered, what is not, and how
to check any claim on it yourself.

If you only want one number: **run `cargo test --release` and read the summary at
the bottom.** Everything below is why that number is what it is.

---

## How to run the suites

```bash
cargo test --release                  # everything
cargo test --release --lib            # unit tests only, ~2 seconds
cargo test --release --test security_and_provenance_audit   # the malware audit
cargo test --release -- --nocapture   # shows the printed inventories
```

Two things worth knowing:

- **`--release` matters.** Timing-sensitive behaviour (frame budgets, buffer reuse)
  differs in a debug build, and the golden images are rendered with the optimised
  maths. CI always uses `--release`, so use it locally too if you want the same
  results.
- **`--nocapture`** is how you see the audit summary that most suites print. Without
  it those lines are swallowed unless a test fails.

To regenerate the numbers in this page:

```bash
./scripts/test_inventory.sh --write    # rewrites docs/TEST-INVENTORY.md
```

CI runs the same script and fails if the committed inventory is stale, so the table
in the repo cannot drift away from reality.

---

## What the suites are

| Suite | What it protects | Why it exists |
| :--- | :--- | :--- |
| `src/**` unit tests | Colour maths, engine invariants, glyph atlas, exports, look codes, platform helpers, view geometry | Fast, precise failures. These run in about two seconds and point at a single function. |
| `cli_contract_matrix` | The built binary: exit codes, every output format, every colour mode, corrupt input | The CLI is what scripts and batch jobs use. Library tests cannot catch a wrong exit code. |
| `engine_stress_and_fuzz` | Adversarial image sizes, truncated buffers, NaN parameters, random look codes | The engine takes attacker-shaped input. It must refuse, never crash. |
| `golden_render` | Seven reference renders, compared pixel by pixel with a tolerance | Catches visual regressions that unit tests are blind to. |
| `security_and_provenance_audit` | The "this is not malware" claims, verified against the source tree | A claim in a README decays. A test does not. |
| `remote_update_security` | The updater: URL parsing, redirects, archive extraction, SHA-256, downgrade guard | This is the only code that touches the network, and it installs executables. |
| `icon_and_perf_guard` | Runtime icon decoding, and two asymptotic cost checks | A blank icon ships silently, and a lost parallel iterator makes the app four times slower without failing any other test. |
| `themes_prefs_and_binary_audit` | Themes, preferences, release-binary shape, docs wiring | Stops the brand and the packaging from silently rotting. |
| `video_and_cli_e2e` | Streaming video, seeking, frame export, `ffmpeg` pipes | The pipeline people actually run for hours. |
| `combinatorial_fuzz_matrix` | Cross-products of colour modes, charsets and parameters | Cheap breadth: hundreds of combinations per test function. |
| `engine_and_color_matrix` | The conversion maths against known inputs | Ordering, clamping, and no-surprise defaults. |
| `glyph_and_charset_matrix` | All 95 glyphs, every preset, custom sets | The app is a glyph matcher; the glyph data is the product. |
| `look_export_and_cli_matrix` | Look code round trips and every export format | Sharing a look is a headline feature, so it must round-trip exactly. |

`docs/TEST-INVENTORY.md` lists every suite with its exact function and case counts,
generated from the test binaries themselves.

---

## The golden-image suite

Unit tests prove a function returns what it should. They cannot tell you that a
render *looks* wrong. `golden_render` fills that gap: it renders seven fixed scenes
through the real pipeline (convert → rasterise → PNG) and compares the result with a
committed reference image.

The comparison is **tolerance-based, not a hash**. A hash would fail on a one-bit
change caused by a different FPU, a different libm, or a different SIMD path — and
CI runs on x86-64 and arm64. Instead the suite allows a small per-channel drift
(6/255) and a small fraction of differing pixels (2%), which absorbs that noise while
still catching real changes.

Re-bless after an intentional visual change:

```bash
LUMAGLYPH_BLESS=1 cargo test --release --test golden_render
git add tests/golden
```

Look at the diff before you bless. A blessed regression is a silent one.

---

## Performance guards

Speed is a stated goal, and a goal that nothing measures tends to quietly disappear.
`icon_and_perf_guard` measures the conversion cost two ways and asserts that the shape
of the curve has not changed.

The measured behaviour, on a 1280x960 source:

| Columns | Grid | Cells | Per frame | Per cell |
| ---: | :--- | ---: | ---: | ---: |
| 40 | 40x15 | 600 | 0.25 ms | 420 ns |
| 60 | 60x23 | 1,380 | 0.47 ms | 341 ns |
| 80 | 80x30 | 2,400 | 0.77 ms | 322 ns |
| 120 | 120x45 | 5,400 | 1.73 ms | 321 ns |
| 160 | 160x60 | 9,600 | 2.96 ms | 308 ns |
| 240 | 240x90 | 21,600 | 6.52 ms | 302 ns |
| 320 | 320x120 | 38,400 | 11.58 ms | 302 ns |
| 420 | 420x158 | 66,360 | 21.00 ms | 316 ns |

Two things follow from that table, and both are asserted:

1. **Cost per output cell is flat** — roughly 300 ns from 600 cells to 66,360. The
   pipeline is O(cols x rows) with no hidden quadratic term.
2. **Source resolution is close to free** at a fixed column count: 640x480 renders in
   1.70 ms and 2560x1920 in 1.75 ms, a 3% difference for 16x the pixels. That is why
   the same code path handles a phone photo, a 4K frame and a video stream.

Neither test asserts an absolute number of milliseconds. They assert *ratios*, with a
3x allowance, because a shared CI runner is not a benchmark machine. The failure they
are designed to catch is structural — an accidental O(n^2), a lost `rayon` iterator, a
per-cell allocation — not a 15% regression on a busy afternoon.

`lumaglyph bench` prints real throughput if you want the absolute figure on your own
machine.

## Vulnerabilities found by these suites

Two real defects were found by writing this suite, both now fixed and covered by a
regression test. They are listed here because a test suite that has never found
anything is usually a test suite that is not looking.

| ID | What was wrong | Impact | Fix |
| :--- | :--- | :--- | :--- |
| **SEC-2026-001** | `GlyphAtlas::glyph_slice()` computed `glyph_idx * stride` without bounds checking. | An out-of-range glyph byte overflowed the multiply and panicked on the slice range. Not reachable from the current renderer (which clamps first), but it is a public API and the failure was a process abort. | The index is now clamped to the last glyph, and the test asserts saturation for `usize::MAX`. |
| **SEC-2026-002** | Scroll-to-zoom multiplied a possibly non-finite scroll delta into the zoom factor. | One `NaN` delta from a trackpad driver poisoned the canvas transform permanently: every later `Rect` was `NaN` and the picture silently stopped drawing. | Non-finite deltas are ignored, and a corrupted zoom resets to 100% instead of propagating. |

Two smaller correctness issues were also found and fixed:

- `--color <typo>` used to be accepted silently and fall back to the default palette,
  so a typo in a batch script produced a folder of wrong-looking exports with exit
  code 0. It now fails with a message.
- Six `unsafe` blocks were missing `SAFETY` justifications. They have them now, and
  `security_and_provenance_audit` fails if a new one appears without a note.

---

## What is *not* covered

Being explicit about gaps is part of being trustworthy.

- **The GUI is tested by extraction, not by driving it.** The arithmetic behind the
  canvas — zoom, fit, the transport-band split, timeline mapping, cell hit-testing —
  lives in plain functions with full unit coverage. The `egui` widgets themselves are
  not exercised headlessly, so a layout that looks wrong is caught by looking, not by
  CI. Screenshots in `docs/screenshots/` are how we check that.
- **No automated accessibility or colour-contrast audit.** The five themes are
  checked for arithmetic consistency, not for WCAG contrast ratios.
- **Video tests need `ffmpeg`.** Where it is missing, video-dependent tests skip
  rather than fail. CI installs it on all three platforms so the release pipeline
  always exercises the real path.
- **The updater's network paths use a loopback mock server.** There is one live
  smoke test against the real GitHub CDN, but it is `#[ignore]`d because it needs
  network access and a published release.
- **Windows-only code paths run on CI but are not unit-tested.** The two Win32 calls in
  `main.rs` (attach the parent console, suppress the console window) only compile on
  Windows, so they are covered by the Windows CI job building and running the binary,
  not by a test that pins their behaviour.
- **Timing and memory figures are measured on the release binary, not asserted.**
  A benchmark printed in the README will drift; treat those numbers as "measured on
  the machine in `docs/DEVELOPING.md`", not as guarantees.

---

## Conventions for new tests

- **Name the behaviour, not the function.** `zoom_never_escapes_its_limits` beats
  `test_zoom_2`.
- **One assertion subject per test**, so a failure names the broken thing.
- **Feed hostile values.** Zero, negative, `NaN`, `inf`, empty strings, `usize::MAX`,
  absurd lengths. Most of the bugs above were found this way.
- **Prefer loops over copy-paste.** One function sweeping nine colour modes is easier
  to keep current than nine functions.
- **Deterministic randomness only.** Use the xorshift generator in
  `engine_stress_and_fuzz` so a failure reproduces exactly.
