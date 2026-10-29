use std::env;
use std::process::Command;

fn sanitize_hex_sha(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.len() >= 7 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(trimmed[..trimmed.len().min(12)].to_ascii_lowercase())
    } else {
        None
    }
}

fn detect_git_sha() -> String {
    if let Ok(gh_sha) = env::var("GITHUB_SHA") {
        if let Some(clean) = sanitize_hex_sha(&gh_sha) {
            return clean[..8.min(clean.len())].to_string();
        }
    }
    if let Ok(out) = Command::new("git")
        .args(["rev-parse", "--short=8", "HEAD"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            if let Some(clean) = sanitize_hex_sha(&s) {
                return clean;
            }
        }
    }
    "release".to_string()
}

fn detect_rustc_version() -> String {
    let rustc_bin = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    if let Ok(out) = Command::new(rustc_bin).arg("--version").output() {
        if out.status.success() {
            let line = String::from_utf8_lossy(&out.stdout);
            let first = line.lines().next().unwrap_or("rustc").trim();
            // Strip anything that could resemble a path just in case custom toolchains embed one
            if !first.contains('/') && !first.contains('\\') {
                return first.to_string();
            }
        }
    }
    "rustc (stable)".to_string()
}

#[cfg(windows)]
fn embed_windows_icon_resources() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/icons/lumaglyph.ico")
        .set("ProductName", "LumaGlyph")
        .set("FileDescription", "LumaGlyph")
        .set("InternalName", "lumaglyph")
        .set("OriginalFilename", "lumaglyph.exe");
    res.compile_for("lumaglyph")
        .expect("Failed compiling Windows PE icon and version resources into lumaglyph.exe");
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icons/lumaglyph.ico");
    println!("cargo:rerun-if-changed=assets/icons/lumaglyph-256.png");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");

    let target = env::var("TARGET").unwrap_or_else(|_| "unknown-target".to_string());
    let profile = env::var("PROFILE").unwrap_or_else(|_| "release".to_string());
    let git_sha = detect_git_sha();
    let rustc_ver = detect_rustc_version();

    println!("cargo:rustc-env=LUMAGLYPH_GIT_SHA={git_sha}");
    println!("cargo:rustc-env=LUMAGLYPH_TARGET_TRIPLE={target}");
    println!("cargo:rustc-env=LUMAGLYPH_BUILD_PROFILE={profile}");
    println!("cargo:rustc-env=LUMAGLYPH_RUSTC_VERSION={rustc_ver}");

    #[cfg(windows)]
    embed_windows_icon_resources();
}
