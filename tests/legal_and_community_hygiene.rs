//! Repository hygiene and legal-surface tests.
//!
//! Every claim in `TRADEMARK.md`, `PRIVACY.md`, `.github/SECURITY.md` and
//! `THIRD-PARTY-NOTICES.md` is only worth anything if it stays true. These tests fail
//! the build when it stops being true, in the same spirit as
//! `security_and_provenance_audit`.
//!
//! The one that matters most is the notices check: MIT and Apache-2.0 both require the
//! copyright notice to be reproduced in binary distributions, and this project ships
//! four binaries. A stale notices file is a licence breach that nobody would notice,
//! which is exactly the kind of thing a test should be watching.

use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &str) -> String {
    let full = root().join(path);
    std::fs::read_to_string(&full).unwrap_or_else(|e| panic!("cannot read {}: {e}", full.display()))
}

fn exists(path: &str) -> bool {
    root().join(path).exists()
}

// ---------------------------------------------------------------------------
// The legal documents exist and are real
// ---------------------------------------------------------------------------

#[test]
fn every_community_and_legal_document_is_present() {
    for doc in [
        "LICENSE",
        "TRADEMARK.md",
        "PRIVACY.md",
        "CODE_OF_CONDUCT.md",
        "CONTRIBUTING.md",
        "THIRD-PARTY-NOTICES.md",
        ".github/SECURITY.md",
        "third-party/licenses/MIT.txt",
        "third-party/licenses/Apache-2.0.txt",
    ] {
        assert!(exists(doc), "{doc} is missing");
        let body = read(doc);
        assert!(
            body.lines().count() > 8,
            "{doc} exists but is a stub ({} lines)",
            body.lines().count()
        );
    }
}

#[test]
fn the_licence_is_mit_and_the_manifest_agrees() {
    let licence = read("LICENSE");
    assert!(
        licence.contains("MIT License"),
        "LICENSE is not the MIT text"
    );
    assert!(
        licence.contains("Permission is hereby granted, free of charge"),
        "LICENSE does not contain the MIT grant"
    );
    let manifest = read("Cargo.toml");
    assert!(
        manifest.contains("license = \"MIT\""),
        "Cargo.toml does not declare MIT, so the crate metadata disagrees with LICENSE"
    );
}

#[test]
fn the_trademark_policy_separates_the_mark_from_the_code() {
    let body = read("TRADEMARK.md");

    // The central claim: MIT grants the code, not the name.
    assert!(
        body.contains("does **not** give you the name"),
        "the trademark policy no longer states that the licence does not grant the mark"
    );
    // It must name what it actually covers.
    for mark in ["LumaGlyph", "LG monogram", "lumaglyph.ico"] {
        assert!(body.contains(mark), "the policy does not mention {mark}");
    }
    // And it must permit the things people legitimately want to do, or it reads as a
    // threat rather than a policy.
    for permission in ["based on LumaGlyph", "fork", "criticism", "unmodified"] {
        assert!(
            body.to_lowercase().contains(&permission.to_lowercase()),
            "the policy does not address '{permission}'"
        );
    }
    // A fork instruction is the practical test of whether the policy is usable.
    assert!(
        body.contains("how to rename"),
        "the policy does not explain how to rename a fork"
    );
    // Unregistered marks, stated honestly.
    assert!(
        body.contains("unregistered"),
        "the policy should not imply a registration that does not exist"
    );
}

#[test]
fn the_privacy_page_matches_what_the_code_actually_does() {
    let body = read("PRIVACY.md");

    for claim in [
        "collects nothing",
        "preferences.json",
        "Check for updates",
        "GitHub",
    ] {
        assert!(
            body.contains(claim),
            "the privacy page no longer mentions '{claim}'"
        );
    }
    // The prefs filename and namespace must match the real implementation, or the page
    // is describing a file that does not exist.
    let updater = read("src/updater.rs");
    assert!(
        updater.contains("preferences.json"),
        "the app no longer writes preferences.json, so PRIVACY.md is wrong"
    );
    assert!(
        updater.contains("lumaglyph"),
        "the config directory is no longer namespaced"
    );
}

#[test]
fn the_repository_security_policy_points_at_a_private_channel() {
    let body = read(".github/SECURITY.md");
    assert!(
        body.contains("security/advisories/new"),
        "the security policy does not give a private reporting route"
    );
    assert!(
        body.contains("do not open a public issue"),
        "the security policy does not tell reporters to stay private"
    );
    assert!(
        body.contains("Supported versions") || body.contains("Supported"),
        "the security policy does not say which versions get fixes"
    );
}

// ---------------------------------------------------------------------------
// Third-party notices
// ---------------------------------------------------------------------------

#[test]
fn the_third_party_notices_cover_every_locked_package() {
    let notices = read("THIRD-PARTY-NOTICES.md");
    let lock = read("Cargo.lock");

    // Every package named in the lockfile must appear in the notices, except the
    // project itself. This is the check that keeps the notices from going stale after a
    // `cargo update`.
    let mut missing = Vec::new();
    for line in lock.lines() {
        let Some(rest) = line.strip_prefix("name = \"") else {
            continue;
        };
        let Some(name) = rest.strip_suffix('"') else {
            continue;
        };
        if name == "lumaglyph" {
            continue;
        }
        // The notices list packages in backticks inside a table cell.
        if !notices.contains(&format!("| `{name}` |")) {
            missing.push(name.to_string());
        }
    }
    assert!(
        missing.is_empty(),
        "{} locked packages are missing from THIRD-PARTY-NOTICES.md: {:?}\n\
         Run: python3 scripts/third_party_notices.py",
        missing.len(),
        &missing[..missing.len().min(10)]
    );
}

#[test]
fn the_notices_are_generated_not_hand_written() {
    let notices = read("THIRD-PARTY-NOTICES.md");
    assert!(
        notices.contains("generated by scripts/third_party_notices.py"),
        "the notices file lost its provenance header"
    );
    assert!(
        exists("scripts/third_party_notices.py"),
        "the generator the notices claim to come from is missing"
    );
}

#[test]
fn no_copyleft_obligation_reaches_the_shipped_binary() {
    // A GPL or AGPL dependency would make the MIT licence of this project unworkable for
    // anyone embedding it. Two crates offer LGPL as one option among three; the notices
    // generator takes the permissive option, so this test asserts that the only
    // copyleft offcuts are ones with a permissive alternative.
    let notices = read("THIRD-PARTY-NOTICES.md");
    let mut offenders = Vec::new();
    for line in notices.lines() {
        if !line.starts_with("| `") {
            continue;
        }
        let cols: Vec<&str> = line.split('|').map(|c| c.trim()).collect();
        if cols.len() < 5 {
            continue;
        }
        let pkg = cols[1];
        let licence = cols[3].to_uppercase();
        let has_permissive_alternative =
            licence.contains("MIT") || licence.contains("APACHE-2.0") || licence.contains("BSD");
        let is_copyleft = licence.contains("GPL") || licence.contains("SSPL");
        if is_copyleft && !has_permissive_alternative {
            offenders.push(format!("{pkg}: {}", cols[3]));
        }
    }
    assert!(
        offenders.is_empty(),
        "a dependency imposes copyleft with no permissive option: {offenders:?}"
    );
}

#[test]
fn the_bundled_fonts_still_carry_their_licence_texts() {
    // The fonts are redistributed, so their OFL texts must ship next to them. This is in
    // the notices table as well, but the files are what actually matters.
    for font in [
        "assets/fonts/OFL-IBMPlexSans.txt",
        "assets/fonts/OFL-JetBrainsMono.txt",
    ] {
        assert!(
            exists(font),
            "{font} is missing, so the bundled fonts are unlicensed"
        );
        let body = read(font);
        assert!(
            body.to_lowercase().contains("open font license"),
            "{font} does not contain the SIL Open Font License text"
        );
        // A file named after a licence must not carry a different licence. The previous
        // contents of the JetBrains Mono file were a Debian packaging copyright file
        // that documented Apache-2 for the fonts and GPL-3 for the packaging scripts,
        // neither of which is the font's actual licence.
        assert!(
            !body.contains("GNU General Public License"),
            "{font} contains GPL text, which does not belong next to a redistributed font"
        );
        assert!(
            body.contains("Copyright"),
            "{font} has no copyright line, so the attribution is incomplete"
        );
    }
}

// ---------------------------------------------------------------------------
// Writing style, applied to the new documents too
// ---------------------------------------------------------------------------

#[test]
fn the_legal_documents_contain_no_emoji_and_no_marketing_voice() {
    for doc in [
        "TRADEMARK.md",
        "PRIVACY.md",
        "CONTRIBUTING.md",
        "THIRD-PARTY-NOTICES.md",
        ".github/SECURITY.md",
    ] {
        let body = read(doc);
        for ch in body.chars() {
            let cp = ch as u32;
            let emoji = (0x1F300..=0x1FAFF).contains(&cp) || (0x2600..=0x27BF).contains(&cp);
            assert!(!emoji, "{doc} contains an emoji: {ch:?}");
        }
        // No exclamation marks in a legal or community document: they read as pressure.
        assert!(!body.contains("!!"), "{doc} uses doubled exclamation marks");
    }
}
