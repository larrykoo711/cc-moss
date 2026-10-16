//! `cc-moss update`, the daily version check and the doctor's version row,
//! all offline against a fake releases page (see `common::releases`).

mod common;

use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output};

use common::releases::{Env, CURRENT, DAY, OLD_EXE, RELEASES};

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

// ---- when the network is used ----------------------------------------

#[test]
fn check_a_fresh_cache_is_not_rechecked() {
    let env = Env::new();
    env.serve_latest("9.9.9");
    env.seed_cache(60, Some(CURRENT));
    let out = env.run(&["doctor"]);
    assert!(env.log("curl").is_empty(), "{:?}", env.log("curl"));
    assert!(text(&out).contains("the latest"), "{}", text(&out));
}

#[test]
fn check_a_day_old_cache_is_rechecked_once() {
    let env = Env::new();
    env.serve_latest("9.9.9");
    env.seed_cache(DAY + 60, Some(CURRENT));
    env.run(&["doctor"]);
    env.run(&["doctor"]);
    let calls = env.log("curl");
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert!(
        calls[0].contains("-I") && calls[0].contains(&format!("{RELEASES}/latest")),
        "{calls:?}"
    );
    assert!(
        calls[0].contains("--proto =https") && calls[0].contains("--tlsv1.2"),
        "https only: {calls:?}"
    );
    assert_eq!(env.cache().unwrap()["latest"], "9.9.9");
}

#[test]
fn check_failures_are_remembered_for_a_day_too() {
    let env = Env::new();
    // Nothing served: the stub fails like an offline machine.
    let out = env.run(&["doctor"]);
    env.run(&["doctor"]);
    assert_eq!(env.log("curl").len(), 1, "{:?}", env.log("curl"));
    assert!(env.cache().unwrap()["checked_at"].as_i64().unwrap() > 0);
    assert!(text(&out).contains("could not check"), "{}", text(&out));
}

#[test]
fn check_off_by_env_never_touches_the_network() {
    let env = Env::new();
    env.serve_latest("9.9.9");
    let out = env
        .cmd()
        .env("CC_MOSS_NO_UPDATE_CHECK", "1")
        .arg("doctor")
        .output()
        .unwrap();
    assert!(env.log("curl").is_empty(), "{:?}", env.log("curl"));
    assert!(env.cache().is_none());
    assert!(text(&out).contains("skipped"), "{}", text(&out));
}

#[test]
fn check_off_by_config_never_touches_the_network() {
    let env = Env::new();
    env.serve_latest("9.9.9");
    std::fs::write(env.path("config.toml"), "[update]\ncheck = false\n").unwrap();
    env.run(&["doctor"]);
    assert!(env.log("curl").is_empty(), "{:?}", env.log("curl"));
}

#[test]
fn hooks_never_check_for_updates() {
    let env = Env::new();
    env.serve_latest("9.9.9");
    let state = env.path("state");
    let id = "00000000-0000-4000-8000-000000000001";
    for (event, json) in [
        ("session-start", common::start_json(id, "startup", None)),
        (
            "user-prompt-submit",
            serde_json::json!({ "session_id": id, "prompt": "hi" }).to_string(),
        ),
        ("stop", serde_json::json!({ "session_id": id }).to_string()),
        ("session-end", common::end_json(id, "logout")),
    ] {
        let mut cmd = env.cmd();
        cmd.args(["hook", event])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let mut child = cmd.spawn().unwrap();
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(json.as_bytes())
            .unwrap();
        child.wait().unwrap();
    }
    assert!(state.join("registry.db").exists(), "the hooks ran");
    for args in [&["ls"][..], &["ls", "--json"], &["ls", "--all"]] {
        env.run(args);
    }
    assert!(env.log("curl").is_empty(), "{:?}", env.log("curl"));
    assert!(env.cache().is_none());
}

#[test]
fn check_update_always_asks_afresh() {
    let env = Env::new();
    env.serve_latest("9.9.9");
    env.seed_cache(60, Some(CURRENT));
    let exe = env.installed("opt/bin/cc-moss");
    let out = env.update(&exe, &["--check"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(env.log("curl").len(), 1, "{:?}", env.log("curl"));
    let t = text(&out);
    assert!(t.contains("9.9.9") && t.contains(CURRENT), "{t}");
    assert!(t.contains(&format!("{RELEASES}/tag/v9.9.9")), "{t}");
    assert_eq!(
        std::fs::read_to_string(&exe).unwrap(),
        OLD_EXE,
        "--check only reports"
    );
    assert_eq!(env.cache().unwrap()["latest"], "9.9.9");
}

#[test]
fn check_update_works_even_with_checking_off() {
    let env = Env::new();
    env.serve_latest(CURRENT);
    std::fs::write(env.path("config.toml"), "[update]\ncheck = false\n").unwrap();
    let exe = env.installed("opt/bin/cc-moss");
    let out = env.update(&exe, &[]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("latest"), "{}", text(&out));
    assert_eq!(env.log("curl").len(), 1);
}

#[test]
fn check_update_explains_when_it_cannot_reach_github() {
    let env = Env::new();
    let exe = env.installed("opt/bin/cc-moss");
    let out = env.update(&exe, &["--yes"]);
    assert!(!out.status.success());
    assert!(
        text(&out).contains("could not check for a new version"),
        "{}",
        text(&out)
    );
    assert_eq!(std::fs::read_to_string(&exe).unwrap(), OLD_EXE);
}

// ---- release binary installs ------------------------------------------

#[test]
fn binary_downloads_verifies_and_replaces() {
    let env = Env::new();
    env.serve_latest("9.9.9");
    let name = env.publish("9.9.9", "cc-moss 9.9.9");
    let exe = env.installed("opt/bin/cc-moss");
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o751)).unwrap();
    let out = env.update(&exe, &["--yes"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        std::fs::read_to_string(&exe)
            .unwrap()
            .contains("cc-moss 9.9.9"),
        "replaced"
    );
    let mode = std::fs::metadata(&exe).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o751, "keeps its permissions");
    let left: Vec<_> = std::fs::read_dir(env.path("opt/bin"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left, ["cc-moss"], "no leftovers");
    let calls = env.log("curl").join("\n");
    for want in [
        format!("{RELEASES}/download/v9.9.9/{name}.tar.gz"),
        format!("{RELEASES}/download/v9.9.9/{name}.tar.gz.sha256"),
    ] {
        assert!(calls.contains(&want), "{want} in\n{calls}");
    }
    assert!(text(&out).contains("Updated to 9.9.9"), "{}", text(&out));
}

#[test]
fn binary_a_bad_checksum_leaves_the_binary_alone() {
    let env = Env::new();
    env.serve_latest("9.9.9");
    let name = env.publish("9.9.9", "cc-moss 9.9.9");
    std::fs::write(
        env.path("files").join(format!("{name}.tar.gz.sha256")),
        format!("{}  {name}.tar.gz\n", "0".repeat(64)),
    )
    .unwrap();
    let exe = env.installed("opt/bin/cc-moss");
    let out = env.update(&exe, &["--yes"]);
    assert!(!out.status.success());
    assert_eq!(std::fs::read_to_string(&exe).unwrap(), OLD_EXE);
    let t = text(&out);
    assert!(
        t.contains("checksum") && t.contains("unchanged"),
        "says why: {t}"
    );
    let left: Vec<_> = std::fs::read_dir(env.path("opt/bin")).unwrap().collect();
    assert_eq!(left.len(), 1, "no leftovers");
}

#[test]
fn binary_a_new_binary_that_fails_its_test_run_leaves_the_old_one_alone() {
    let env = Env::new();
    env.serve_latest("9.9.9");
    env.publish("9.9.9", "something else");
    let exe = env.installed("opt/bin/cc-moss");
    let out = env.update(&exe, &["--yes"]);
    assert!(!out.status.success());
    assert_eq!(std::fs::read_to_string(&exe).unwrap(), OLD_EXE);
    assert!(text(&out).contains("unchanged"), "{}", text(&out));
}

#[test]
fn binary_a_missing_download_leaves_the_old_one_alone() {
    let env = Env::new();
    env.serve_latest("9.9.9");
    let exe = env.installed("opt/bin/cc-moss");
    let out = env.update(&exe, &["--yes"]);
    assert!(!out.status.success());
    assert_eq!(std::fs::read_to_string(&exe).unwrap(), OLD_EXE);
    assert!(text(&out).contains("could not download"), "{}", text(&out));
}

#[test]
fn binary_a_read_only_folder_gets_steps_to_do_it_by_hand() {
    if Command::new("id")
        .arg("-u")
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
    {
        return; // root can write anywhere
    }
    let env = Env::new();
    env.serve_latest("9.9.9");
    let name = env.publish("9.9.9", "cc-moss 9.9.9");
    let exe = env.installed("opt/bin/cc-moss");
    let dir = env.path("opt/bin");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    let out = env.update(&exe, &["--yes"]);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!out.status.success());
    assert_eq!(std::fs::read_to_string(&exe).unwrap(), OLD_EXE);
    let t = text(&out);
    assert!(t.contains("cannot write"), "{t}");
    assert!(
        t.contains(&format!("{RELEASES}/download/v9.9.9/{name}.tar.gz")),
        "{t}"
    );
    assert!(
        env.log("curl").iter().all(|c| c.contains("-I")),
        "no download into a folder it cannot write: {:?}",
        env.log("curl")
    );
}

#[test]
fn binary_asks_first_and_refuses_without_a_terminal() {
    let env = Env::new();
    env.serve_latest("9.9.9");
    env.publish("9.9.9", "cc-moss 9.9.9");
    let exe = env.installed("opt/bin/cc-moss");
    let out = env.update(&exe, &[]);
    assert!(!out.status.success());
    assert!(text(&out).contains("--yes"), "{}", text(&out));
    assert_eq!(std::fs::read_to_string(&exe).unwrap(), OLD_EXE);
}

#[test]
fn binary_already_latest_is_a_friendly_no_op() {
    let env = Env::new();
    env.serve_latest(CURRENT);
    let exe = env.installed("opt/bin/cc-moss");
    let out = env.update(&exe, &["--yes"]);
    assert!(out.status.success());
    assert!(
        text(&out).contains("already on the latest"),
        "{}",
        text(&out)
    );
    assert_eq!(env.log("curl").len(), 1, "no download");
}

#[test]
fn binary_speaks_chinese() {
    let env = Env::new();
    env.serve_latest("9.9.9");
    let name = env.publish("9.9.9", "cc-moss 9.9.9");
    std::fs::write(
        env.path("files").join(format!("{name}.tar.gz.sha256")),
        format!("{}  {name}.tar.gz\n", "0".repeat(64)),
    )
    .unwrap();
    let exe = env.installed("opt/bin/cc-moss");
    let out = env
        .cmd()
        .env("CC_MOSS_LANG", "zh")
        .env("CC_MOSS_UPDATE_EXE", &exe)
        .args(["update", "--yes"])
        .output()
        .unwrap();
    let t = text(&out);
    assert!(t.contains("校验") && t.contains("原样"), "{t}");
}

// ---- Homebrew and cargo installs --------------------------------------

#[test]
fn managed_homebrew_runs_brew_upgrade() {
    let env = Env::new();
    env.serve_latest("9.9.9");
    env.logging_stub("brew", 0);
    let exe = env.installed("homebrew/Cellar/cc-moss/0.0.1/bin/cc-moss");
    let out = env.update(&exe, &["--yes"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(env.log("brew"), ["upgrade larrykoo711/tap/cc-moss"]);
    assert_eq!(env.log("curl").len(), 1, "brew downloads, not cc-moss");
    assert_eq!(std::fs::read_to_string(&exe).unwrap(), OLD_EXE);
}

#[test]
fn managed_homebrew_through_its_bin_link() {
    let env = Env::new();
    env.serve_latest("9.9.9");
    let exe = env.installed("homebrew/Cellar/cc-moss/0.0.1/bin/cc-moss");
    let link = env.path("homebrew/bin/cc-moss");
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&exe, &link).unwrap();
    // A real upgrade points bin/cc-moss at the new version.
    env.upgrading_stub("brew", &link, "9.9.9");
    let out = env.update(&link, &["--yes"]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(env.log("brew"), ["upgrade larrykoo711/tap/cc-moss"]);
    assert!(text(&out).contains("Updated to 9.9.9"), "{}", text(&out));
}

#[test]
fn managed_homebrew_that_did_not_upgrade_is_reported() {
    // brew says "already installed" and exits 0 while the tap lags behind.
    let env = Env::new();
    env.serve_latest("9.9.9");
    env.logging_stub("brew", 0);
    let exe = env.installed("homebrew/Cellar/cc-moss/0.0.1/bin/cc-moss");
    let link = env.path("homebrew/bin/cc-moss");
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&exe, &link).unwrap();
    let out = env.update(&link, &["--yes"]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(!text(&out).contains("Updated to"), "{}", text(&out));
    assert!(text(&out).contains("0.0.1"), "{}", text(&out));
}

#[test]
fn managed_cargo_that_did_not_upgrade_is_reported() {
    let env = Env::new();
    env.serve_latest("9.9.9");
    env.logging_stub("cargo", 0);
    let exe = env.installed("cargo-home/bin/cc-moss");
    let out = env
        .cmd()
        .env("CARGO_HOME", env.path("cargo-home"))
        .env("CC_MOSS_UPDATE_EXE", &exe)
        .args(["update", "--yes"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "{}", text(&out));
    assert!(!text(&out).contains("Updated to"), "{}", text(&out));
}

#[test]
fn binary_a_symlink_in_the_download_is_refused() {
    let env = Env::new();
    env.serve_latest("9.9.9");
    env.publish_symlink("9.9.9");
    let exe = env.installed("opt/bin/cc-moss");
    let out = env.update(&exe, &["--yes"]);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("did not unpack"), "{}", text(&out));
    assert_eq!(std::fs::read_to_string(&exe).unwrap(), OLD_EXE);
}

#[test]
fn managed_cargo_runs_cargo_install() {
    let env = Env::new();
    env.serve_latest("9.9.9");
    let exe = env.installed("cargo-home/bin/cc-moss");
    env.upgrading_stub("cargo", &exe, "9.9.9");
    let out = env
        .cmd()
        .env("CARGO_HOME", env.path("cargo-home"))
        .env("CC_MOSS_UPDATE_EXE", &exe)
        .args(["update", "--yes"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(
        env.log("cargo"),
        ["install --locked --git https://github.com/larrykoo711/cc-moss --tag v9.9.9 cc-moss"]
    );
}

#[test]
fn managed_cargo_default_home_is_under_home() {
    let env = Env::new();
    env.serve_latest("9.9.9");
    let exe = env.installed("home/.cargo/bin/cc-moss");
    env.upgrading_stub("cargo", &exe, "9.9.9");
    let out = env
        .cmd()
        .env("HOME", env.path("home"))
        .env("CC_MOSS_UPDATE_EXE", &exe)
        .args(["update", "--yes"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(env.log("cargo").len(), 1);
}

#[test]
fn managed_a_failing_brew_is_reported() {
    let env = Env::new();
    env.serve_latest("9.9.9");
    env.logging_stub("brew", 1);
    let exe = env.installed("homebrew/Cellar/cc-moss/0.0.1/bin/cc-moss");
    let out = env.update(&exe, &["--yes"]);
    assert!(!out.status.success());
    assert!(
        text(&out).contains("brew upgrade larrykoo711/tap/cc-moss"),
        "{}",
        text(&out)
    );
}

// ---- doctor -------------------------------------------------------------

fn version_row(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .skip_while(|l| !l.contains("version"))
        .take(2)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn doctor_says_when_cc_moss_is_up_to_date() {
    let env = Env::new();
    env.serve_latest(CURRENT);
    let row = version_row(&env.run(&["doctor"]));
    assert!(
        row.contains('✓') && row.contains(CURRENT) && row.contains("the latest"),
        "{row}"
    );
}

#[test]
fn doctor_points_at_a_new_version() {
    let env = Env::new();
    env.serve_latest("9.9.9");
    let row = version_row(&env.run(&["doctor"]));
    assert!(
        row.contains('!') && row.contains("9.9.9 is out") && row.contains("→ cc-moss update"),
        "{row}"
    );
}

#[test]
fn doctor_skips_the_version_when_checking_is_off() {
    let env = Env::new();
    env.serve_latest("9.9.9");
    let out = env
        .cmd()
        .env("CC_MOSS_NO_UPDATE_CHECK", "1")
        .arg("doctor")
        .output()
        .unwrap();
    let row = version_row(&out);
    assert!(
        row.contains('·') && row.contains(CURRENT) && row.contains("skipped"),
        "{row}"
    );
}

#[test]
fn check_json_reports_a_newer_version() {
    let env = Env::new();
    env.serve_latest("9.9.9");
    let exe = env.installed("opt/bin/cc-moss");
    let out = env.update(&exe, &["--check", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON on stdout");
    assert_eq!(v["schema"], 1);
    assert_eq!(v["current"], CURRENT);
    assert_eq!(v["latest"], "9.9.9");
    assert_eq!(v["available"], true);
    assert_eq!(v["notes"], format!("{RELEASES}/tag/v9.9.9"));
}

#[test]
fn check_json_when_already_latest() {
    let env = Env::new();
    env.serve_latest(CURRENT);
    let exe = env.installed("opt/bin/cc-moss");
    let out = env.update(&exe, &["--check", "--json"]);
    assert!(out.status.success(), "{}", text(&out));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON on stdout");
    assert_eq!(v["available"], false);
    assert_eq!(v["latest"], CURRENT);
    assert!(v["notes"].is_null());
}

#[test]
fn json_needs_check() {
    let env = Env::new();
    let exe = env.installed("opt/bin/cc-moss");
    let out = env.update(&exe, &["--json"]);
    assert!(
        !out.status.success(),
        "--json without --check must be refused"
    );
}

// ---- the menubar app (macOS) ------------------------------------------

mod app_update {
    use super::*;
    use crate::common::app;
    use std::path::{Path, PathBuf};

    /// A fake releases page plus a temporary home on macOS, whose
    /// ~/Applications holds an app saying `installed` (none if `None`).
    struct Mac {
        env: Env,
    }

    impl Mac {
        fn new(installed: Option<&str>, running: bool) -> Mac {
            let env = Env::new();
            app::stubs(&env.path("bin"), &env.path("logs"), running);
            let mac = Mac { env };
            if let Some(v) = installed {
                app::fake_app(&mac.user_app(), v);
            }
            mac
        }

        fn user_app(&self) -> PathBuf {
            self.env.path("home/Applications/cc-moss.app")
        }

        fn log(&self, name: &str) -> String {
            app::log(&self.env.path("logs"), name)
        }

        fn update(&self, exe: &Path, extra: &[(&str, PathBuf)]) -> Output {
            let mut cmd = self.env.cmd();
            cmd.env("HOME", self.env.path("home"))
                .env("CC_MOSS_PLATFORM", "macos")
                .env("CC_MOSS_UPDATE_EXE", exe);
            for (k, v) in extra {
                cmd.env(k, v);
            }
            cmd.args(["update", "--yes"]).output().unwrap()
        }
    }

    #[test]
    fn app_binary_update_replaces_the_installed_app() {
        let mac = Mac::new(Some("0.0.1"), false);
        mac.env.serve_latest("9.9.9");
        mac.env
            .publish_with_app("9.9.9", "cc-moss 9.9.9", Some("9.9.9"));
        let exe = mac.env.installed("opt/bin/cc-moss");
        let out = mac.update(&exe, &[]);
        assert!(out.status.success(), "{}", text(&out));
        assert_eq!(app::version(&mac.user_app()).as_deref(), Some("9.9.9"));
        assert!(text(&out).contains("menubar app"), "{}", text(&out));
        // Not running before, so not started now.
        assert!(mac.log("open").is_empty(), "{}", mac.log("open"));
        let left: Vec<_> = std::fs::read_dir(mac.env.path("opt/bin"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left, ["cc-moss"], "no leftovers");
    }

    #[test]
    fn app_running_is_quit_and_reopened() {
        let mac = Mac::new(Some("0.0.1"), true);
        mac.env.serve_latest("9.9.9");
        mac.env
            .publish_with_app("9.9.9", "cc-moss 9.9.9", Some("9.9.9"));
        let exe = mac.env.installed("opt/bin/cc-moss");
        let out = mac.update(&exe, &[]);
        assert!(out.status.success(), "{}", text(&out));
        assert_eq!(app::version(&mac.user_app()).as_deref(), Some("9.9.9"));
        assert!(
            mac.log("osascript").contains(&format!(
                "tell application id \"{}\" to quit",
                app::BUNDLE_ID
            )),
            "{}",
            mac.log("osascript")
        );
        let open = mac.log("open");
        assert!(
            open.contains("-g") && open.contains(&mac.user_app().display().to_string()),
            "{open}"
        );
        assert!(mac.log("pkill").is_empty() && mac.log("killall").is_empty());
    }

    #[test]
    fn app_not_installed_stays_not_installed() {
        let mac = Mac::new(None, false);
        mac.env.serve_latest("9.9.9");
        mac.env
            .publish_with_app("9.9.9", "cc-moss 9.9.9", Some("9.9.9"));
        let exe = mac.env.installed("opt/bin/cc-moss");
        let out = mac.update(&exe, &[]);
        assert!(out.status.success(), "{}", text(&out));
        assert!(!mac.user_app().exists());
        assert!(mac.log("osascript").is_empty());
    }

    #[test]
    fn app_cargo_update_only_mentions_it() {
        let mac = Mac::new(Some("0.0.1"), false);
        mac.env.serve_latest("9.9.9");
        let exe = mac.env.installed("cargo-home/bin/cc-moss");
        mac.env.upgrading_stub("cargo", &exe, "9.9.9");
        let out = mac.update(&exe, &[("CARGO_HOME", mac.env.path("cargo-home"))]);
        assert!(out.status.success(), "{}", text(&out));
        assert_eq!(app::version(&mac.user_app()).as_deref(), Some("0.0.1"));
        assert!(text(&out).contains("0.0.1"), "{}", text(&out));
    }

    #[test]
    fn app_homebrew_update_takes_the_new_keg() {
        let mac = Mac::new(Some("0.0.1"), false);
        mac.env.serve_latest("9.9.9");
        let old = mac
            .env
            .installed("homebrew/Cellar/cc-moss/0.0.1/bin/cc-moss");
        let link = mac.env.path("homebrew/bin/cc-moss");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&old, &link).unwrap();
        // Like `brew upgrade`: a new keg with its own app, bin/ linked to it.
        let keg = mac.env.path("homebrew/Cellar/cc-moss/9.9.9");
        app::fake_app(&keg.join("cc-moss.app"), "9.9.9");
        mac.env.stub(
            "brew",
            &format!(
                "#!/bin/sh\nmkdir -p '{keg}/bin'\nprintf '#!/bin/sh\\necho cc-moss 9.9.9\\n' > '{keg}/bin/cc-moss'\nchmod 755 '{keg}/bin/cc-moss'\nln -sf '{keg}/bin/cc-moss' '{link}'\n",
                keg = keg.display(),
                link = link.display()
            ),
        );
        let out = mac.update(&link, &[]);
        assert!(out.status.success(), "{}", text(&out));
        assert_eq!(app::version(&mac.user_app()).as_deref(), Some("9.9.9"));
    }

    #[test]
    fn app_left_alone_under_sudo() {
        let mac = Mac::new(Some("0.0.1"), true);
        app::mark(&mac.user_app());
        mac.env.serve_latest("9.9.9");
        mac.env
            .publish_with_app("9.9.9", "cc-moss 9.9.9", Some("9.9.9"));
        let exe = mac.env.installed("opt/bin/cc-moss");
        let out = mac.update(&exe, &[("SUDO_USER", PathBuf::from("demo"))]);
        let t = text(&out);
        assert!(out.status.success(), "{t}");
        assert!(app::is_marked(&mac.user_app()), "{t}");
        assert_eq!(app::version(&mac.user_app()).as_deref(), Some("0.0.1"));
        assert!(t.contains("without sudo"), "{t}");
        for tool in ["osascript", "kill", "open", "pgrep"] {
            assert!(mac.log(tool).is_empty(), "{tool} was run");
        }
    }

    #[test]
    fn app_newer_installed_is_not_downgraded() {
        let mac = Mac::new(Some("99.0.0"), false);
        app::mark(&mac.user_app());
        mac.env.serve_latest("9.9.9");
        mac.env
            .publish_with_app("9.9.9", "cc-moss 9.9.9", Some("9.9.9"));
        let exe = mac.env.installed("opt/bin/cc-moss");
        let out = mac.update(&exe, &[]);
        let t = text(&out);
        assert!(out.status.success(), "{t}");
        assert!(app::is_marked(&mac.user_app()), "{t}");
        assert!(t.contains("99.0.0") && t.contains("newer"), "{t}");
    }

    #[test]
    fn app_dev_build_is_left_alone() {
        let mac = Mac::new(Some("dev"), false);
        app::mark(&mac.user_app());
        mac.env.serve_latest("9.9.9");
        mac.env
            .publish_with_app("9.9.9", "cc-moss 9.9.9", Some("9.9.9"));
        let exe = mac.env.installed("opt/bin/cc-moss");
        let out = mac.update(&exe, &[]);
        let t = text(&out);
        assert!(out.status.success(), "{t}");
        assert!(app::is_marked(&mac.user_app()), "{t}");
        assert!(t.contains("unknown"), "{t}");
    }

    #[test]
    fn app_by_hand_steps_mention_the_app() {
        if Command::new("id")
            .arg("-u")
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
        {
            return; // root can write anywhere
        }
        let mac = Mac::new(Some("0.0.1"), false);
        mac.env.serve_latest("9.9.9");
        let name = mac
            .env
            .publish_with_app("9.9.9", "cc-moss 9.9.9", Some("9.9.9"));
        let exe = mac.env.installed("opt/bin/cc-moss");
        let dir = mac.env.path("opt/bin");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let out = mac.update(&exe, &[]);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let t = text(&out);
        assert!(!out.status.success(), "{t}");
        assert!(
            t.contains(&format!("{name}/cc-moss.app"))
                && t.contains(&format!("./{name}/cc-moss install")),
            "{t}"
        );
    }
}
