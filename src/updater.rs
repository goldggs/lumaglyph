use crate::charset::CharsetPreset;
use crate::color::ColorMode;
use crate::theme::{AccentPreset, CheckerContrast, CornerStyle, StudioTheme, UiDensity};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

/// Default canonical GitHub repository slug (`owner/repo`) used ONLY when the user opts in
/// or explicitly triggers a release check / verified update download.
pub const DEFAULT_GITHUB_REPO: &str = match option_env!("LUMAGLYPH_GITHUB_REPO") {
    Some(repo) => repo,
    None => "goldggs/lumaglyph",
};

/// Canonical documentation & download entry points surfaced in the app's Help menu.
/// Every link is HTTPS, is on `github.com`, and points at the same canonical repository.
pub const REPO_URL: &str = "https://github.com/goldggs/lumaglyph";
pub const LATEST_RELEASE_URL: &str = "https://github.com/goldggs/lumaglyph/releases/latest";
pub const DOCS_INDEX_URL: &str = "https://github.com/goldggs/lumaglyph/blob/main/docs/README.md";
pub const DOCS_INSTALL_URL: &str = "https://github.com/goldggs/lumaglyph/blob/main/docs/INSTALL.md";
pub const DOCS_QUICKSTART_URL: &str =
    "https://github.com/goldggs/lumaglyph/blob/main/docs/QUICKSTART.md";
pub const DOCS_USER_GUIDE_URL: &str =
    "https://github.com/goldggs/lumaglyph/blob/main/docs/USER-GUIDE.md";
pub const DOCS_TROUBLESHOOTING_URL: &str =
    "https://github.com/goldggs/lumaglyph/blob/main/docs/TROUBLESHOOTING.md";
pub const DOCS_DEVELOPING_URL: &str =
    "https://github.com/goldggs/lumaglyph/blob/main/docs/DEVELOPING.md";

/// Every documentation link the app can open, for audit tests.
pub const ALL_DOC_LINKS: [(&str, &str); 7] = [
    ("Repository", REPO_URL),
    ("Latest Downloads", LATEST_RELEASE_URL),
    ("Documentation Index", DOCS_INDEX_URL),
    ("Installation Guide", DOCS_INSTALL_URL),
    ("Quick Start", DOCS_QUICKSTART_URL),
    ("User Guide", DOCS_USER_GUIDE_URL),
    ("Troubleshooting", DOCS_TROUBLESHOOTING_URL),
];

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const BUILD_GIT_SHA: &str = env!("LUMAGLYPH_GIT_SHA");
pub const BUILD_TARGET_TRIPLE: &str = env!("LUMAGLYPH_TARGET_TRIPLE");
pub const BUILD_PROFILE: &str = env!("LUMAGLYPH_BUILD_PROFILE");
pub const BUILD_RUSTC_VERSION: &str = env!("LUMAGLYPH_RUSTC_VERSION");

/// Maximum bytes accepted from the GitHub Releases JSON API (`256 KiB`).
pub const MAX_API_JSON_BYTES: usize = 256 * 1024;
/// Maximum bytes accepted for `SHA256SUMS.txt` (`16 KiB`).
pub const MAX_CHECKSUM_FILE_BYTES: usize = 16 * 1024;
/// Minimum plausible archive size (`256 bytes` floor; rejects 0-byte or stub payloads).
pub const MIN_ARCHIVE_BYTES: usize = 256;
/// Maximum archive download size (`64 MiB`).
pub const MAX_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;
/// Maximum single extracted binary size (`64 MiB`).
pub const MAX_BINARY_BYTES: usize = 64 * 1024 * 1024;
/// Maximum total decompressed bytes across all entries in an archive (`96 MiB`).
pub const MAX_TOTAL_DECOMPRESSED_BYTES: usize = 96 * 1024 * 1024;
/// Maximum entries permitted inside an official release archive (`lumaglyph[.exe]`, `LICENSE`, `README.md`).
pub const MAX_ARCHIVE_ENTRIES: usize = 8;
/// Maximum HTTP redirect hops allowed when downloading assets from GitHub CDN.
pub const MAX_REDIRECT_HOPS: usize = 3;

static TMP_NONCE: AtomicU64 = AtomicU64::new(1);

/// Plain-text structured build & runtime report (no host build paths).
pub fn format_build_report(
    gpu_renderer: Option<&str>,
    ffmpeg_info: Option<&str>,
    rss: &str,
) -> String {
    let threads = rayon::current_num_threads();
    let gpu = gpu_renderer.unwrap_or("Headless / CLI");
    let ff = ffmpeg_info.unwrap_or("Not detected on PATH");
    format!(
        "LumaGlyph v{APP_VERSION} ({BUILD_GIT_SHA})\n\
         Target        : {BUILD_TARGET_TRIPLE}\n\
         Profile       : {BUILD_PROFILE}\n\
         Compiler      : {BUILD_RUSTC_VERSION}\n\
         Worker Threads: {threads}\n\
         GPU Renderer  : {gpu}\n\
         FFmpeg Pipe   : {ff}\n\
         Memory (RSS)  : {rss}\n\
         Network Mode  : Offline by default (opt-in GitHub release check)"
    )
}

/// Persistent workstation settings stored locally in JSON.
/// Strictly 100% offline by default (`check_updates_on_startup = false`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppPreferences {
    // Appearance
    pub theme: StudioTheme,
    pub accent: AccentPreset,
    pub ui_density: UiDensity,
    pub corner_style: CornerStyle,
    pub checker_contrast: CheckerContrast,
    pub show_cell_hud: bool,
    pub show_canvas_border: bool,
    pub show_status_bar: bool,

    // Engine & Startup Defaults
    pub default_cols: u32,
    pub default_charset: CharsetPreset,
    pub default_color_mode: ColorMode,
    pub default_oled_black: bool,
    pub atlas_cell_height: usize,

    // Export & Video Pipeline
    pub png_scale_preset: u8,
    pub video_crf: u8,
    pub ffmpeg_custom_dir: String,

    // Optimisations (background idle, decoder budget, threading)
    pub idle_when_unfocused: bool,
    pub pause_decode_when_unfocused: bool,
    pub idle_when_minimized: bool,
    pub trim_memory_on_idle: bool,
    pub worker_threads: usize,
    pub video_preview_max_dim: u32,
    pub audio_envelope_enabled: bool,
    pub frame_dedup_enabled: bool,

    // Rendering & Quality
    pub fps_cap: u32,
    pub vsync: bool,
    pub msaa_samples: u16,
    pub texture_filter: TextureFilterMode,
    pub preview_supersample: u8,
    pub show_fps_readout: bool,
    pub animation_smoothing: bool,

    // Privacy & Release Checks
    pub check_updates_on_startup: bool,
    pub github_repo: String,
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            theme: StudioTheme::GraphitePro,
            accent: AccentPreset::ThemeDefault,
            ui_density: UiDensity::Standard100,
            corner_style: CornerStyle::Precision2px,
            checker_contrast: CheckerContrast::Subtle,
            show_cell_hud: true,
            show_canvas_border: true,
            show_status_bar: true,
            default_cols: 156,
            default_charset: CharsetPreset::Genuine95,
            default_color_mode: ColorMode::TrueColor,
            default_oled_black: false,
            atlas_cell_height: 20,
            png_scale_preset: 2,
            video_crf: 18,
            ffmpeg_custom_dir: String::new(),
            idle_when_unfocused: true,
            pause_decode_when_unfocused: true,
            idle_when_minimized: true,
            trim_memory_on_idle: true,
            worker_threads: 0,
            video_preview_max_dim: 640,
            audio_envelope_enabled: true,
            frame_dedup_enabled: true,
            fps_cap: 60,
            vsync: true,
            msaa_samples: 4,
            texture_filter: TextureFilterMode::Auto,
            preview_supersample: 1,
            show_fps_readout: false,
            animation_smoothing: true,
            check_updates_on_startup: false,
            github_repo: DEFAULT_GITHUB_REPO.to_string(),
        }
    }
}

/// Texture sampling policy for the rendered ASCII canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TextureFilterMode {
    /// Nearest above 180% zoom, linear below (crisp glyph edges at all scales).
    #[default]
    Auto,
    /// Always nearest-neighbour (sharpest single-pixel glyph cell edges).
    Nearest,
    /// Always bilinear (smoothest motion when zoomed).
    Linear,
}

impl TextureFilterMode {
    pub const ALL: [Self; 3] = [Self::Auto, Self::Nearest, Self::Linear];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto (Nearest when zoomed)",
            Self::Nearest => "Nearest Neighbor (Crisp)",
            Self::Linear => "Bilinear (Smooth)",
        }
    }

    /// Resolves the concrete sampling mode for the current canvas zoom factor.
    pub fn resolve(self, zoom: f32) -> &'static str {
        match self {
            Self::Auto => {
                if zoom >= 1.8 {
                    "nearest"
                } else {
                    "linear"
                }
            }
            Self::Nearest => "nearest",
            Self::Linear => "linear",
        }
    }
}

/// Candidate frame-rate caps offered in `Settings -> Rendering & Quality` (`0` = uncapped).
pub const FPS_CAP_OPTIONS: [u32; 6] = [0, 15, 24, 30, 60, 144];

impl AppPreferences {
    /// Effective repaint interval for the current FPS cap (`None` = uncapped/vsync-driven).
    pub fn repaint_interval(&self) -> Option<std::time::Duration> {
        match self.fps_cap {
            0 => None,
            cap => Some(std::time::Duration::from_secs_f64(
                1.0 / f64::from(cap.clamp(1, 480)),
            )),
        }
    }

    /// Effective worker-thread count for the parallel ASCII engine (`0` = automatic).
    pub fn effective_worker_threads(&self) -> usize {
        if self.worker_threads == 0 {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        } else {
            self.worker_threads.clamp(1, 64)
        }
    }

    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("lumaglyph").join("preferences.json"))
    }

    pub fn load() -> Self {
        Self::config_path()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str::<Self>(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let Some(path) = Self::config_path() else {
            return Err("Could not determine OS config directory".to_string());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Failed creating config dir: {e}"))?;
        }
        let body = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed serializing config: {e}"))?;
        fs::write(path, body).map_err(|e| format!("Failed saving preferences: {e}"))
    }

    pub fn png_cell_height(&self) -> usize {
        match self.png_scale_preset {
            1 => 20,
            3 => 54,
            _ => 36,
        }
    }
}

/// Supported prebuilt release platform targets and their deterministic archive/binary contracts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformTarget {
    LinuxX86_64,
    MacosArm64,
    MacosX86_64,
    WindowsX86_64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    TarGz,
    Zip,
}

impl PlatformTarget {
    pub const ALL: [PlatformTarget; 4] = [
        PlatformTarget::LinuxX86_64,
        PlatformTarget::MacosArm64,
        PlatformTarget::MacosX86_64,
        PlatformTarget::WindowsX86_64,
    ];

    pub fn current() -> Option<Self> {
        Self::from_os_arch(std::env::consts::OS, std::env::consts::ARCH)
    }

    pub fn from_os_arch(os: &str, arch: &str) -> Option<Self> {
        match (os, arch) {
            ("linux", "x86_64") => Some(Self::LinuxX86_64),
            ("macos", "aarch64") => Some(Self::MacosArm64),
            ("macos", "x86_64") => Some(Self::MacosX86_64),
            ("windows", "x86_64") => Some(Self::WindowsX86_64),
            _ => None,
        }
    }

    pub fn expected_archive_name(self) -> &'static str {
        match self {
            Self::LinuxX86_64 => "lumaglyph-linux-x86_64.tar.gz",
            Self::MacosArm64 => "lumaglyph-macos-arm64.tar.gz",
            Self::MacosX86_64 => "lumaglyph-macos-x86_64.tar.gz",
            Self::WindowsX86_64 => "lumaglyph-windows-x86_64.zip",
        }
    }

    pub fn expected_binary_name(self) -> &'static str {
        match self {
            Self::WindowsX86_64 => "lumaglyph.exe",
            Self::LinuxX86_64 | Self::MacosArm64 | Self::MacosX86_64 => "lumaglyph",
        }
    }

    pub fn archive_format(self) -> ArchiveFormat {
        match self {
            Self::WindowsX86_64 => ArchiveFormat::Zip,
            Self::LinuxX86_64 | Self::MacosArm64 | Self::MacosX86_64 => ArchiveFormat::TarGz,
        }
    }

    pub fn output_filename_for_tag(self, validated_tag: &str) -> String {
        match self {
            Self::WindowsX86_64 => format!("lumaglyph-{validated_tag}.exe"),
            Self::LinuxX86_64 | Self::MacosArm64 | Self::MacosX86_64 => {
                format!("lumaglyph-{validated_tag}")
            }
        }
    }
}

/// Transport & origin policy. Production callers ALWAYS use `EndpointPolicy::production_github()`,
/// which enforces strict `https://`, zero custom ports, zero credentials, and exact GitHub hosts.
#[derive(Debug, Clone)]
pub struct EndpointPolicy {
    pub api_base: String,
    pub release_host: String,
    pub allowed_redirect_hosts: Vec<String>,
    pub require_https: bool,
    pub allow_explicit_port: Option<u16>,
    pub require_canonical_repo: bool,
}

impl EndpointPolicy {
    pub fn production_github(require_canonical_repo: bool) -> Self {
        Self {
            api_base: "https://api.github.com".to_string(),
            release_host: "github.com".to_string(),
            allowed_redirect_hosts: vec![
                "github.com".to_string(),
                "objects.githubusercontent.com".to_string(),
                "release-assets.githubusercontent.com".to_string(),
            ],
            require_https: true,
            allow_explicit_port: None,
            require_canonical_repo,
        }
    }

    /// Loopback mock-server policy for deterministic security & fault-injection testing.
    pub fn loopback_mock(port: u16, require_canonical_repo: bool) -> Self {
        Self {
            api_base: format!("http://127.0.0.1:{port}"),
            release_host: "127.0.0.1".to_string(),
            allowed_redirect_hosts: vec!["127.0.0.1".to_string()],
            require_https: false,
            allow_explicit_port: Some(port),
            require_canonical_repo,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedUrl {
    pub scheme: String,
    pub host: String,
    pub port: Option<u16>,
    pub path: String,
    pub query: Option<String>,
}

impl ParsedUrl {
    /// Strict zero-ambiguity URL parser rejecting control chars, spaces, backslashes,
    /// `@` userinfo spoofing, fragments, empty hosts, and `..` path traversal segments.
    pub fn parse_strict(raw: &str) -> Result<Self, String> {
        if raw.is_empty() || raw.len() > 2048 {
            return Err("URL is empty or exceeds maximum length (2048)".to_string());
        }
        if raw
            .bytes()
            .any(|b| b <= 0x20 || b >= 0x7F || b == b'\\' || b == b'#')
        {
            return Err(
                "URL contains forbidden whitespace, control, backslash, or fragment characters"
                    .to_string(),
            );
        }

        let Some((scheme_raw, rest)) = raw.split_once("://") else {
            return Err("URL is missing '://' scheme separator".to_string());
        };
        let scheme = scheme_raw.to_ascii_lowercase();
        if scheme != "https" && scheme != "http" {
            return Err(format!(
                "Unsupported URL scheme '{scheme}': only https is allowed"
            ));
        }
        // Require exact lowercase scheme in raw input so mixed-case tricks fail early
        if scheme_raw != scheme {
            return Err("URL scheme must be lowercase".to_string());
        }

        let (authority, path_and_query) = match rest.find('/') {
            Some(idx) => (&rest[..idx], &rest[idx..]),
            None => {
                if rest.contains('?') {
                    return Err("URL query without path slash is rejected".to_string());
                }
                (rest, "/")
            }
        };

        if authority.is_empty() {
            return Err("URL authority/host is empty".to_string());
        }
        if authority.contains('@') {
            return Err(
                "URL userinfo ('@') is forbidden (credential/host spoofing protection)".to_string(),
            );
        }
        if authority.starts_with('[') || authority.ends_with(']') {
            return Err("IPv6 literal URL hosts are not permitted".to_string());
        }

        let (host_raw, port) = match authority.split_once(':') {
            Some((h, p_str)) => {
                if p_str.is_empty() || p_str.len() > 5 || !p_str.bytes().all(|b| b.is_ascii_digit())
                {
                    return Err("Invalid URL port specification".to_string());
                }
                let p: u16 = p_str
                    .parse()
                    .map_err(|_| "URL port out of u16 range".to_string())?;
                if p == 0 {
                    return Err("URL port 0 is invalid".to_string());
                }
                (h, Some(p))
            }
            None => (authority, None),
        };

        if host_raw.is_empty()
            || host_raw.starts_with('.')
            || host_raw.ends_with('.')
            || host_raw.contains("..")
            || !host_raw
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
        {
            return Err(format!("Malformed URL hostname '{host_raw}'"));
        }
        let host = host_raw.to_ascii_lowercase();

        let (path, query) = match path_and_query.split_once('?') {
            Some((p, q)) => (p, Some(q.to_string())),
            None => (path_and_query, None),
        };

        if !path.starts_with('/') {
            return Err("URL path must start with '/'".to_string());
        }
        // Reject raw or percent-encoded path traversal (`..`, `%2e`, `%2f`, `%5c`) in the URL path,
        // and NUL bytes (`%00`) anywhere in path or query. (Presigned CDN query params such as
        // `response-content-type=application%2Foctet-stream` legitimately contain `%2F` in `query`.)
        let lower_path = path.to_ascii_lowercase();
        let lower_pq = path_and_query.to_ascii_lowercase();
        if path.split('/').any(|seg| seg == ".." || seg == ".")
            || lower_path.contains("%2e")
            || lower_path.contains("%2f")
            || lower_path.contains("%5c")
            || lower_pq.contains("%00")
        {
            return Err("URL path traversal or encoded separator is forbidden".to_string());
        }

        Ok(Self {
            scheme,
            host,
            port,
            path: path.to_string(),
            query,
        })
    }

    fn verify_transport(&self, policy: &EndpointPolicy) -> Result<(), String> {
        if policy.require_https && self.scheme != "https" {
            return Err(format!(
                "Insecure URL scheme '{}': HTTPS is strictly required",
                self.scheme
            ));
        }
        if !policy.require_https && self.scheme != "http" && self.scheme != "https" {
            return Err(format!("Disallowed URL scheme '{}'", self.scheme));
        }
        if self.port != policy.allow_explicit_port {
            return Err(format!(
                "Disallowed URL port {:?} (expected {:?})",
                self.port, policy.allow_explicit_port
            ));
        }
        // In production mode, reject raw IPv4 literals even if malformed DNS tried them
        if policy.require_https
            && self
                .host
                .split('.')
                .all(|part| part.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err("Raw IP address hosts are forbidden in release URLs".to_string());
        }
        Ok(())
    }
}

/// Validates a GitHub `owner/repo` slug against injection, path traversal, and spoofing.
pub fn validate_repo_slug(repo: &str, require_canonical: bool) -> Result<(&str, &str), String> {
    let trimmed = repo.trim();
    if trimmed.is_empty() || trimmed.starts_with("YOUR_GITHUB_USERNAME/") {
        return Err("Repository slug is not configured".to_string());
    }
    let Some((owner, name)) = trimmed.split_once('/') else {
        return Err("Repository slug must be in 'owner/repo' format".to_string());
    };
    if name.contains('/') {
        return Err("Repository slug must contain exactly one '/'".to_string());
    }
    if owner.is_empty() || owner.len() > 39 || name.is_empty() || name.len() > 100 {
        return Err("Repository owner or name length is invalid".to_string());
    }
    if owner == "." || owner == ".." || name == "." || name == ".." {
        return Err("Repository slug cannot use '.' or '..'".to_string());
    }
    if owner.starts_with('-') || owner.ends_with('-') || owner.starts_with('.') {
        return Err("Invalid GitHub owner identifier".to_string());
    }
    let valid_seg = |s: &str| {
        s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
    };
    if !valid_seg(owner) || !valid_seg(name) {
        return Err("Repository slug contains forbidden characters".to_string());
    }
    if require_canonical && !trimmed.eq_ignore_ascii_case(DEFAULT_GITHUB_REPO) {
        return Err(format!(
            "Repository '{trimmed}' does not match canonical repository '{DEFAULT_GITHUB_REPO}'"
        ));
    }
    Ok((owner, name))
}

/// Validates a stable release tag (`vX.Y.Z` or `X.Y.Z`), rejecting pre-releases, shell chars, and traversal.
pub fn validate_release_tag(tag: &str) -> Result<&str, String> {
    if tag.is_empty() || tag.len() > 24 {
        return Err("Release tag is empty or exceeds 24 characters".to_string());
    }
    if tag.bytes().any(|b| b <= 0x20 || b >= 0x7F) {
        return Err("Release tag contains whitespace or non-ASCII characters".to_string());
    }
    let body = tag
        .strip_prefix('v')
        .or_else(|| tag.strip_prefix('V'))
        .unwrap_or(tag);
    let parts: Vec<&str> = body.split('.').collect();
    if parts.len() != 3 {
        return Err(format!(
            "Release tag '{tag}' must be strict 3-part SemVer (vMAJOR.MINOR.PATCH)"
        ));
    }
    for p in parts {
        if p.is_empty() || p.len() > 5 || !p.bytes().all(|b| b.is_ascii_digit()) {
            return Err(format!(
                "Invalid numeric component '{p}' in release tag '{tag}'"
            ));
        }
    }
    Ok(tag)
}

/// Validates that `html_url` points strictly to `https://github.com/{owner}/{repo}/releases/tag/{tag}`.
pub fn validate_release_html_url(
    url: &str,
    owner: &str,
    repo: &str,
    tag: &str,
    policy: &EndpointPolicy,
) -> Result<(), String> {
    let parsed = ParsedUrl::parse_strict(url)?;
    parsed.verify_transport(policy)?;
    if parsed.host != policy.release_host {
        return Err(format!(
            "Untrusted release page host '{}' (expected '{}')",
            parsed.host, policy.release_host
        ));
    }
    if parsed.query.is_some() {
        return Err("Release HTML URL must not contain query parameters".to_string());
    }
    let expected_path = format!(
        "/{}/{}/releases/tag/{}",
        owner.to_ascii_lowercase(),
        repo.to_ascii_lowercase(),
        tag
    );
    let actual_check = {
        // Case-insensitive owner/repo prefix, exact tag match
        let segs: Vec<&str> = parsed.path.split('/').collect();
        segs.len() == 6
            && segs[0].is_empty()
            && segs[1].eq_ignore_ascii_case(owner)
            && segs[2].eq_ignore_ascii_case(repo)
            && segs[3] == "releases"
            && segs[4] == "tag"
            && segs[5] == tag
    };
    if !actual_check {
        return Err(format!(
            "Release HTML URL path '{}' does not match expected '{expected_path}'",
            parsed.path
        ));
    }
    Ok(())
}

/// Validates that a GitHub asset `browser_download_url` points strictly to
/// `https://github.com/{owner}/{repo}/releases/download/{tag}/{asset_name}`.
pub fn validate_asset_download_url(
    url: &str,
    owner: &str,
    repo: &str,
    tag: &str,
    asset_name: &str,
    policy: &EndpointPolicy,
) -> Result<(), String> {
    let parsed = ParsedUrl::parse_strict(url)?;
    parsed.verify_transport(policy)?;
    if parsed.host != policy.release_host {
        return Err(format!(
            "Untrusted asset download host '{}' (expected '{}')",
            parsed.host, policy.release_host
        ));
    }
    if parsed.query.is_some() {
        return Err("Initial asset download URL must not contain query parameters".to_string());
    }
    let segs: Vec<&str> = parsed.path.split('/').collect();
    let ok = segs.len() == 7
        && segs[0].is_empty()
        && segs[1].eq_ignore_ascii_case(owner)
        && segs[2].eq_ignore_ascii_case(repo)
        && segs[3] == "releases"
        && segs[4] == "download"
        && segs[5] == tag
        && segs[6] == asset_name;
    if !ok {
        return Err(format!(
            "Asset URL path '{}' does not match '/{owner}/{repo}/releases/download/{tag}/{asset_name}'",
            parsed.path
        ));
    }
    Ok(())
}

/// Validates every redirect `Location` hop against the strict CDN host allowlist and HTTPS transport.
pub fn validate_redirect_url(url: &str, policy: &EndpointPolicy) -> Result<(), String> {
    let parsed = ParsedUrl::parse_strict(url)?;
    parsed.verify_transport(policy)?;
    if !policy
        .allowed_redirect_hosts
        .iter()
        .any(|h| h == &parsed.host)
    {
        return Err(format!("Blocked untrusted redirect host '{}'", parsed.host));
    }
    Ok(())
}

/// Validates a flat filename (no directories, drives, traversal, or control bytes).
pub fn validate_flat_filename(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 128 {
        return Err("Filename is empty or exceeds 128 bytes".to_string());
    }
    if name.starts_with('.') || name.ends_with('.') || name.ends_with(' ') {
        return Err(format!(
            "Disallowed leading/trailing character in filename '{name}'"
        ));
    }
    if name.bytes().any(|b| {
        b <= 0x1F
            || b >= 0x7F
            || matches!(
                b,
                b'/' | b'\\' | b':' | b'*' | b'?' | b'"' | b'<' | b'>' | b'|'
            )
    }) {
        return Err(format!(
            "Forbidden path/shell character in filename '{name}'"
        ));
    }
    if name.contains("..") {
        return Err(format!("Path traversal sequence in filename '{name}'"));
    }
    Ok(())
}

/// Computes FIPS 180-4 SHA-256 over `data`.
pub fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let out = hasher.finalize();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    arr
}

/// Formats a 32-byte SHA-256 digest as 64 lowercase hex characters.
pub fn hex_encode_32(digest: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for &b in digest {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0F) as usize] as char);
    }
    out
}

/// Parses a 64-character hex string into `[u8; 32]`.
pub fn parse_hex_32(hex_str: &str) -> Result<[u8; 32], String> {
    if hex_str.len() != 64 {
        return Err(format!(
            "SHA-256 hex digest must be 64 characters, got {}",
            hex_str.len()
        ));
    }
    let bytes = hex_str.as_bytes();
    let nibble = |c: u8| -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    };
    let mut out = [0u8; 32];
    for i in 0..32 {
        let hi = nibble(bytes[i * 2])
            .ok_or_else(|| "Non-hex character in SHA-256 digest".to_string())?;
        let lo = nibble(bytes[i * 2 + 1])
            .ok_or_else(|| "Non-hex character in SHA-256 digest".to_string())?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

/// Constant-time 32-byte digest comparison (zero early-exit timing leak).
pub fn constant_time_eq_32(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// Strictly parses `SHA256SUMS.txt`, rejecting malformed lines, unsafe filenames, and duplicate entries.
pub fn parse_sha256sums(text: &str) -> Result<BTreeMap<String, [u8; 32]>, String> {
    if text.len() > MAX_CHECKSUM_FILE_BYTES {
        return Err("SHA256SUMS.txt exceeds maximum allowed size (16 KiB)".to_string());
    }
    if text.bytes().any(|b| b == 0) {
        return Err("SHA256SUMS.txt contains NUL bytes".to_string());
    }

    let mut map = BTreeMap::new();
    for (line_idx, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim_end_matches('\r').trim();
        if line.is_empty() {
            continue;
        }
        if line.len() < 66 || !line.is_ascii() {
            return Err(format!(
                "Malformed SHA256SUMS.txt at line {}: non-ASCII or line too short",
                line_idx + 1
            ));
        }
        let hex_part = &line[..64];
        let digest = parse_hex_32(hex_part)
            .map_err(|e| format!("SHA256SUMS.txt line {}: {e}", line_idx + 1))?;
        let sep_and_name = &line[64..];
        let raw_name = if let Some(rest) = sep_and_name.strip_prefix("  ") {
            rest
        } else if let Some(rest) = sep_and_name.strip_prefix(" *") {
            rest
        } else if let Some(rest) = sep_and_name.strip_prefix(' ') {
            rest
        } else {
            return Err(format!(
                "Malformed separator in SHA256SUMS.txt at line {}",
                line_idx + 1
            ));
        };
        let filename = raw_name.trim();
        validate_flat_filename(filename).map_err(|e| {
            format!(
                "Unsafe filename in SHA256SUMS.txt line {}: {e}",
                line_idx + 1
            )
        })?;

        if map.insert(filename.to_string(), digest).is_some() {
            return Err(format!(
                "Duplicate filename '{filename}' in SHA256SUMS.txt (hash-confusion protection)"
            ));
        }
    }

    if map.is_empty() {
        return Err("SHA256SUMS.txt contained zero valid checksum entries".to_string());
    }
    Ok(map)
}

/// Validates an archive member path (`TarSlip` / `ZipSlip` / unexpected payload protection).
/// Official LumaGlyph archives contain ONLY `lumaglyph[.exe]`, `LICENSE`, and `README.md`.
pub fn validate_archive_member_name(
    raw_name: &str,
    expected_binary: &str,
) -> Result<String, String> {
    if raw_name.is_empty() || raw_name.len() > 128 {
        return Err("Archive member name is empty or too long".to_string());
    }
    if raw_name.bytes().any(|b| b <= 0x1F || b >= 0x7F) {
        return Err("Archive member name contains control or non-ASCII bytes".to_string());
    }
    if raw_name.contains('\\') || raw_name.contains(':') {
        return Err(format!(
            "Archive member '{raw_name}' contains backslash or drive separator (ZipSlip blocked)"
        ));
    }
    // Strip optional harmless './' prefix ONLY once if flat; reject any subdirectories or '..'
    let cleaned = raw_name.strip_prefix("./").unwrap_or(raw_name);
    if cleaned.contains('/') || cleaned.contains("..") || cleaned == "." || cleaned.is_empty() {
        return Err(format!(
            "Archive member '{raw_name}' is not a flat root file (path traversal blocked)"
        ));
    }
    validate_flat_filename(cleaned)?;

    let allowed = cleaned == expected_binary
        || cleaned.eq_ignore_ascii_case("LICENSE")
        || cleaned.eq_ignore_ascii_case("LICENSE.txt")
        || cleaned.eq_ignore_ascii_case("README.md");

    if !allowed {
        return Err(format!(
            "Unexpected file '{cleaned}' inside release archive (strict allowlist rejected)"
        ));
    }
    Ok(cleaned.to_string())
}

/// Verifies the native OS + CPU machine-code header of an extracted executable before saving.
pub fn verify_executable_header(bin: &[u8], target: PlatformTarget) -> Result<(), String> {
    if bin.len() < 256 {
        return Err(format!(
            "Extracted binary is too small ({} bytes) to be a valid native executable",
            bin.len()
        ));
    }
    if bin.len() > MAX_BINARY_BYTES {
        return Err("Extracted binary exceeds maximum allowed size (64 MiB)".to_string());
    }

    match target {
        PlatformTarget::LinuxX86_64 => {
            if &bin[0..4] != b"\x7FELF" {
                return Err(
                    "Binary header mismatch: expected Linux ELF magic (\\x7FELF)".to_string(),
                );
            }
            if bin[4] != 2 || bin[5] != 1 {
                return Err("Binary header mismatch: expected 64-bit little-endian ELF".to_string());
            }
            let e_type = u16::from_le_bytes([bin[16], bin[17]]);
            if e_type != 2 && e_type != 3 {
                return Err(format!(
                    "Binary header mismatch: expected ELF executable (ET_EXEC/ET_DYN), got {e_type}"
                ));
            }
            let e_machine = u16::from_le_bytes([bin[18], bin[19]]);
            if e_machine != 0x003E {
                return Err(format!(
                    "Binary architecture mismatch: expected EM_X86_64 (0x3E), got 0x{e_machine:04X}"
                ));
            }
            Ok(())
        }
        PlatformTarget::MacosArm64 => {
            let magic = u32::from_le_bytes([bin[0], bin[1], bin[2], bin[3]]);
            if magic != 0xFEED_FACF {
                return Err(format!(
                    "Binary header mismatch: expected 64-bit Mach-O magic (0xFEEDFACF), got 0x{magic:08X}"
                ));
            }
            let cputype = u32::from_le_bytes([bin[4], bin[5], bin[6], bin[7]]);
            if cputype != 0x0100_000C {
                return Err(format!(
                    "Binary architecture mismatch: expected macOS ARM64 (0x0100000C), got 0x{cputype:08X}"
                ));
            }
            let filetype = u32::from_le_bytes([bin[12], bin[13], bin[14], bin[15]]);
            if filetype != 0x0000_0002 {
                return Err(format!(
                    "Binary header mismatch: expected Mach-O MH_EXECUTE (2), got {filetype}"
                ));
            }
            Ok(())
        }
        PlatformTarget::MacosX86_64 => {
            let magic = u32::from_le_bytes([bin[0], bin[1], bin[2], bin[3]]);
            if magic != 0xFEED_FACF {
                return Err(format!(
                    "Binary header mismatch: expected 64-bit Mach-O magic (0xFEEDFACF), got 0x{magic:08X}"
                ));
            }
            let cputype = u32::from_le_bytes([bin[4], bin[5], bin[6], bin[7]]);
            if cputype != 0x0100_0007 {
                return Err(format!(
                    "Binary architecture mismatch: expected macOS x86_64 (0x01000007), got 0x{cputype:08X}"
                ));
            }
            let filetype = u32::from_le_bytes([bin[12], bin[13], bin[14], bin[15]]);
            if filetype != 0x0000_0002 {
                return Err(format!(
                    "Binary header mismatch: expected Mach-O MH_EXECUTE (2), got {filetype}"
                ));
            }
            Ok(())
        }
        PlatformTarget::WindowsX86_64 => {
            if &bin[0..2] != b"MZ" {
                return Err("Binary header mismatch: expected Windows MZ header".to_string());
            }
            let pe_off = u32::from_le_bytes([bin[0x3C], bin[0x3D], bin[0x3E], bin[0x3F]]) as usize;
            let min_needed = pe_off
                .checked_add(24 + 70)
                .ok_or_else(|| "Malformed PE header offset overflow".to_string())?;
            if pe_off < 0x40 || min_needed > bin.len() {
                return Err("Malformed PE header offset out of bounds".to_string());
            }
            if &bin[pe_off..pe_off + 4] != b"PE\0\0" {
                return Err("Binary header mismatch: missing PE\\0\\0 signature".to_string());
            }
            let machine = u16::from_le_bytes([bin[pe_off + 4], bin[pe_off + 5]]);
            if machine != 0x8664 {
                return Err(format!(
                    "Binary architecture mismatch: expected AMD64 (0x8664), got 0x{machine:04X}"
                ));
            }
            let opt_magic = u16::from_le_bytes([bin[pe_off + 24], bin[pe_off + 25]]);
            if opt_magic != 0x020B {
                return Err(format!(
                    "Binary header mismatch: expected PE32+ (0x020B), got 0x{opt_magic:04X}"
                ));
            }
            let subsystem = u16::from_le_bytes([bin[pe_off + 24 + 68], bin[pe_off + 24 + 69]]);
            if subsystem != 2 {
                return Err(format!(
                    "Windows binary subsystem mismatch: expected WINDOWS_GUI (2), got {subsystem}"
                ));
            }
            Ok(())
        }
    }
}

/// Safely extracts and validates the single expected binary from an authenticated `.tar.gz` or `.zip` archive
/// in memory with hard decompression caps (immune to zip/gzip bombs, path traversal, symlinks, and duplicate shadowing).
pub fn extract_verified_binary_from_archive(
    archive_bytes: &[u8],
    target: PlatformTarget,
) -> Result<Vec<u8>, String> {
    if archive_bytes.len() < MIN_ARCHIVE_BYTES || archive_bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(format!(
            "Archive size ({} bytes) is outside allowed bounds",
            archive_bytes.len()
        ));
    }

    let expected_bin = target.expected_binary_name();
    let mut seen_names = BTreeSet::new();
    let mut total_decompressed: usize = 0;
    let mut extracted_bin: Option<Vec<u8>> = None;

    match target.archive_format() {
        ArchiveFormat::TarGz => {
            let gz = GzDecoder::new(Cursor::new(archive_bytes));
            let mut archive = tar::Archive::new(gz);
            let entries = archive
                .entries()
                .map_err(|e| format!("Invalid tar.gz archive stream: {e}"))?;

            for (idx, entry_res) in entries.enumerate() {
                if idx >= MAX_ARCHIVE_ENTRIES {
                    return Err(
                        "Archive contains too many entries (archive bomb protection)".to_string(),
                    );
                }
                let entry = entry_res.map_err(|e| format!("Corrupt tar entry: {e}"))?;
                let entry_type = entry.header().entry_type();
                if !entry_type.is_file() {
                    return Err(format!(
                        "Disallowed non-regular tar entry type '{entry_type:?}' (symlinks/dirs/devices blocked)"
                    ));
                }

                let path_bytes = entry.path_bytes();
                let raw_name = std::str::from_utf8(&path_bytes)
                    .map_err(|_| "Non-UTF8 path inside tar archive".to_string())?;
                let clean_name = validate_archive_member_name(raw_name, expected_bin)?;

                if !seen_names.insert(clean_name.clone()) {
                    return Err(format!(
                        "Duplicate member '{clean_name}' inside tar archive (shadowing blocked)"
                    ));
                }

                let mut buf = Vec::new();
                let cap = MAX_BINARY_BYTES + 1;
                entry
                    .take(cap as u64)
                    .read_to_end(&mut buf)
                    .map_err(|e| format!("Failed decompressing '{clean_name}': {e}"))?;
                if buf.len() > MAX_BINARY_BYTES {
                    return Err(format!(
                        "Member '{clean_name}' exceeds maximum decompressed size (gzip bomb blocked)"
                    ));
                }
                total_decompressed = total_decompressed.saturating_add(buf.len());
                if total_decompressed > MAX_TOTAL_DECOMPRESSED_BYTES {
                    return Err(
                        "Total decompressed archive bytes exceeded safety limit".to_string()
                    );
                }

                if clean_name == expected_bin {
                    extracted_bin = Some(buf);
                }
            }
        }
        ArchiveFormat::Zip => {
            let mut zip = zip::ZipArchive::new(Cursor::new(archive_bytes))
                .map_err(|e| format!("Invalid zip archive: {e}"))?;
            if zip.is_empty() || zip.len() > MAX_ARCHIVE_ENTRIES {
                return Err(format!(
                    "Zip entry count ({}) is outside allowed bounds (1..={MAX_ARCHIVE_ENTRIES})",
                    zip.len()
                ));
            }
            for idx in 0..zip.len() {
                let file = zip
                    .by_index(idx)
                    .map_err(|e| format!("Corrupt zip entry #{idx}: {e}"))?;
                if file.is_dir() || file.is_symlink() {
                    return Err(
                        "Disallowed directory or symlink inside release zip archive".to_string()
                    );
                }
                let raw_name = file.name().to_string();
                let clean_name = validate_archive_member_name(&raw_name, expected_bin)?;

                if !seen_names.insert(clean_name.clone()) {
                    return Err(format!(
                        "Duplicate member '{clean_name}' inside zip archive (shadowing blocked)"
                    ));
                }

                let mut buf = Vec::new();
                let cap = MAX_BINARY_BYTES + 1;
                file.take(cap as u64)
                    .read_to_end(&mut buf)
                    .map_err(|e| format!("Failed decompressing '{clean_name}': {e}"))?;
                if buf.len() > MAX_BINARY_BYTES {
                    return Err(format!(
                        "Member '{clean_name}' exceeds maximum decompressed size (zip bomb blocked)"
                    ));
                }
                total_decompressed = total_decompressed.saturating_add(buf.len());
                if total_decompressed > MAX_TOTAL_DECOMPRESSED_BYTES {
                    return Err("Total decompressed zip bytes exceeded safety limit".to_string());
                }

                if clean_name == expected_bin {
                    extracted_bin = Some(buf);
                }
            }
        }
    }

    let bin = extracted_bin.ok_or_else(|| {
        format!("Release archive did not contain expected binary '{expected_bin}'")
    })?;
    verify_executable_header(&bin, target)?;
    Ok(bin)
}

#[derive(Debug, Clone)]
pub struct ReleaseAssetInfo {
    pub name: String,
    pub download_url: String,
    pub size: usize,
}

#[derive(Debug, Clone)]
pub struct ReleaseCheckOutcome {
    pub latest_tag: String,
    pub html_url: String,
    pub published_at: String,
    pub summary: String,
    pub update_available: bool,
    pub platform_asset: Option<ReleaseAssetInfo>,
    pub checksums_asset: Option<ReleaseAssetInfo>,
}

#[derive(Debug, Clone)]
pub struct VerifiedDownloadOutcome {
    pub tag: String,
    pub archive_name: String,
    pub sha256_hex: String,
    pub saved_path: PathBuf,
    pub binary_size_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct VerifiedInstallOutcome {
    pub tag: String,
    pub method: String,
    pub sha256_hex: String,
    pub installed_exe: PathBuf,
    pub binary_size_bytes: usize,
}

#[derive(Deserialize)]
struct GithubAssetDto {
    name: String,
    browser_download_url: String,
    size: u64,
}

#[derive(Deserialize)]
struct GithubReleaseDto {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<GithubAssetDto>,
}

/// Performs a bounded GET with manual redirect inspection (`redirects(0)`), enforcing `policy` at every hop.
fn fetch_bytes_hardened(
    initial_url: &str,
    accept_header: &str,
    max_bytes: usize,
    expected_exact_len: Option<usize>,
    policy: &EndpointPolicy,
) -> Result<Vec<u8>, String> {
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(20))
        .user_agent(&format!("lumaglyph/{APP_VERSION}"))
        .build();

    let mut current_url = initial_url.to_string();
    for hop in 0..=MAX_REDIRECT_HOPS {
        let mut req = agent.get(&current_url);
        if hop == 0 && !accept_header.is_empty() {
            req = req.set("Accept", accept_header);
        }

        match req.call() {
            Ok(resp) => {
                let status = resp.status();
                if matches!(status, 301 | 302 | 303 | 307 | 308) {
                    if hop == MAX_REDIRECT_HOPS {
                        return Err(format!(
                            "Exceeded maximum redirect hops ({MAX_REDIRECT_HOPS})"
                        ));
                    }
                    let loc = resp
                        .header("Location")
                        .ok_or_else(|| format!("HTTP {status} redirect missing Location header"))?
                        .trim()
                        .to_string();
                    validate_redirect_url(&loc, policy)?;
                    current_url = loc;
                    continue;
                }
                if status != 200 {
                    return Err(format!("Unexpected HTTP status {status}"));
                }

                if let Some(cl_str) = resp.header("Content-Length") {
                    if let Ok(cl) = cl_str.trim().parse::<usize>() {
                        if cl > max_bytes {
                            return Err(format!(
                                "Remote Content-Length ({cl} bytes) exceeds safety limit ({max_bytes} bytes)"
                            ));
                        }
                        if let Some(exp) = expected_exact_len {
                            if cl != exp {
                                return Err(format!(
                                    "Remote Content-Length ({cl}) does not match declared release asset size ({exp})"
                                ));
                            }
                        }
                    }
                }

                let mut buf = Vec::with_capacity(expected_exact_len.unwrap_or(8192).min(65536));
                resp.into_reader()
                    .take((max_bytes + 1) as u64)
                    .read_to_end(&mut buf)
                    .map_err(|e| format!("Network stream read error: {e}"))?;

                if buf.len() > max_bytes {
                    return Err(format!(
                        "Remote response exceeded maximum allowed size ({max_bytes} bytes)"
                    ));
                }
                if let Some(exp) = expected_exact_len {
                    if buf.len() != exp {
                        return Err(format!(
                            "Downloaded byte count ({}) does not match declared asset size ({exp}): truncated or tampered stream",
                            buf.len()
                        ));
                    }
                }
                return Ok(buf);
            }
            Err(ureq::Error::Status(404, _)) => {
                return Err("HTTP 404 Not Found on GitHub Releases endpoint".to_string());
            }
            Err(ureq::Error::Status(code, _)) => {
                return Err(format!("GitHub endpoint returned HTTP {code}"));
            }
            Err(other) => {
                return Err(format!("Network transport error: {other}"));
            }
        }
    }

    Err("Redirect loop terminated".to_string())
}

/// Asynchronous non-blocking GitHub release check spawned on a lightweight OS thread.
/// Opens zero network connections unless explicitly called.
pub fn spawn_release_check(repo: String) -> Receiver<Result<ReleaseCheckOutcome, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("lumaglyph-release-check".to_string())
        .spawn(move || {
            let res = check_latest_release_blocking(&repo);
            let _ = tx.send(res);
        })
        .ok();
    rx
}

/// Asynchronous non-blocking verified binary update download spawned on a background thread.
pub fn spawn_verified_release_download(
    repo: String,
    dest_dir: PathBuf,
) -> Receiver<Result<VerifiedDownloadOutcome, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("lumaglyph-verified-download".to_string())
        .spawn(move || {
            let Some(target) = PlatformTarget::current() else {
                let _ = tx.send(Err(
                    "Unsupported OS/CPU architecture for prebuilt binary".to_string()
                ));
                return;
            };
            let policy = EndpointPolicy::production_github(false);
            let res = download_and_verify_release_with_policy(
                &repo,
                APP_VERSION,
                target,
                &dest_dir,
                &policy,
            );
            let _ = tx.send(res);
        })
        .ok();
    rx
}

pub fn check_latest_release_blocking(repo: &str) -> Result<ReleaseCheckOutcome, String> {
    let policy = EndpointPolicy::production_github(false);
    check_latest_release_with_policy(repo, APP_VERSION, PlatformTarget::current(), &policy)
}

/// Testable policy-driven release discovery with full URL, tag, draft/prerelease, and asset validation.
pub fn check_latest_release_with_policy(
    repo: &str,
    current_version: &str,
    platform: Option<PlatformTarget>,
    policy: &EndpointPolicy,
) -> Result<ReleaseCheckOutcome, String> {
    let (owner, name) = validate_repo_slug(repo, policy.require_canonical_repo)?;
    let api_url = format!(
        "{}/repos/{owner}/{name}/releases/latest",
        policy.api_base.trim_end_matches('/')
    );
    let api_parsed = ParsedUrl::parse_strict(&api_url)?;
    api_parsed.verify_transport(policy)?;

    let body_bytes = fetch_bytes_hardened(
        &api_url,
        "application/vnd.github+json",
        MAX_API_JSON_BYTES,
        None,
        policy,
    )?;

    let dto: GithubReleaseDto = serde_json::from_slice(&body_bytes)
        .map_err(|e| format!("Invalid GitHub release JSON: {e}"))?;

    if dto.draft {
        return Err("Rejected draft GitHub release".to_string());
    }
    if dto.prerelease {
        return Err("Rejected prerelease GitHub tag".to_string());
    }

    let tag = validate_release_tag(&dto.tag_name)?.to_string();
    validate_release_html_url(&dto.html_url, owner, name, &tag, policy)?;

    let mut platform_asset = None;
    let mut checksums_asset = None;

    let expected_arch_name = platform.map(|p| p.expected_archive_name());
    for asset in &dto.assets {
        validate_flat_filename(&asset.name)?;
        if Some(asset.name.as_str()) == expected_arch_name {
            if asset.size < (MIN_ARCHIVE_BYTES as u64) || asset.size > (MAX_ARCHIVE_BYTES as u64) {
                return Err(format!(
                    "Release asset '{}' has invalid size ({} bytes)",
                    asset.name, asset.size
                ));
            }
            validate_asset_download_url(
                &asset.browser_download_url,
                owner,
                name,
                &tag,
                &asset.name,
                policy,
            )?;
            if platform_asset.is_some() {
                return Err(format!("Duplicate release asset '{}'", asset.name));
            }
            platform_asset = Some(ReleaseAssetInfo {
                name: asset.name.clone(),
                download_url: asset.browser_download_url.clone(),
                size: asset.size as usize,
            });
        } else if asset.name == "SHA256SUMS.txt" {
            if asset.size == 0 || asset.size > (MAX_CHECKSUM_FILE_BYTES as u64) {
                return Err(format!(
                    "SHA256SUMS.txt asset has invalid size ({} bytes)",
                    asset.size
                ));
            }
            validate_asset_download_url(
                &asset.browser_download_url,
                owner,
                name,
                &tag,
                "SHA256SUMS.txt",
                policy,
            )?;
            if checksums_asset.is_some() {
                return Err("Duplicate SHA256SUMS.txt asset in release".to_string());
            }
            checksums_asset = Some(ReleaseAssetInfo {
                name: asset.name.clone(),
                download_url: asset.browser_download_url.clone(),
                size: asset.size as usize,
            });
        }
    }

    let update_available = is_newer_semver(&tag, current_version);
    let summary = dto
        .body
        .unwrap_or_default()
        .lines()
        .take(3)
        .map(|l| {
            l.chars()
                .filter(|c| !c.is_control())
                .take(160)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ");

    Ok(ReleaseCheckOutcome {
        latest_tag: tag,
        html_url: dto.html_url,
        published_at: dto.published_at.unwrap_or_default(),
        summary,
        update_available,
        platform_asset,
        checksums_asset,
    })
}

/// Downloads, cryptographically verifies (`SHA256SUMS.txt` in constant time), safely extracts in memory,
/// verifies the OS/CPU machine-code executable header, and atomically writes the new binary to `dest_dir`.
pub fn download_and_verify_release_blocking(
    repo: &str,
    dest_dir: &Path,
    require_canonical_repo: bool,
) -> Result<VerifiedDownloadOutcome, String> {
    let target = PlatformTarget::current().ok_or_else(|| {
        "Unsupported OS/CPU architecture for prebuilt binary download".to_string()
    })?;
    let policy = EndpointPolicy::production_github(require_canonical_repo);
    download_and_verify_release_with_policy(repo, APP_VERSION, target, dest_dir, &policy)
}

/// Full 8-layer verified update download pipeline.
pub fn download_and_verify_release_with_policy(
    repo: &str,
    current_version: &str,
    target: PlatformTarget,
    dest_dir: &Path,
    policy: &EndpointPolicy,
) -> Result<VerifiedDownloadOutcome, String> {
    let check = check_latest_release_with_policy(repo, current_version, Some(target), policy)?;

    if !check.update_available {
        return Err(format!(
            "Refusing update download: remote release '{}' is not newer than installed v{current_version}",
            check.latest_tag
        ));
    }

    let archive_asset = check.platform_asset.ok_or_else(|| {
        format!(
            "Release '{}' does not publish expected asset '{}'",
            check.latest_tag,
            target.expected_archive_name()
        )
    })?;
    let sums_asset = check.checksums_asset.ok_or_else(|| {
        format!(
            "Release '{}' is missing mandatory SHA256SUMS.txt manifest",
            check.latest_tag
        )
    })?;

    // Step 1: Download & parse SHA256SUMS.txt
    let sums_bytes = fetch_bytes_hardened(
        &sums_asset.download_url,
        "application/octet-stream",
        MAX_CHECKSUM_FILE_BYTES,
        Some(sums_asset.size),
        policy,
    )?;
    let sums_text = std::str::from_utf8(&sums_bytes)
        .map_err(|_| "SHA256SUMS.txt is not valid UTF-8 text".to_string())?;
    let sums_map = parse_sha256sums(sums_text)?;
    let expected_digest = sums_map.get(&archive_asset.name).ok_or_else(|| {
        format!(
            "SHA256SUMS.txt does not contain a checksum entry for '{}'",
            archive_asset.name
        )
    })?;

    // Step 2: Download archive with exact byte-size enforcement
    let archive_bytes = fetch_bytes_hardened(
        &archive_asset.download_url,
        "application/octet-stream",
        MAX_ARCHIVE_BYTES,
        Some(archive_asset.size),
        policy,
    )?;

    // Step 3: Verify SHA-256 in constant time BEFORE touching any archive decompressor
    let actual_digest = sha256_bytes(&archive_bytes);
    if !constant_time_eq_32(&actual_digest, expected_digest) {
        return Err(format!(
            "CRITICAL: SHA-256 checksum mismatch for '{}' (expected {}, got {}) — download aborted",
            archive_asset.name,
            hex_encode_32(expected_digest),
            hex_encode_32(&actual_digest)
        ));
    }

    // Step 4: Safe bounded in-memory extraction + OS/CPU executable header verification
    let verified_bin = extract_verified_binary_from_archive(&archive_bytes, target)?;

    // Step 5: Atomic temp-file write + fsync + rename inside dest_dir
    fs::create_dir_all(dest_dir).map_err(|e| {
        format!(
            "Failed creating destination directory '{}': {e}",
            dest_dir.display()
        )
    })?;

    let final_filename = target.output_filename_for_tag(&check.latest_tag);
    validate_flat_filename(&final_filename)?;
    let final_path = dest_dir.join(&final_filename);

    let nonce = TMP_NONCE.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let tmp_path = dest_dir.join(format!(".{final_filename}.part.{pid}.{nonce}"));

    let write_res = (|| -> Result<(), String> {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
            .map_err(|e| format!("Failed creating temporary download file: {e}"))?;
        f.write_all(&verified_bin)
            .map_err(|e| format!("Failed writing verified binary: {e}"))?;
        f.sync_all()
            .map_err(|e| format!("Failed syncing verified binary to disk: {e}"))?;
        drop(f);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o755))
                .map_err(|e| format!("Failed setting executable permissions: {e}"))?;
        }

        fs::rename(&tmp_path, &final_path).map_err(|e| {
            format!(
                "Failed finalizing verified binary '{}': {e}",
                final_path.display()
            )
        })?;
        Ok(())
    })();

    if let Err(e) = write_res {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }

    Ok(VerifiedDownloadOutcome {
        tag: check.latest_tag,
        archive_name: archive_asset.name,
        sha256_hex: hex_encode_32(&actual_digest),
        saved_path: final_path,
        binary_size_bytes: verified_bin.len(),
    })
}

/// Cleans up stale `<exe>.old` and `<exe>.old.<pid>.<nonce>` backup binaries left behind
/// by a previous in-place self-update (notably on Windows where a running `.exe` can be renamed
/// to `.old` while running, and then deleted once the newly launched process starts).
pub fn cleanup_stale_old_executables() -> usize {
    let Ok(exe) = std::env::current_exe() else {
        return 0;
    };
    let (Some(dir), Some(name)) = (exe.parent(), exe.file_name().and_then(|s| s.to_str())) else {
        return 0;
    };
    cleanup_stale_old_executables_in_dir(dir, name)
}

pub fn cleanup_stale_old_executables_in_dir(dir: &Path, exe_name: &str) -> usize {
    if exe_name.is_empty() || exe_name.contains('/') || exe_name.contains('\\') {
        return 0;
    }
    let exact_old = format!("{exe_name}.old");
    let prefix_old = format!("{exe_name}.old.");
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0usize;
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if !ft.is_file() {
            continue;
        }
        let Some(fname) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let is_numbered_old = fname.strip_prefix(&prefix_old).is_some_and(|rest| {
            !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        });
        if (fname == exact_old || is_numbered_old) && fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Atomically installs a verified native executable onto `target_exe` using a sibling staging file
/// and safe `.old` rename swap (supporting running-executable replacement on Windows, macOS, and Linux)
/// with automatic rollback if final placement fails.
pub fn apply_verified_binary_to_path(
    target_exe: &Path,
    verified_bin: &[u8],
    target: PlatformTarget,
) -> Result<PathBuf, String> {
    verify_executable_header(verified_bin, target)?;

    let parent = target_exe
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let exe_name = target_exe
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Invalid target executable path".to_string())?;

    fs::create_dir_all(parent).map_err(|e| {
        format!(
            "Failed accessing target executable directory '{}': {e}",
            parent.display()
        )
    })?;

    let nonce = TMP_NONCE.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let stage_path = parent.join(format!(".{exe_name}.update.{pid}.{nonce}"));

    let stage_res = (|| -> Result<(), String> {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stage_path)
            .map_err(|e| format!("Failed creating staged update binary: {e}"))?;
        f.write_all(verified_bin)
            .map_err(|e| format!("Failed writing staged update binary: {e}"))?;
        f.sync_all()
            .map_err(|e| format!("Failed syncing staged update binary: {e}"))?;
        drop(f);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&stage_path, fs::Permissions::from_mode(0o755))
                .map_err(|e| format!("Failed setting executable permissions: {e}"))?;
        }
        Ok(())
    })();

    if let Err(e) = stage_res {
        let _ = fs::remove_file(&stage_path);
        return Err(e);
    }

    let primary_old = parent.join(format!("{exe_name}.old"));
    if primary_old.exists() {
        let _ = fs::remove_file(&primary_old);
    }
    let backup_path = if primary_old.exists() {
        parent.join(format!("{exe_name}.old.{pid}.{nonce}"))
    } else {
        primary_old
    };

    let had_existing = target_exe.exists();
    if had_existing {
        if let Err(e) = fs::rename(target_exe, &backup_path) {
            let _ = fs::remove_file(&stage_path);
            return Err(format!(
                "Failed moving existing executable '{}' aside for update: {e}",
                target_exe.display()
            ));
        }
    }

    if let Err(e) = fs::rename(&stage_path, target_exe) {
        if had_existing {
            let _ = fs::rename(&backup_path, target_exe);
        }
        let _ = fs::remove_file(&stage_path);
        return Err(format!(
            "Failed installing new release binary at '{}': {e}",
            target_exe.display()
        ));
    }

    if had_existing {
        // Succeeds immediately on Unix; on Windows if locked by running image, cleaned up on relaunch startup.
        let _ = fs::remove_file(&backup_path);
    }

    Ok(target_exe.to_path_buf())
}

/// Full 8-layer verified release download + atomic in-place executable replacement.
pub fn install_verified_release_with_policy(
    repo: &str,
    current_version: &str,
    target: PlatformTarget,
    target_exe: &Path,
    policy: &EndpointPolicy,
) -> Result<VerifiedInstallOutcome, String> {
    let check = check_latest_release_with_policy(repo, current_version, Some(target), policy)?;

    if !check.update_available {
        return Err(format!(
            "Refusing update install: remote release '{}' is not newer than installed v{current_version}",
            check.latest_tag
        ));
    }

    let archive_asset = check.platform_asset.ok_or_else(|| {
        format!(
            "Release '{}' does not publish expected asset '{}'",
            check.latest_tag,
            target.expected_archive_name()
        )
    })?;
    let sums_asset = check.checksums_asset.ok_or_else(|| {
        format!(
            "Release '{}' is missing mandatory SHA256SUMS.txt manifest",
            check.latest_tag
        )
    })?;

    let sums_bytes = fetch_bytes_hardened(
        &sums_asset.download_url,
        "application/octet-stream",
        MAX_CHECKSUM_FILE_BYTES,
        Some(sums_asset.size),
        policy,
    )?;
    let sums_text = std::str::from_utf8(&sums_bytes)
        .map_err(|_| "SHA256SUMS.txt is not valid UTF-8 text".to_string())?;
    let sums_map = parse_sha256sums(sums_text)?;
    let expected_digest = sums_map.get(&archive_asset.name).ok_or_else(|| {
        format!(
            "SHA256SUMS.txt does not contain a checksum entry for '{}'",
            archive_asset.name
        )
    })?;

    let archive_bytes = fetch_bytes_hardened(
        &archive_asset.download_url,
        "application/octet-stream",
        MAX_ARCHIVE_BYTES,
        Some(archive_asset.size),
        policy,
    )?;

    let actual_digest = sha256_bytes(&archive_bytes);
    if !constant_time_eq_32(&actual_digest, expected_digest) {
        return Err(format!(
            "CRITICAL: SHA-256 checksum mismatch for '{}' (expected {}, got {}) — update aborted",
            archive_asset.name,
            hex_encode_32(expected_digest),
            hex_encode_32(&actual_digest)
        ));
    }

    let verified_bin = extract_verified_binary_from_archive(&archive_bytes, target)?;
    let installed_exe = apply_verified_binary_to_path(target_exe, &verified_bin, target)?;

    Ok(VerifiedInstallOutcome {
        tag: check.latest_tag,
        method: format!("verified-release-binary ({})", archive_asset.name),
        sha256_hex: hex_encode_32(&actual_digest),
        installed_exe,
        binary_size_bytes: verified_bin.len(),
    })
}

/// Detects if `exe_path` resides inside a local `lumaglyph` Cargo + Git checkout
/// (e.g. `<repo>/target/release/lumaglyph[.exe]`).
pub fn detect_local_cargo_checkout(exe_path: &Path) -> Option<PathBuf> {
    let mut cursor = exe_path.parent()?;
    for _ in 0..5 {
        let cargo_toml = cursor.join("Cargo.toml");
        let git_dir = cursor.join(".git");
        if cargo_toml.is_file() && git_dir.exists() {
            if let Ok(manifest) = fs::read_to_string(&cargo_toml) {
                if manifest.contains("name = \"lumaglyph\"") {
                    return Some(cursor.to_path_buf());
                }
            }
        }
        cursor = cursor.parent()?;
    }
    None
}

fn configure_hidden_subprocess(cmd: &mut std::process::Command) {
    cmd.stdin(std::process::Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
}

fn resolve_cargo_program() -> PathBuf {
    if let Some(c) = std::env::var_os("CARGO") {
        let p = PathBuf::from(c);
        if p.is_file() {
            return p;
        }
    }
    let bin_name = if cfg!(target_os = "windows") {
        "cargo.exe"
    } else {
        "cargo"
    };
    if let Some(ch) = std::env::var_os("CARGO_HOME") {
        let p = PathBuf::from(ch).join("bin").join(bin_name);
        if p.is_file() {
            return p;
        }
    }
    if let Some(home) = dirs::home_dir() {
        let p = home.join(".cargo").join("bin").join(bin_name);
        if p.is_file() {
            return p;
        }
    }
    PathBuf::from(bin_name)
}

/// Fetches the verified release tag from the canonical GitHub repository inside a local Cargo checkout,
/// rebuilds `lumaglyph` in `--release --locked` mode with zero console window popups (moving any running
/// Windows `lumaglyph.exe` aside to `.old` prior to linking so `link.exe` never hits `LNK1104`), verifies
/// the compiled native binary header, and updates `target_exe`.
pub fn rebuild_from_verified_release_tag(
    repo: &str,
    tag: &str,
    repo_root: &Path,
    target_exe: &Path,
    require_canonical_repo: bool,
) -> Result<VerifiedInstallOutcome, String> {
    let (owner, name) = validate_repo_slug(repo, require_canonical_repo)?;
    let clean_tag = validate_release_tag(tag)?;
    let remote_url = format!("https://github.com/{owner}/{name}.git");
    let refspec = format!("refs/tags/{clean_tag}:refs/tags/{clean_tag}");

    // Ensure clean working tree before touching checkout state
    let mut status_cmd = std::process::Command::new("git");
    configure_hidden_subprocess(&mut status_cmd);
    let status_out = status_cmd
        .current_dir(repo_root)
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .map_err(|e| format!("git unavailable for source rebuild: {e}"))?;
    if !status_out.status.success() || !status_out.stdout.is_empty() {
        return Err(
            "Local git checkout has uncommitted changes; skipping source rebuild".to_string(),
        );
    }

    let mut fetch_cmd = std::process::Command::new("git");
    configure_hidden_subprocess(&mut fetch_cmd);
    let fetch_out = fetch_cmd
        .current_dir(repo_root)
        .args(["fetch", "--depth", "1", "--force", &remote_url, &refspec])
        .output()
        .map_err(|e| format!("Failed running git fetch: {e}"))?;
    if !fetch_out.status.success() {
        return Err(format!(
            "git fetch failed: {}",
            String::from_utf8_lossy(&fetch_out.stderr).trim()
        ));
    }

    let mut checkout_cmd = std::process::Command::new("git");
    configure_hidden_subprocess(&mut checkout_cmd);
    let checkout_out = checkout_cmd
        .current_dir(repo_root)
        .args(["checkout", "--quiet", clean_tag])
        .output()
        .map_err(|e| format!("Failed checking out release tag '{clean_tag}': {e}"))?;
    if !checkout_out.status.success() {
        return Err(format!(
            "git checkout '{clean_tag}' failed: {}",
            String::from_utf8_lossy(&checkout_out.stderr).trim()
        ));
    }

    let bin_name = if cfg!(target_os = "windows") {
        "lumaglyph.exe"
    } else {
        "lumaglyph"
    };
    let built_path = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root.join("target"))
        .join("release")
        .join(bin_name);

    // On Windows, if the running executable IS `target\release\lumaglyph.exe`, rename it to `.old`
    // before invoking `cargo build --release` so MSVC `link.exe` can write the new `.exe` cleanly.
    let mut moved_running_backup: Option<PathBuf> = None;
    if built_path == target_exe && target_exe.exists() {
        let parent = target_exe.parent().unwrap_or_else(|| Path::new("."));
        let nonce = TMP_NONCE.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let primary_old = parent.join(format!("{bin_name}.old"));
        if primary_old.exists() {
            let _ = fs::remove_file(&primary_old);
        }
        let backup_path = if primary_old.exists() {
            parent.join(format!("{bin_name}.old.{pid}.{nonce}"))
        } else {
            primary_old
        };
        if fs::rename(target_exe, &backup_path).is_ok() {
            moved_running_backup = Some(backup_path);
        }
    }

    let mut cargo_cmd = std::process::Command::new(resolve_cargo_program());
    configure_hidden_subprocess(&mut cargo_cmd);
    let build_res = cargo_cmd
        .current_dir(repo_root)
        .args(["build", "--release", "--locked"])
        .output();

    let build_out = match build_res {
        Ok(out) if out.status.success() => out,
        Ok(out) => {
            if let Some(bak) = &moved_running_backup {
                let _ = fs::rename(bak, target_exe);
            }
            return Err(format!(
                "cargo build --release --locked failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Err(e) => {
            if let Some(bak) = &moved_running_backup {
                let _ = fs::rename(bak, target_exe);
            }
            return Err(format!("Failed invoking cargo build: {e}"));
        }
    };
    let _ = build_out;

    let built_bytes = match fs::read(&built_path) {
        Ok(b) => b,
        Err(e) => {
            if let Some(bak) = &moved_running_backup {
                let _ = fs::rename(bak, target_exe);
            }
            return Err(format!(
                "Failed reading rebuilt release binary '{}': {e}",
                built_path.display()
            ));
        }
    };
    let digest = sha256_bytes(&built_bytes);

    if let Some(target) = PlatformTarget::current() {
        if built_path != target_exe {
            apply_verified_binary_to_path(target_exe, &built_bytes, target)?;
        } else {
            verify_executable_header(&built_bytes, target)?;
        }
    }

    if let Some(bak) = &moved_running_backup {
        let _ = fs::remove_file(bak);
    }

    let final_exe = if built_path == target_exe {
        built_path
    } else {
        target_exe.to_path_buf()
    };

    Ok(VerifiedInstallOutcome {
        tag: clean_tag.to_string(),
        method: "cargo-release-rebuild".to_string(),
        sha256_hex: hex_encode_32(&digest),
        installed_exe: final_exe,
        binary_size_bytes: built_bytes.len(),
    })
}

/// Checks for a newer official GitHub release, rebuilds from source if running inside a clean local
/// Cargo/Git checkout (or when `force_source_rebuild` is set) and seamlessly installs the SHA-256-verified
/// prebuilt release binary onto `current_exe()` otherwise, ready for immediate automatic relaunch.
pub fn update_and_prepare_relaunch_blocking(
    repo: &str,
    force_source_rebuild: bool,
    require_canonical_repo: bool,
) -> Result<VerifiedInstallOutcome, String> {
    let current_exe = std::env::current_exe()
        .map_err(|e| format!("Failed resolving current executable path: {e}"))?;
    let policy = EndpointPolicy::production_github(require_canonical_repo);
    let check =
        check_latest_release_with_policy(repo, APP_VERSION, PlatformTarget::current(), &policy)?;

    if !check.update_available {
        return Err(format!(
            "Already up to date (installed v{APP_VERSION}, latest release {})",
            check.latest_tag
        ));
    }

    if let Some(repo_root) = detect_local_cargo_checkout(&current_exe) {
        match rebuild_from_verified_release_tag(
            repo,
            &check.latest_tag,
            &repo_root,
            &current_exe,
            require_canonical_repo,
        ) {
            Ok(outcome) => return Ok(outcome),
            Err(err) if force_source_rebuild => return Err(err),
            Err(_) => {
                // Fall through seamlessly to verified prebuilt GitHub release binary installation
            }
        }
    } else if force_source_rebuild {
        return Err(
            "Current executable is not inside a local LumaGlyph Cargo/Git workspace".to_string(),
        );
    }

    let target = PlatformTarget::current()
        .ok_or_else(|| "Unsupported OS/CPU architecture for prebuilt binary update".to_string())?;
    install_verified_release_with_policy(repo, APP_VERSION, target, &current_exe, &policy)
}

/// Background worker thread that installs/rebuilds the latest verified GitHub release onto `current_exe()`.
pub fn spawn_verified_self_update(
    repo: String,
) -> Receiver<Result<VerifiedInstallOutcome, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("lumaglyph-self-update".to_string())
        .spawn(move || {
            let res = update_and_prepare_relaunch_blocking(&repo, false, false);
            let _ = tx.send(res);
        })
        .ok();
    rx
}

/// Spawns the newly updated executable (`exe_path`) as an independent process so the current
/// process can exit immediately and hand off seamlessly to the new release.
pub fn relaunch_executable(exe_path: &Path, args: &[String]) -> Result<u32, String> {
    if !exe_path.is_file() {
        return Err(format!(
            "Updated executable '{}' does not exist",
            exe_path.display()
        ));
    }
    let mut cmd = std::process::Command::new(exe_path);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    let child = cmd.spawn().map_err(|e| {
        format!(
            "Failed launching updated executable '{}': {e}",
            exe_path.display()
        )
    })?;
    Ok(child.id())
}

/// Clean strict SemVer comparison (`v0.3.1` > `0.3.0`) with zero external crate overhead.
pub fn is_newer_semver(remote_tag: &str, current_version: &str) -> bool {
    let parse_triple = |s: &str| -> Option<(u32, u32, u32)> {
        let clean = s.trim();
        let body = clean
            .strip_prefix('v')
            .or_else(|| clean.strip_prefix('V'))
            .unwrap_or(clean);
        let core = body.split(['-', '+']).next().unwrap_or(body);
        let mut parts = core.split('.');
        let major = parts.next()?.parse::<u32>().ok()?;
        let minor = parts.next()?.parse::<u32>().ok()?;
        let patch = parts.next()?.parse::<u32>().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some((major, minor, patch))
    };
    match (parse_triple(remote_tag), parse_triple(current_version)) {
        (Some(rem), Some(cur)) => rem > cur,
        _ => false,
    }
}
