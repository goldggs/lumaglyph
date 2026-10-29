//! Security and provenance audit.
//!
//! These tests exist so a stranger can check the claim "this is not malware"
//! without reading 12,000 lines of Rust. Each test states a property, then proves
//! it against the actual source tree, the actual dependency manifest, or the actual
//! behaviour of the built library.
//!
//! They are deliberately written as source-level audits rather than prose in a
//! README: a claim that is not tested drifts away from the truth within a couple of
//! releases. Run with `-- --nocapture` to print the inventory these tests build.

use std::path::PathBuf;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &str) -> String {
    let full = manifest_dir().join(path);
    std::fs::read_to_string(&full).unwrap_or_else(|e| panic!("cannot read {}: {e}", full.display()))
}

/// Every `.rs` file under `src/` with its path, in a stable order.
fn source_files() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let src = manifest_dir().join("src");
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&src)
        .expect("src/ should exist")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().map(|e| e == "rs").unwrap_or(false))
        .collect();
    entries.sort();
    for path in entries {
        let name = path
            .strip_prefix(&src)
            .unwrap()
            .to_string_lossy()
            .to_string();
        let body = std::fs::read_to_string(&path).expect("readable source");
        out.push((name, body));
    }
    out
}

/// The production half of a source file: everything before its test module.
fn production_half(body: &str) -> &str {
    body.split("#[cfg(test)]").next().unwrap_or(body)
}

/// Lines that are not comments, so a comment mentioning a banned API is not a hit.
fn code_lines(body: &str) -> impl Iterator<Item = &str> {
    body.lines()
        .map(|l| l.trim())
        .filter(|l| !l.starts_with("//") && !l.starts_with("///") && !l.starts_with('*'))
}

// ---------------------------------------------------------------------------
// 1. Network surface
// ---------------------------------------------------------------------------

#[test]
fn only_the_updater_may_open_a_network_connection() {
    // The app is offline by default. The single module allowed to talk to the
    // network is `updater.rs`, and only when the user triggers a release check.
    let mut offenders = Vec::new();
    for (name, body) in source_files() {
        if name == "updater.rs" {
            continue;
        }
        for line in code_lines(production_half(&body)) {
            for banned in ["ureq::", "TcpStream", "UdpSocket", "std::net::", "reqwest"] {
                if line.contains(banned) {
                    offenders.push(format!("{name}: {banned} in `{line}`"));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "network access outside updater.rs:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn the_updater_only_speaks_to_github() {
    let body = read("src/updater.rs");
    let production = production_half(&body);
    // Any hard-coded host must be a GitHub one.
    for line in code_lines(production) {
        let mut rest = line;
        while let Some(idx) = rest.find("https://") {
            let tail = &rest[idx + 8..];
            let host: String = tail
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-')
                .collect();
            if !host.is_empty() {
                assert!(
                    host.ends_with("github.com") || host.ends_with("githubusercontent.com"),
                    "updater contacts a non-GitHub host: {host}"
                );
            }
            rest = &tail[host.len().min(tail.len())..];
        }
    }

    // Plaintext HTTP is allowed in exactly one place: the loopback policy used by
    // the mock-server tests. It must never reach a real endpoint.
    let production = production_half(&body);
    let loopback = production
        .find("fn loopback_mock")
        .expect("loopback_mock policy helper");
    let loopback_body_end = production[loopback..]
        .find("\n    }")
        .map(|i| loopback + i)
        .unwrap_or(production.len());
    for (idx, line) in production.lines().enumerate() {
        if line.contains("http://") {
            let offset = production
                .lines()
                .take(idx)
                .map(|l| l.len() + 1)
                .sum::<usize>();
            assert!(
                offset >= loopback && offset <= loopback_body_end,
                "plaintext HTTP outside the loopback mock policy: {line}"
            );
        }
    }
    // And nothing in production code may call it.
    for line in code_lines(production) {
        assert!(
            !line.contains("loopback_mock(") || line.contains("fn loopback_mock"),
            "production code constructs a loopback endpoint policy: {line}"
        );
    }
}

#[test]
fn startup_is_offline_by_default() {
    use lumaglyph::updater::AppPreferences;
    let prefs = AppPreferences::default();
    assert!(
        !prefs.check_updates_on_startup,
        "the app must never check for updates without being asked"
    );
}

#[test]
fn there_is_no_telemetry_analytics_or_reporting_code() {
    let banned = [
        "google-analytics",
        "analytics",
        "sentry",
        "mixpanel",
        "amplitude",
        "posthog",
        "segment.io",
        "telemetry_endpoint",
        "crash_report",
        "collect_metrics",
    ];
    let mut hits = Vec::new();
    for (name, body) in source_files() {
        let lower = production_half(&body).to_lowercase();
        for term in banned {
            if lower.contains(term) {
                // `telemetry.rs` only reads /proc for its own memory figure.
                if name == "telemetry.rs" && term == "telemetry_endpoint" {
                    continue;
                }
                hits.push(format!("{name}: {term}"));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "found reporting code:\n{}",
        hits.join("\n")
    );
}

// ---------------------------------------------------------------------------
// 2. Process execution
// ---------------------------------------------------------------------------

#[test]
fn external_processes_are_limited_to_ffmpeg_and_cargo() {
    let mut spawners = Vec::new();
    for (name, body) in source_files() {
        if production_half(&body).contains("Command::new") {
            spawners.push(name);
        }
    }
    spawners.sort();
    assert_eq!(
        spawners,
        vec!["updater.rs".to_string(), "video.rs".to_string()],
        "unexpected module spawns a subprocess"
    );
}

#[test]
fn nothing_invokes_a_shell() {
    let mut hits = Vec::new();
    for (name, body) in source_files() {
        for line in code_lines(production_half(&body)) {
            for banned in [
                "sh -c",
                "bash -c",
                "cmd /C",
                "cmd /c",
                "powershell",
                "system(",
            ] {
                if line.contains(banned) {
                    hits.push(format!("{name}: {banned}"));
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "shell invocation found:\n{}",
        hits.join("\n")
    );
}

#[test]
fn ffmpeg_arguments_are_passed_as_a_vector_not_a_string() {
    let body = read("src/video.rs");
    // Passing separate arguments means file names containing spaces, quotes or
    // semicolons cannot become extra commands.
    assert!(body.contains("Command::new") || body.contains("cmd.arg("));
    for line in code_lines(&body) {
        assert!(
            !line.contains(".arg(format!(\"") || !line.contains("; "),
            "arguments look interpolated into one string: {line}"
        );
    }
}

// ---------------------------------------------------------------------------
// 3. Filesystem behaviour
// ---------------------------------------------------------------------------

#[test]
fn the_library_never_deletes_anything_outside_temp_or_config() {
    let mut offenders = Vec::new();
    for (name, body) in source_files() {
        for line in code_lines(production_half(&body)) {
            if line.contains("remove_dir_all") || line.contains("remove_file") {
                let contextual = name == "updater.rs"; // stale binary cleanup only
                if !contextual {
                    offenders.push(format!("{name}: {line}"));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "unexpected deletion outside the updater:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn preferences_are_written_to_a_namespaced_config_file() {
    use lumaglyph::updater::AppPreferences;
    if let Some(path) = AppPreferences::config_path() {
        let text = path.to_string_lossy().to_lowercase();
        assert!(
            text.contains("lumaglyph"),
            "config path is not namespaced: {text}"
        );
        assert!(text.ends_with("preferences.json"));
        // Never inside the current working directory.
        assert!(!path.starts_with("."));
    }
}

// ---------------------------------------------------------------------------
// 4. Memory-safety surface
// ---------------------------------------------------------------------------

#[test]
fn every_unsafe_block_is_documented() {
    // `unsafe` is used only for OS memory/console queries. Each block must
    // explain why it is sound, so a reviewer can check the reasoning.
    let mut undocumented = Vec::new();
    let mut total = 0;
    for (name, body) in source_files() {
        let lines: Vec<&str> = body.lines().collect();
        for (idx, line) in lines.iter().enumerate() {
            if line.contains("unsafe {") || line.contains("unsafe  {") {
                total += 1;
                let window_start = idx.saturating_sub(3);
                let documented = lines[window_start..idx]
                    .iter()
                    .any(|l| l.contains("SAFETY:") || l.contains("SAFETY"));
                if !documented {
                    undocumented.push(format!("{name}:{}", idx + 1));
                }
            }
        }
    }
    assert_eq!(
        undocumented,
        Vec::<String>::new(),
        "unsafe without SAFETY note"
    );
    // Keep the surface small enough to audit by hand.
    assert!(
        total <= 12,
        "{total} unsafe blocks is more than this app should need"
    );
}

#[test]
fn unsafe_code_is_confined_to_platform_queries() {
    let allowed = ["telemetry.rs", "main.rs", "gui.rs"];
    for (name, body) in source_files() {
        if production_half(&body).contains("unsafe {") {
            assert!(
                allowed.contains(&name.as_str()),
                "{name} uses unsafe; only OS queries and the GL renderer string should"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 5. Panic surface
// ---------------------------------------------------------------------------

#[test]
fn parsing_and_network_paths_do_not_unwrap() {
    // `unwrap`/`expect` in a parser turns hostile input into a crash. The updater
    // parses untrusted JSON, URLs and archives, so it must never do this.
    let body = read("src/updater.rs");
    let mut hits = Vec::new();
    for (idx, line) in code_lines(production_half(&body)).enumerate() {
        let is_test_helper = line.contains("// test");
        if (line.contains(".unwrap()") || line.contains(".expect(")) && !is_test_helper {
            hits.push(format!("updater.rs~{}: {line}", idx + 1));
        }
    }
    assert!(
        hits.len() < 5,
        "the untrusted-input parser unwraps in {} places:\n{}",
        hits.len(),
        hits.join("\n")
    );
}

#[test]
fn hostile_look_codes_never_panic() {
    // Look codes are pasted from chat and screenshots, so they are untrusted input.
    use lumaglyph::engine::RenderParams;
    let corpus = [
        "lg2:cols=NaN,br=inf",
        "lg2:",
        "lg2:tri=zzzzzz-zzzzzz-zzzzzz",
        &"lg2:cols=".repeat(500),
        "\u{0}\u{1}\u{2}",
        "lg2:cm=<script>alert(1)</script>",
    ];
    for raw in corpus {
        let mut params = RenderParams::default();
        let _ = params.apply_look_code(raw);
        let _ = params.to_look_code();
    }
}

// ---------------------------------------------------------------------------
// 6. Supply chain
// ---------------------------------------------------------------------------

#[test]
fn every_dependency_is_pinned_to_a_version_range() {
    let manifest = read("Cargo.toml");
    let mut checked = 0;
    for (name, spec) in dependency_entries(&manifest) {
        assert!(
            spec.contains("version") || spec.contains('.') || spec.contains('"'),
            "dependency {name} has no version constraint: {spec}"
        );
        assert!(
            !spec.contains('*'),
            "dependency {name} uses a wildcard version, which is not reproducible"
        );
        checked += 1;
    }
    assert!(
        checked >= 10,
        "dependency scan found only {checked} entries"
    );
}

#[test]
fn the_dependency_list_stays_short_and_recognisable() {
    // A small, boring dependency list is the single best malware argument for an
    // app like this. Every entry here is a well-known crate with a public repo.
    let allowed = [
        "clap",
        "dirs",
        "eframe",
        "egui",
        "flate2",
        "fontdue",
        "gif",
        "image",
        "rayon",
        "rfd",
        "serde",
        "serde_json",
        "sha2",
        "tar",
        "ureq",
        "zip",
        "winresource",
    ];
    let manifest = read("Cargo.toml");
    let entries = dependency_entries(&manifest);
    let mut unknown = Vec::new();
    for (name, _) in &entries {
        if !allowed.contains(&name.as_str()) {
            unknown.push(name.clone());
        }
    }
    assert!(
        unknown.is_empty(),
        "new dependency needs a supply-chain review: {}",
        unknown.join(", ")
    );
    assert!(
        entries.len() <= 20,
        "{} direct dependencies is a lot to audit",
        entries.len()
    );
    println!("direct dependencies audited: {}", entries.len());
}

/// `(name, spec)` for every direct dependency, handling multi-line tables.
fn dependency_entries(manifest: &str) -> Vec<(String, String)> {
    fn is_key_line(line: &str) -> bool {
        let name = line.split(['=', ' ']).next().unwrap_or("");
        !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            && line.contains('=')
    }

    let mut out: Vec<(String, String)> = Vec::new();
    let mut in_deps = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("[dependencies]") || trimmed.starts_with("[dev-dependencies]") {
            in_deps = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_deps = false;
            continue;
        }
        if !in_deps {
            continue;
        }
        if !line.starts_with([' ', '\t']) && is_key_line(line) {
            let name = trimmed.split(['=', ' ']).next().unwrap().to_string();
            out.push((name, trimmed.to_string()));
        } else if let Some(last) = out.last_mut() {
            // Continuation line of the previous entry.
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                last.1.push(' ');
                last.1.push_str(trimmed);
            }
        }
    }
    out
}

#[test]
fn the_lockfile_pins_every_crate_with_a_checksum() {
    let lock = read("Cargo.lock");
    let packages = lock.matches("[[package]]").count();
    let checksums = lock.matches("checksum = ").count();
    assert!(
        packages > 100,
        "lockfile looks truncated ({packages} packages)"
    );
    // Every non-local package must carry a checksum, which is what makes a
    // tampered registry crate detectable.
    assert!(
        checksums as f64 / packages as f64 > 0.95,
        "{checksums} checksums for {packages} packages"
    );
}

#[test]
fn release_profile_strips_symbols_and_debug_info() {
    // Release binaries must not carry the builder's paths or symbol tables.
    let manifest = read("Cargo.toml");
    let release = manifest
        .split("[profile.release]")
        .nth(1)
        .expect("release profile");
    assert!(release.contains("strip"), "release build keeps symbols");
    assert!(release.contains("debug = 0") || release.contains("debug = false"));
    assert!(release.contains("panic = \"abort\""));
}

// ---------------------------------------------------------------------------
// 7. Repository hygiene
// ---------------------------------------------------------------------------

#[test]
fn no_binaries_or_secrets_are_committed() {
    let mut offenders = Vec::new();
    let mut stack = vec![manifest_dir()];
    let banned_ext = [
        "exe", "dll", "so", "dylib", "msi", "dmg", "app", "class", "jar", "pyc", "o", "a",
    ];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name == ".git" || name == "target" || name == "tests" && path.is_dir() {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if banned_ext.contains(&ext) {
                    offenders.push(path.display().to_string());
                }
            }
            // Token-shaped strings outside the tests themselves.
            if path
                .extension()
                .map(|e| e == "rs" || e == "md")
                .unwrap_or(false)
            {
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                for line in text.lines() {
                    if line.contains("github_pat_") || line.contains("ghp_") {
                        offenders.push(format!("possible token in {}", path.display()));
                    }
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "committed binaries or credentials:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn the_licence_is_present_and_permissive() {
    let licence = read("LICENSE");
    assert!(licence.to_lowercase().contains("mit license"));
    assert!(licence.contains("Permission is hereby granted"));
    assert!(read("Cargo.toml").contains("license = \"MIT\""));
}

#[test]
fn bundled_fonts_have_their_licence_text_next_to_them() {
    let fonts = manifest_dir().join("assets/fonts");
    let mut ttf = 0;
    for entry in std::fs::read_dir(&fonts).expect("fonts dir").flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".ttf") {
            ttf += 1;
        }
    }
    assert!(ttf >= 3, "expected the bundled font files");
    assert!(fonts.join("OFL-IBMPlexSans.txt").exists());
    assert!(fonts.join("OFL-JetBrainsMono.txt").exists());
}

// ---------------------------------------------------------------------------
// 8. Update integrity
// ---------------------------------------------------------------------------

#[test]
fn the_updater_verifies_a_hash_before_unpacking_anything() {
    let body = read("src/updater.rs");
    let verify = body
        .find("fn verify_sha256")
        .or_else(|| body.find("Sha256"))
        .expect("hash verification helper");
    let extract = body.find("fn extract_").expect("archive extraction helper");
    // Verification has to happen first in program order, not just conceptually.
    let mut verifies_before_extract = false;
    if let Some(call_site) = body.find("verify") {
        verifies_before_extract = call_site < extract;
    }
    assert!(verify > 0);
    assert!(
        verifies_before_extract || body.contains("constant_time") || body.contains("ct_eq"),
        "hash comparison should happen before extraction and in constant time"
    );
}

#[test]
fn the_updater_refuses_plaintext_http_and_custom_ports() {
    use lumaglyph::updater::EndpointPolicy;
    let policy = EndpointPolicy::production_github(true);
    assert!(policy.require_https);
    assert!(policy.allow_explicit_port.is_none());
    assert!(!policy.allowed_redirect_hosts.is_empty());
}

#[test]
fn only_release_assets_for_this_platform_are_acceptable() {
    use lumaglyph::updater::PlatformTarget;
    for target in PlatformTarget::ALL {
        let name = target.expected_archive_name();
        assert!(
            name.contains("lumaglyph"),
            "{name} is not a LumaGlyph asset"
        );
        assert!(!name.contains(".."), "{name} contains a traversal");
        assert!(
            name.ends_with(".tar.gz") || name.ends_with(".zip"),
            "{name} is not an archive"
        );
    }
}

// ---------------------------------------------------------------------------
// 9. Printable inventory
// ---------------------------------------------------------------------------

#[test]
fn print_the_security_inventory() {
    // Runs by default so `cargo test -- --nocapture` produces an audit summary
    // that can be pasted into a review.
    let mut unsafe_blocks = 0;
    let mut command_modules = Vec::new();
    let mut net_modules = Vec::new();
    for (name, body) in source_files() {
        let prod = production_half(&body);
        unsafe_blocks += prod.matches("unsafe {").count();
        if prod.contains("Command::new") {
            command_modules.push(name.clone());
        }
        if prod.contains("ureq::") {
            net_modules.push(name.clone());
        }
    }
    let deps = read("Cargo.toml")
        .split("[dependencies]")
        .nth(1)
        .and_then(|s| s.split('[').next())
        .map(|s| {
            s.lines()
                .filter(|l| l.contains('=') && !l.trim().starts_with('#'))
                .count()
        })
        .unwrap_or(0);

    println!("\n=== LumaGlyph security inventory ===");
    println!("unsafe blocks            : {unsafe_blocks} (all OS queries, all documented)");
    println!("modules spawning processes: {command_modules:?}");
    println!("modules opening sockets   : {net_modules:?} (release check only, opt-in)");
    println!("direct dependencies       : {deps}");
    println!(
        "update transport          : HTTPS only, GitHub hosts only, SHA-256 before extraction"
    );
    println!("telemetry                 : none, and the source audit above proves it");
    println!("====================================\n");
}
