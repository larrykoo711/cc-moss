//! A fake GitHub releases page: a stub `curl` on PATH serves fixtures from
//! a folder and logs every call; stub `brew` and `cargo` log theirs.

#![allow(dead_code)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Releases are served from here; the stub never looks at the host.
pub const RELEASES: &str = "https://releases.invalid/acme/cc-moss/releases";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// Answers `curl -I <url>` with `<files>/<last path part>.headers` and
/// `curl -o <out> <url>` with a copy of `<files>/<last path part>`; anything
/// missing fails like a network error.
pub const CURL_STUB: &str = r#"#!/bin/sh
echo "$*" >> "$STUB_LOG_DIR/curl.log"
head=""; out=""; url=""
while [ $# -gt 0 ]; do
  case "$1" in
    -I) head=1 ;;
    -o) out="$2"; shift ;;
    https://*) url="$1" ;;
  esac
  shift
done
name="${url##*/}"
if [ -n "$head" ]; then
  [ -f "$CURL_STUB_DIR/$name.headers" ] || { echo "curl: (6) Could not resolve host" >&2; exit 6; }
  cat "$CURL_STUB_DIR/$name.headers"
else
  [ -f "$CURL_STUB_DIR/$name" ] || { echo "curl: (22) 404" >&2; exit 22; }
  cp "$CURL_STUB_DIR/$name" "$out"
fi
"#;

pub fn target() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-musl",
        ("linux", "aarch64") => "aarch64-unknown-linux-musl",
        other => panic!("no release for {other:?}"),
    }
}

pub fn sha256(path: &Path) -> String {
    let out = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .or_else(|_| Command::new("sha256sum").arg(path).output())
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

pub fn write_exe(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

pub struct Env {
    pub dir: tempfile::TempDir,
}

impl Env {
    pub fn new() -> Self {
        let env = Env {
            dir: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir_all(env.path("files")).unwrap();
        std::fs::create_dir_all(env.path("logs")).unwrap();
        env.stub("curl", CURL_STUB);
        std::fs::write(env.path("config.toml"), "").unwrap();
        env
    }

    pub fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    /// A program on the stub PATH.
    pub fn stub(&self, name: &str, script: &str) {
        write_exe(&self.path("bin").join(name), script);
    }

    /// A stub that logs its arguments to `logs/<name>.log`, then exits `code`.
    pub fn logging_stub(&self, name: &str, code: i32) {
        self.stub(
            name,
            &format!("#!/bin/sh\necho \"$*\" >> \"$STUB_LOG_DIR/{name}.log\"\nexit {code}\n"),
        );
    }

    /// What a cc-moss needs to see the fake releases page.
    pub fn vars(&self) -> Vec<(String, String)> {
        let path = format!(
            "{}:{}",
            self.path("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        [
            ("PATH", path),
            ("CC_MOSS_RELEASES_URL", RELEASES.to_string()),
            ("CURL_STUB_DIR", self.path("files").display().to_string()),
            ("STUB_LOG_DIR", self.path("logs").display().to_string()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
    }

    pub fn cmd(&self) -> Command {
        let mut cmd = super::bin();
        cmd.env_remove("CC_MOSS_NO_UPDATE_CHECK")
            .env_remove("CARGO_HOME")
            .envs(self.vars())
            .env("CC_MOSS_STATE_DIR", self.path("state"))
            .env("CC_MOSS_CONFIG", self.path("config.toml"));
        cmd
    }

    pub fn run(&self, args: &[&str]) -> Output {
        self.cmd().args(args).output().unwrap()
    }

    /// `update` acting on `exe` instead of the test binary.
    pub fn update(&self, exe: &Path, args: &[&str]) -> Output {
        self.cmd()
            .env("CC_MOSS_UPDATE_EXE", exe)
            .arg("update")
            .args(args)
            .output()
            .unwrap()
    }

    pub fn log(&self, name: &str) -> Vec<String> {
        std::fs::read_to_string(self.path("logs").join(format!("{name}.log")))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }

    /// The releases page says `version` is the latest.
    pub fn serve_latest(&self, version: &str) {
        std::fs::write(
            self.path("files/latest.headers"),
            format!(
                "HTTP/2 302 \r\nlocation: https://github.com/acme/cc-moss/releases/tag/v{version}\r\n\r\n"
            ),
        )
        .unwrap();
    }

    /// Publish `version` for this machine: a tarball whose `cc-moss` prints
    /// `says` for `--version`, and its checksum file.
    pub fn publish(&self, version: &str, says: &str) -> String {
        self.publish_with_app(version, says, None)
    }

    /// Like `publish`, with a (fake) `cc-moss.app` saying `app` next to the
    /// binary, as the macOS release archives carry it.
    pub fn publish_with_app(&self, version: &str, says: &str, app: Option<&str>) -> String {
        let name = format!("cc-moss-v{version}-{}", target());
        let build = self.path("build");
        write_exe(
            &build.join(&name).join("cc-moss"),
            &format!("#!/bin/sh\necho \"{says}\"\n"),
        );
        if let Some(app) = app {
            super::app::fake_app(&build.join(&name).join("cc-moss.app"), app);
        }
        let tarball = self.path("files").join(format!("{name}.tar.gz"));
        let ok = Command::new("tar")
            .arg("-C")
            .arg(&build)
            .arg("-czf")
            .arg(&tarball)
            .arg(&name)
            .status()
            .unwrap();
        assert!(ok.success());
        std::fs::write(
            self.path("files").join(format!("{name}.tar.gz.sha256")),
            format!("{}  {name}.tar.gz\n", sha256(&tarball)),
        )
        .unwrap();
        name
    }

    /// Publish `version` with `cc-moss` as a symlink instead of a file: the
    /// checksum is right, the content is not.
    pub fn publish_symlink(&self, version: &str) -> String {
        let name = format!("cc-moss-v{version}-{}", target());
        let build = self.path("build-link");
        std::fs::create_dir_all(build.join(&name)).unwrap();
        std::os::unix::fs::symlink("/bin/sh", build.join(&name).join("cc-moss")).unwrap();
        let tarball = self.path("files").join(format!("{name}.tar.gz"));
        let ok = Command::new("tar")
            .arg("-C")
            .arg(&build)
            .arg("-czf")
            .arg(&tarball)
            .arg(&name)
            .status()
            .unwrap();
        assert!(ok.success());
        std::fs::write(
            self.path("files").join(format!("{name}.tar.gz.sha256")),
            format!("{}  {name}.tar.gz\n", sha256(&tarball)),
        )
        .unwrap();
        name
    }

    /// A package manager stub that logs its arguments and, like a real
    /// upgrade, leaves a cc-moss at `exe` that says `version`.
    pub fn upgrading_stub(&self, name: &str, exe: &Path, version: &str) {
        self.stub(
            name,
            &format!(
                "#!/bin/sh\necho \"$*\" >> \"$STUB_LOG_DIR/{name}.log\"\nrm -f '{exe}'\nprintf '#!/bin/sh\\necho cc-moss {version}\\n' > '{exe}'\nchmod 755 '{exe}'\n",
                exe = exe.display()
            ),
        );
    }

    pub fn cache(&self) -> Option<serde_json::Value> {
        let text = std::fs::read_to_string(self.path("state/update.json")).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn seed_cache(&self, age_secs: i64, latest: Option<&str>) {
        seed_cache(&self.path("state"), age_secs, latest);
    }

    /// An installed cc-moss outside the test binary, at `rel`.
    pub fn installed(&self, rel: &str) -> PathBuf {
        let exe = self.path(rel);
        write_exe(&exe, OLD_EXE);
        exe
    }
}

pub const OLD_EXE: &str = "#!/bin/sh\necho cc-moss 0.0.1\n";
pub const DAY: i64 = 24 * 60 * 60;

/// Write `<state>/update.json` as if a check `age_secs` ago found `latest`.
pub fn seed_cache(state: &Path, age_secs: i64, latest: Option<&str>) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    std::fs::create_dir_all(state).unwrap();
    std::fs::write(
        state.join("update.json"),
        serde_json::json!({ "checked_at": now - age_secs, "latest": latest }).to_string(),
    )
    .unwrap();
}
