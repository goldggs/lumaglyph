# Security policy

## Reporting a vulnerability

**Please do not open a public issue for a security problem.**

Report it privately through GitHub, which keeps the thread visible only to you and the
maintainer:

1. Go to <https://github.com/goldggs/lumaglyph/security/advisories/new>
2. Describe the issue, and include the output of `lumaglyph build-info` (it reports the
   version, target triple and toolchain, and contains nothing about you).

If you cannot use the advisory form, open a normal issue that says only that you have a
security report and how to reach you, without the details. A private channel will follow.

Include, as far as you can:

- the version and platform,
- what the attacker controls (a file, a filename, an update response, a config file),
- what happens as a result,
- the smallest input that reproduces it.

A rough report is far better than none. There is no form to fill in and no penalty for
being unsure whether something counts.

## What to expect

| Stage | Target |
| :--- | :--- |
| Acknowledging your report | 3 days |
| Initial assessment (is it a vulnerability, how severe) | 7 days |
| Fix for a confirmed issue | 30 days for high severity, best effort otherwise |
| Credit in the release notes | Always, unless you ask not to be named |

This is a volunteer project, so those are targets rather than contractual promises. If a
timeline slips, you will be told rather than left waiting.

There is no bug bounty. There is a genuine thank-you in the release notes.

## Supported versions

Fixes go into the latest release. Older versions are not patched, because the app
self-updates in place and there is no reason to run an old one.

| Version | Supported |
| :--- | :--- |
| Latest release | Yes |
| Anything older | No - update first |

## Scope

In scope:

- the application and its dependencies as shipped in a release archive,
- the update mechanism (signature and digest verification, redirect handling, archive
  extraction),
- anything that lets a malicious file, filename, look code or config file execute code,
  read files it should not, or crash the process,
- any way the app could send data anywhere other than the documented update check.

Out of scope:

- vulnerabilities in `ffmpeg`. It is a separate program, invoked with an argument vector
  and never through a shell. Report those to the ffmpeg project.
- anything that requires an attacker to already have local code execution on your machine.
- missing hardening headers on a GitHub page this project does not control.
- the absence of code signing. That is known, documented in
  [docs/SIGNING.md](../docs/SIGNING.md), and being worked on.

## What the project already guarantees

These are not aspirations; each is enforced by a test that fails the build if it stops
being true. `docs/SECURITY.md` explains them and tells you how to run the audit
yourself.

- No telemetry, analytics or crash reporting of any kind.
- One module opens a socket, and only when you ask it to check for updates.
- Updates are HTTPS-only, GitHub-only, and SHA-256 verified before extraction.
- No shell: `ffmpeg` receives an argument vector, never a command string.
- Every `unsafe` block is documented and confined to three modules.

## Safe harbour

If you research this project in good faith, within the scope above, and give the
maintainer a reasonable chance to fix an issue before publishing, the project will not
pursue any complaint about your research. Do not access other people's data, do not run
denial-of-service tests against GitHub's infrastructure, and do not test against
installations you do not own.
