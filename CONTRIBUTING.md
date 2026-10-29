# Contributing

Thanks for considering it. This is a small project, so a few conventions make the
difference between a patch that lands and one that sits in the queue.

Before anything else: be decent to people. See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
Security problems go through [the private channel](.github/SECURITY.md), never a public
issue.

---

## The fastest useful contributions

Not everything needs code. In rough order of usefulness:

1. **A reproducible bug report.** Steps, version, platform. The
   [troubleshooting guide](docs/TROUBLESHOOTING.md) covers what to check first.
2. **A screenshot of something that looks wrong.** Layout, spacing, a glyph that renders
   badly. The visual side is the hardest part to test automatically.
3. **A failing test.** A test that reproduces a bug is often more valuable than the fix,
   because it stays fixed afterwards.
4. **A documentation fix.** Anything you had to work out for yourself belongs in the
   docs.

---

## Setting up

You need a Rust toolchain (stable) and, for video features, `ffmpeg` on your `PATH`.

```bash
git clone https://github.com/goldggs/lumaglyph.git
cd lumaglyph
cargo build --release
cargo test --release
```

On Linux you also need the GUI headers:

```bash
sudo apt-get install libgl1-mesa-dev libegl1-mesa-dev
```

`docs/DEVELOPING.md` describes the architecture. The short version: `engine.rs` is the
pixel-to-glyph maths, `gui.rs` is the window, `cli.rs` is the command line, `updater.rs`
is the only module that touches the network, and `video.rs` is the only one that runs a
subprocess.

---

## Before you open a pull request

Four commands. All of them run in CI, so running them locally saves a round trip:

```bash
cargo fmt --all                                  # formatting
cargo clippy --all-targets -- -D warnings         # lints are errors
cargo test --release                              # the full suite
python3 scripts/third_party_notices.py            # only if you changed dependencies
```

If you added or removed a dependency, also regenerate the test inventory:

```bash
./scripts/test_inventory.sh --write
```

CI fails if either generated file is stale, on purpose. A table that drifts away from
the code is worse than no table.

---

## What gets merged

- **Small, focused pull requests.** One change per PR. A PR that reformats three files
  and fixes a bug is hard to review and easy to reject by accident.
- **Tests with behaviour changes.** If you change what the program does, add or update a
  test that would have caught the old behaviour.
- **Plain language.** No marketing copy in the UI or the docs, no exclamation marks, no
  emoji. There is a test that fails on emoji anywhere in the source tree, so this one is
  enforced rather than suggested.
- **No new dependencies without a reason.** The dependency list is deliberately short and
  audited. If a change needs a new crate, say why in the PR description, and expect the
  question "can this be twenty lines instead?"
- **No telemetry.** Not opted-in, not anonymised, not aggregated. The app works offline
  and goes online only to check for a release. This is a design constraint, not a
  preference, and a PR that adds tracking will be closed.

---

## Licence of contributions

By contributing you agree that your contribution is licensed under the project's MIT
licence, and that you have the right to license it.

Please sign off your commits, which certifies that:

```text
Signed-off-by: Your Name <you@example.com>
```

`git commit -s` adds it automatically. This is the
[Developer Certificate of Origin](https://developercertificate.org/), a one-line
statement that you wrote the code or have the right to submit it. There is no CLA and
there will not be one; you keep the copyright to your work.

The project name and logo are covered by [TRADEMARK.md](TRADEMARK.md), which is separate
from the code licence and does not restrict your contribution.

---

## Review, and how long it takes

This is a volunteer project. Realistic expectations:

- A small fix or doc change: usually within a week.
- A feature: longer, and possibly a conversation first about whether it fits.
- No response for two weeks: comment on the PR. Notifications get lost.

If a PR goes quiet it is almost never a judgement on the work. Say something.

---

## Things that will be declined

So nobody wastes an evening on them:

- Adding analytics, crash reporting or a telemetry opt-in.
- Bundling or auto-downloading `ffmpeg`. It is detected and used; it is not shipped.
- Auto-updating without verification. The digest check is not negotiable.
- A second GUI toolkit, or a rewrite in a different style. The interface is a deliberate
  design and consistency matters more than any single improvement.
- Renaming the project, or changing the licence.
- Signing paperwork on behalf of downstream users who embed this code. Their compliance
  is theirs; the maintainer does not certify other products.

---

## Reporting something you would rather not post publicly

Use the [security advisory form](https://github.com/goldggs/lumaglyph/security/advisories/new).
For anything else sensitive, open a bare issue asking for a private channel without the
details. That works.
