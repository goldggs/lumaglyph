use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

/// Default canonical GitHub repository slug (`owner/repo`) used ONLY when the user opts in
/// or explicitly clicks "Check for Updates". Override at compile time via `LUMAGLYPH_GITHUB_REPO`
/// or in user settings.
pub const DEFAULT_GITHUB_REPO: &str = match option_env!("LUMAGLYPH_GITHUB_REPO") {
    Some(repo) => repo,
    None => "goldggs/lumaglyph",
};

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Persistent user preferences stored locally.
/// Strictly 100% offline by default (`check_updates_on_startup = false`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppPreferences {
    pub check_updates_on_startup: bool,
    pub github_repo: String,
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            check_updates_on_startup: false,
            github_repo: DEFAULT_GITHUB_REPO.to_string(),
        }
    }
}

impl AppPreferences {
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
}

#[derive(Debug, Clone)]
pub struct ReleaseCheckOutcome {
    pub latest_tag: String,
    pub html_url: String,
    pub published_at: String,
    pub summary: String,
    pub update_available: bool,
}

#[derive(Deserialize)]
struct GithubReleaseDto {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    body: Option<String>,
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

pub fn check_latest_release_blocking(repo: &str) -> Result<ReleaseCheckOutcome, String> {
    let trimmed = repo.trim();
    if trimmed.is_empty() || trimmed.starts_with("YOUR_GITHUB_USERNAME/") {
        return Err(
            "Repository slug is still set to placeholder (YOUR_GITHUB_USERNAME/lumaglyph)."
                .to_string(),
        );
    }

    let url = format!("https://api.github.com/repos/{trimmed}/releases/latest");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(4))
        .timeout_read(Duration::from_secs(5))
        .user_agent(&format!("lumaglyph/{APP_VERSION}"))
        .build();

    let resp = agent
        .get(&url)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(404, _) => {
                format!("No published GitHub releases found yet for {trimmed}")
            }
            other => format!("GitHub release check failed: {other}"),
        })?;

    let dto: GithubReleaseDto = resp
        .into_json()
        .map_err(|e| format!("Invalid GitHub release JSON: {e}"))?;

    let update_available = is_newer_semver(&dto.tag_name, APP_VERSION);
    let summary = dto
        .body
        .unwrap_or_default()
        .lines()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ");

    Ok(ReleaseCheckOutcome {
        latest_tag: dto.tag_name,
        html_url: dto.html_url,
        published_at: dto.published_at.unwrap_or_default(),
        summary,
        update_available,
    })
}

/// Clean semver comparison (`v0.2.0` > `0.1.0`) with zero external crate overhead.
pub fn is_newer_semver(remote_tag: &str, current_version: &str) -> bool {
    let parse_triple = |s: &str| -> (u32, u32, u32) {
        let clean = s.trim().trim_start_matches(['v', 'V']);
        let core = clean.split(['-', '+']).next().unwrap_or(clean);
        let mut parts = core.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
        (
            parts.next().unwrap_or(0),
            parts.next().unwrap_or(0),
            parts.next().unwrap_or(0),
        )
    };
    parse_triple(remote_tag) > parse_triple(current_version)
}
