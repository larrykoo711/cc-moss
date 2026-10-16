//! Is there a newer cc-moss? One HEAD request to the GitHub releases page,
//! at most once a day, remembered in `<state dir>/update.json`.
//!
//! The request asks `…/releases/latest` where it redirects and reads the tag
//! from the `location:` header: no API, no rate limit, and nothing about the
//! machine is sent. Only the picker, `doctor` and `update` call this; the
//! hooks and scripted commands (`ls --json`) never do.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::Config;

/// Where releases live. `$CC_MOSS_RELEASES_URL` overrides it in debug builds (tests).
pub const DEFAULT_RELEASES_URL: &str = "https://github.com/larrykoo711/cc-moss/releases";
/// How long a check, successful or not, is good for.
pub const CACHE_TTL_SECS: i64 = 24 * 60 * 60;
/// The background check gives up after this.
pub const QUICK_TIMEOUT: Duration = Duration::from_secs(3);

/// A release version, `major.minor.patch`. Pre-releases are not parsed, so
/// they never count as a new version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
}

impl Version {
    /// `1.2.3` or `v1.2.3`; `None` for anything else, `1.2.3-rc.1` included.
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = text.split('.');
        let mut next = || -> Option<u64> {
            let p = parts.next()?;
            if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            p.parse().ok()
        };
        let v = Version {
            major: next()?,
            minor: next()?,
            patch: next()?,
        };
        parts.next().is_none().then_some(v)
    }

    /// The version of this build.
    pub fn current() -> Version {
        // A pre-release build (`0.2.0-rc.1`) counts as its release number.
        let v = env!("CARGO_PKG_VERSION");
        Version::parse(v.split('-').next().unwrap_or(v)).unwrap_or(Version {
            major: 0,
            minor: 0,
            patch: 0,
        })
    }

    pub fn is_newer_than(&self, other: &Version) -> bool {
        self > other
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The release tag in the `location:` header of the `…/releases/latest`
/// redirect. Header names are matched case-insensitively.
pub fn latest_from_headers(headers: &str) -> Option<Version> {
    headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if !name.trim().eq_ignore_ascii_case("location") {
            return None;
        }
        let value = value.trim().trim_end_matches('/');
        let (_, tag) = value.rsplit_once("/tag/")?;
        Version::parse(tag)
    })
}

/// `$CC_MOSS_RELEASES_URL`, else the GitHub releases page.
pub fn releases_url() -> String {
    // Debug builds (the tests) only: a release build always asks GitHub.
    std::env::var("CC_MOSS_RELEASES_URL")
        .ok()
        .filter(|_| cfg!(debug_assertions))
        .map(|u| u.trim_end_matches('/').to_string())
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| DEFAULT_RELEASES_URL.to_string())
}

/// Whether the picker and `doctor` may look for a new version: on, unless
/// `[update] check = false` or `CC_MOSS_NO_UPDATE_CHECK` is set (and not `0`).
pub fn enabled(config: &Config, env: &dyn Fn(&str) -> Option<String>) -> bool {
    let off_by_env = env("CC_MOSS_NO_UPDATE_CHECK").is_some_and(|v| !v.is_empty() && v != "0");
    config.update.check && !off_by_env
}

/// [`enabled`] for this process.
pub fn enabled_here(config: &Config) -> bool {
    enabled(config, &|k| std::env::var(k).ok())
}

/// The last check: when, and the newest version known then.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cache {
    /// Unix seconds.
    pub checked_at: i64,
    pub latest: Option<String>,
}

impl Cache {
    pub fn path() -> PathBuf {
        crate::store_dir::state_dir().join("update.json")
    }

    /// `None` when there is no cache or it cannot be read.
    pub fn load(path: &Path) -> Option<Cache> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Write through a temporary file, so a reader never sees half of it.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            crate::store_dir::ensure_private_dir(dir)?;
        }
        let tmp = path.with_extension(format!("json.{}", std::process::id()));
        std::fs::write(&tmp, serde_json::to_string(self)?)?;
        std::fs::rename(&tmp, path).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
    }

    pub fn is_fresh(&self, now: i64) -> bool {
        (self.checked_at..self.checked_at.saturating_add(CACHE_TTL_SECS)).contains(&now)
    }

    pub fn latest_version(&self) -> Option<Version> {
        self.latest.as_deref().and_then(Version::parse)
    }
}

/// Seconds since the epoch.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Why a check found no version.
#[derive(Debug)]
pub enum CheckError {
    /// `curl` is not installed.
    NoCurl,
    /// No answer in time.
    TimedOut,
    /// curl failed (offline, DNS, TLS, HTTP error); its message.
    Failed(String),
    /// The answer had no release tag (no release yet, or a pre-release).
    NoRelease,
}

impl std::fmt::Display for CheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckError::NoCurl => write!(f, "curl is not installed"),
            CheckError::TimedOut => write!(f, "no answer in time"),
            CheckError::Failed(e) => write!(f, "{e}"),
            CheckError::NoRelease => write!(f, "no release found"),
        }
    }
}

/// Ask the releases page for the latest version, now.
pub fn fetch_latest(timeout: Duration) -> Result<Version, CheckError> {
    let url = format!("{}/latest", releases_url());
    let mut cmd = Command::new("curl");
    cmd.args(["-fsS", "-I", "--proto", "=https", "--tlsv1.2", "--max-time"])
        .arg(timeout.as_secs().max(1).to_string())
        .arg(&url);
    // A second more than curl's own limit, for a curl that ignores it.
    let out = crate::command::run_with_timeout(&mut cmd, timeout + Duration::from_secs(1))
        .map_err(|e| match e {
            crate::command::RunError::NotStarted(_) if e.is_not_installed() => CheckError::NoCurl,
            crate::command::RunError::NotStarted(e) => CheckError::Failed(e.to_string()),
            crate::command::RunError::TimedOut => CheckError::TimedOut,
        })?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(CheckError::Failed(if err.is_empty() {
            format!("curl {}", out.status)
        } else {
            err
        }));
    }
    latest_from_headers(&String::from_utf8_lossy(&out.stdout)).ok_or(CheckError::NoRelease)
}

/// Check now and remember the answer. A failed check is remembered too (with
/// the version known before), so an offline machine does not retry all day.
pub fn check_and_cache(timeout: Duration) -> Result<Version, CheckError> {
    let path = Cache::path();
    let result = fetch_latest(timeout);
    let latest = match &result {
        Ok(v) => Some(v.to_string()),
        Err(_) => Cache::load(&path).and_then(|c| c.latest),
    };
    let _ = Cache {
        checked_at: now_secs(),
        latest,
    }
    .save(&path);
    result
}

/// The newest version known without asking: the cache, fresh or not.
pub fn cached_latest() -> Option<Version> {
    Cache::load(&Cache::path()).and_then(|c| c.latest_version())
}

/// Whether the cache is missing or older than a day.
pub fn cache_is_stale() -> bool {
    Cache::load(&Cache::path()).is_none_or(|c| !c.is_fresh(now_secs()))
}

/// The newest version known, checking first when the cache is stale.
pub fn latest_checking_if_stale(timeout: Duration) -> Option<Version> {
    if cache_is_stale() {
        let _ = check_and_cache(timeout);
    }
    cached_latest()
}

/// `latest` if it is newer than this build.
pub fn newer(latest: Option<Version>) -> Option<Version> {
    latest.filter(|v| v.is_newer_than(&Version::current()))
}
