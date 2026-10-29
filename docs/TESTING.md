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
`icon_and_perf_guard` measures the conversion cost and asserts that the shape of the
cost curve has not changed.

The cost of one conversion is **a fixed overhead per call plus a per-cell slope**:

```text
time = fixed + slope * (cols * rows)
```

The slope describes the algorithm. The fixed part is thread-pool dispatch and
allocation, and it varies enormously by machine — roughly 0.1 ms on a workstation and
1.8 ms on a shared Windows runner. So the test does **not** assert the slope, and it does
not assert milliseconds. It asserts the *shape*, which is the only thing that means the
same on every machine:

1. **Time must grow with the grid.** Trivially true today, and it catches an inverted or
   quantised cost model.
2. **A 48-fold increase in cells must cost between 2.5x and 300x more time.** Linear is
   48x, quadratic would be about 2,300x, constant-cost is 1x. The band is wide because
   the fixed overhead compresses the ratio on slow machines and contention inflates it on
   fast ones.
3. **Cost per cell at the largest grid must stay under 3,000 ns.** This is the one that
   catches a lost `rayon` iterator or a per-cell allocation, which would move a single
   measurement by an order of magnitude without changing the ratio much.

The measured figures, collected from all four machines this project builds on:

| Machine | Ratio for 48x cells (ideal 48x) | ns per cell at 420 columns |
| :--- | ---: | ---: |
| Workstation (Linux, 8 cores) | 42x | 318 ns |
| `ubuntu-latest` CI | 16x | 192 ns |
| `windows-latest` CI | 8x | 246 ns |
| `macos-15` CI | 83x | 336 ns |

The spread is the point. Any assertion tight enough to be interesting on the first row
would be red on the others.

The measured behaviour on a 1280x960 source, from a workstation:

| Columns | Grid | Cells | Per frame | Per cell |
| ---: | :--- | ---: | ---: | ---: |
| 60 | 60x23 | 1,380 | 0.55 ms | 396 ns |
| 120 | 120x45 | 5,400 | 1.83 ms | 339 ns |
| 240 | 240x90 | 21,600 | 7.00 ms | 324 ns |
| 420 | 420x158 | 66,360 | 21.09 ms | 318 ns |

And source resolution is close to free at a fixed column count: 640x480 renders in
1.76 ms and 2560x1920 in 1.83 ms, a 4% difference for 16x the pixels. Downsampling reads
a bounded number of samples per output cell, which is why the same code path handles a
phone photo, a 4K frame and a video stream.

`lumaglyph bench` prints real throughput if you want the absolute figure on your own
machine.

### Three attempts, and what each one taught

This test took four releases to get right, and the record is more useful than the final
numbers.

| Attempt | What it asserted | Why it failed |
| :--- | :--- | :--- |
| 1 | A constant cost per output cell | The fixed per-call overhead dominates at small grids, so small grids looked 3-6x more expensive per cell. Red on Linux and Windows. |
| 2 | Least squares over four points, tight curvature bound | One stalled sample on a three-core macOS runner produced 47% apparent curvature. A quadratic term would be out by 2,300x, so the test was measuring the runner, not the code. |
| 3 | Two endpoints, 2x tolerance on intermediate points | On the next run macOS skewed the *largest* grid instead, and the same test that passed a day earlier was 2.2x off. |
| 4 | Monotonic growth, a wide ratio band, and a per-cell ceiling | Stable across four machines and repeated runs. |

Two lessons, both now applied everywhere in this suite:

- **Timing tests must not run in parallel.** `cargo test` runs test functions on threads;
  four separate timing tests were competing with each other and with `rayon`. All timing
  now happens inside a single test function, sequentially.
- **Assert the shape, not the speed.** A shared runner is not a benchmark machine. Ratios
  and ceilings relative to the same run survive a busy afternoon; absolute milliseconds
  do not.

### Two lessons this suite taught

- **Timing tests must not run in parallel.** `cargo test` runs test functions on
  threads, so four separate timing tests were competing with each other and with
  `rayon` for the same cores. The measurements were inflated and unrepeatable. All
  timing now happens inside a single test function, sequentially.
- **Assert ratios, not milliseconds.** A shared runner is not a benchmark machine. Every
  threshold here is relative to another measurement taken in the same run, or a
  structural property of the curve, so a busy afternoon cannot turn CI red.

## Vulnerabilities found by these suites

Two real defects were found by writing this suite, both now fixed and covered by a
regression test. They are listed here because a test suite that has never found
anything is usually a test suite that is not looking.

| ID | What was wrong | Impact | Fix |
| :--- | :--- | :--- | :--- |
| **SEC-2026-001** | `GlyphAtlas::glyph_slice()` computed `glyph_idx * stride` without bounds checking. | An out-of-range glyph byte overflowed the multiply and panicked on the slice range. Not reachable from the current renderer (which clamps first), but it is a public API and the failure was a process abort. | The index is now clamped to the last glyph, and the test asserts saturation for `usize::MAX`. |
| **SEC-2026-002** | Scroll-to-zoom multiplied a possibly non-finite scroll delta into the zoom factor. | One `NaN` delta from a trackpad driver poisoned the canvas transform permanently: every later `Rect` was `NaN` and the picture silently stopped drawing. | Non-finite deltas are ignored, and a corrupted zoom resets to 100% instead of propagating. |

A third class of problem was found in the tests themselves: they passed on Linux and
failed on Windows, twice, for reasons that were entirely about the environment rather
than the code. Both are described in [Performance guards](#performance-guards) and
below, because a test suite whose portability bugs are hidden is a test suite you
cannot trust on three platforms.

Two usability bugs were found by running the CLI as a subprocess rather than as a
library, which is the whole reason that suite exists:

| Symptom | Cause | Fix |
| :--- | :--- | :--- |
| `lumaglyph myphoto.pgn` (typo) opened a window on a file that did not exist; an unknown command did the same | The optional positional file argument swallowed any unrecognised word | The path is checked before the window is created, and a missing file exits with a message |
| A test hung for an hour on a headless macOS runner | Once the above was fixed, a *test harness* bug: the child's output was read only after it exited, so a large ANSI dump filled the pipe buffer and blocked it | Both pipes are drained on their own threads, and every invocation has a 60-second deadline |

Two smaller correctness issues were also found and fixed:

- `--color <typo>` used to be accepted silently and fall back to the default palette,
  so a typo in a batch script produced a folder of wrong-looking exports with exit
  code 0. It now fails with a message.
- Six `unsafe` blocks were missing `SAFETY` justifications. They have them now, and
  `security_and_provenance_audit` fails if a new one appears without a note.

And one test-only bug worth recording: the security audit located the single permitted
`http://` string by reconstructing byte offsets from line lengths. Rust's `str::lines()`
strips a trailing carriage return, so on a CRLF checkout the offsets drifted by one byte
per line and the audit failed on Windows only. It now uses `match_indices`, which
reports true byte positions, and `.gitattributes` forces LF in the working tree on every
platform, so the class of bug cannot return. The whole suite was re-run against a
deliberately CRLF-converted copy of the repository to confirm that.

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
