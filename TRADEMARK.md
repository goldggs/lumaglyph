# Trademark policy

LumaGlyph is open source under the MIT licence. That licence gives you the code. It
does **not** give you the name or the logo. This page explains where the line is, and
it is deliberately permissive: almost everything people want to do with the name is
fine.

The short rule: **if you changed the program, give it a different name.**

---

## What this policy covers

The following are the project's marks:

| Mark | Where it lives |
| :--- | :--- |
| The name **LumaGlyph** | The project, its releases, its documentation |
| The **LG monogram** | `assets/brand/lg-mark.png`, `assets/icons/` |
| The wordmark and banner artwork | `assets/brand/banner.png`, `assets/brand/social-preview.png` |
| The application icon | `assets/icons/lumaglyph.ico`, `lumaglyph.icns` |

These are **unregistered** marks. Nothing here depends on a registration; this page
records the project's position so that nobody has to guess what is allowed.

---

## What you may do without asking

- **Use the code for anything.** Fork it, modify it, sell it, ship it inside a
  commercial product, run it on a thousand machines. The MIT licence permits all of
  that, and this policy does not narrow it.
- **Say what it is.** "A fork of LumaGlyph", "based on LumaGlyph", "compatible with
  LumaGlyph look codes", "an alternative to LumaGlyph" are all accurate statements and
  all fine. Comparison, criticism, reviews, tutorials, articles and academic work may
  use the name freely.
- **Keep the name for an unmodified copy.** If you redistribute the official release
  binaries, unchanged, then calling them LumaGlyph is simply true. Keep the `.sha256`
  file alongside them so people can verify that is what they got.
- **Link to the project** with the name or the logo, for example in a list of tools or
  a package manager's catalogue entry that points at the original releases.
- **Package the original source** in a Linux distribution, Homebrew tap, Scoop bucket or
  similar, under its own name, when you are building the unmodified source.

None of the above needs permission, and none of it needs a credit line, though a credit
line is always welcome.

---

## What you may not do

- **Ship a modified build under the LumaGlyph name.** If you patch the code, change the
  defaults, add a feature, or bundle it with something else, it is no longer the program
  this name refers to. Give it a new name.
- **Use the name or logo in your own product's name.** "LumaGlyph Pro", "LumaGlyph
  Cloud", "LumaGlyph Studio Plus" and the like suggest the project makes or endorses
  your thing. It does not.
- **Use the logo as your own.** A fork with the LG monogram as its icon is trading on
  the original's identity, and users cannot tell the two apart.
- **Suggest endorsement, partnership or employment** that does not exist. Do not put the
  name on a paid course, a consultancy, a hosted service or a crowdfunding campaign in a
  way that implies it is official.
- **Register the name, a confusingly similar name, or a domain** such as
  `lumaglyph-app.com`, `getlumaglyph.io`, or a `lumaglyph` account on a platform, to
  represent the project.
- **Remove or alter the notices** in the source, including the licence and this policy.

---

## If you fork it: how to rename

Renaming a fork properly takes about ten minutes.

1. Pick a new name that is not confusingly similar. Something unrelated to "glyph",
   "luma" or ASCII art is safest.
2. Change the product name in `Cargo.toml`, the window titles and the About panel.
3. Replace the icon in `assets/icons/` with your own artwork, and remove the LG monogram
   from any bundle or splash screen.
4. Rewrite `README.md` for your project rather than leaving the original describing a
   different program.
5. Add one line that says what it is based on: "A fork of LumaGlyph
   (https://github.com/goldggs/lumaglyph)." That is the polite thing to do, it is
   accurate, and it needs no permission.

If you would rather keep the name and ship patches upstream, that is better for
everyone. Pull requests are welcome.

---

## Why this policy exists

MIT is a copyright licence. It grants rights to copy, modify and distribute the *code*.
Trademark is a different body of law, and no open source licence grants trademark
rights, because a licence that let anyone use a name would make the name worthless as a
way of telling one build from another.

That matters more for a security-sensitive tool than it sounds. The value of the name is
that it tells a user "this binary came from the release page, built by CI, with the
source you can read." If any modified build could use the name, that signal would carry
no information, and the project's SHA-256 verification and reproducible builds would be
pointing at a name anyone could claim.

So the policy is narrow on purpose: it restricts the branding, not the code. Everything
downstream of the code is yours.

---

## Asking for permission

If you want to use the name or the logo in a way this page does not cover, ask first.
Most reasonable requests are granted in writing, and a written answer protects you as
much as it protects the project.

Open an issue at <https://github.com/goldggs/lumaglyph/issues> titled "Trademark
permission", or contact the maintainer on GitHub at
<https://github.com/goldggs>.

---

## Enforcement

This policy is enforced only against confusing use: a fork passing itself off as the
original, a product borrowing the name, a registered mark or domain that collides. It is
not enforced against criticism, comparison, packaging of unmodified source, or anything
else on the "may do" list above.

Nobody has been asked to stop so far. The intent is for it to stay that way.
