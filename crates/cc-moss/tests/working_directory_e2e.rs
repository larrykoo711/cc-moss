//! 所有工作目录检查仅使用隔离HOME/状态目录与tmux socket。
mod common;
use common::{wait_for, World};
use std::process::{Child, Stdio};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn db(w: &World) -> rusqlite::Connection {
    rusqlite::Connection::open(w.state().join("registry.db")).unwrap()
}
fn mark(w: &World) -> (Option<i64>, String, Option<String>) {
    db(w)
        .query_row(
            "SELECT restore_started_at,cwd,name FROM slots LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
}

#[test]
fn working_directory_custom_cc_moss_config_parent_is_forbidden_even_without_claude_config() {
    let w = World::new("cwd-config-parent");
    let config = w.path("custom-settings");
    let neighbor = w.path("custom-settings-project");
    std::fs::create_dir(&config).unwrap();
    std::fs::create_dir(&neighbor).unwrap();
    let file = config.join("config.toml");
    for exists in [false, true] {
        if exists {
            std::fs::write(&file, "").unwrap();
        }
        let out = w
            .cc_moss()
            .env("CC_MOSS_CONFIG", &file)
            .args(["check-directory", "--json", "--"])
            .arg(&config)
            .arg(&neighbor)
            .output()
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["results"][0]["reason"], "cwd_forbidden", "{value}");
        assert_eq!(value["results"][1]["ok"], true, "{value}");
    }
}

#[test]
fn working_directory_custom_xdg_config_parent_is_forbidden_without_config_file() {
    let w = World::new("cwd-xdg-parent");
    let xdg = w.path("custom-xdg");
    let config = xdg.join("cc-moss");
    let neighbor = xdg.join("cc-moss-project");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir(&neighbor).unwrap();
    let out = w
        .cc_moss()
        .env_remove("CC_MOSS_CONFIG")
        .env("XDG_CONFIG_HOME", &xdg)
        .args(["check-directory", "--json", "--"])
        .arg(&config)
        .arg(&neighbor)
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["results"][0]["reason"], "cwd_forbidden", "{value}");
    assert_eq!(value["results"][1]["ok"], true, "{value}");
}

#[test]
fn working_directory_config_file_symlink_protects_both_parent_directories() {
    let w = World::new("cwd-config-link");
    let visible = w.path("visible-config");
    let real = w.path("real-config");
    let neighbor = w.path("real-config-project");
    for path in [&visible, &real, &neighbor] {
        std::fs::create_dir(path).unwrap();
    }
    std::fs::write(real.join("config.toml"), "").unwrap();
    std::os::unix::fs::symlink(real.join("config.toml"), visible.join("config.toml")).unwrap();
    let out = w
        .cc_moss()
        .env("CC_MOSS_CONFIG", visible.join("config.toml"))
        .args(["check-directory", "--json", "--"])
        .arg(&visible)
        .arg(&real)
        .arg(&neighbor)
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["results"][0]["reason"], "cwd_forbidden", "{value}");
    assert_eq!(value["results"][1]["reason"], "cwd_forbidden", "{value}");
    assert_eq!(value["results"][2]["ok"], true, "{value}");
}

#[test]
fn working_directory_restore_denies_history_before_claim_or_tmux() {
    let w = World::new("cwd-restore-t12");
    w.new_session("history");
    let sid = w.session_id("history");
    w.first_prompt(&sid, "history");
    w.tmux(&["kill-server"]);
    wait_for(|| {
        let pid: u32 = db(&w)
            .query_row("SELECT pid FROM slots", [], |r| r.get(0))
            .ok()?;
        (!moss_core::proc::is_alive(pid, None)).then_some(())
    });
    db(&w).execute("UPDATE slots SET cwd='/'", []).unwrap();
    let before = mark(&w);
    let out = w
        .cc_moss()
        .args(["restore", "--yes", "--json", "--", &sid])
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["results"][0]["reason"], "cwd_forbidden", "{value}");
    assert_eq!(mark(&w), before);
    assert!(w.session_names().is_empty());
}

#[test]
fn working_directory_restart_denies_busy_before_queue_claim_and_term() {
    let w = World::new("cwd-restart-t12");
    w.new_session("busy");
    let sid = w.session_id("busy");
    w.first_prompt(&sid, "busy");
    db(&w).execute("UPDATE slots SET cwd='/'", []).unwrap();
    let before = mark(&w);
    let pid: u32 = db(&w)
        .query_row("SELECT pid FROM slots", [], |r| r.get(0))
        .unwrap();
    for flags in [vec![], vec!["--now", "--yes"]] {
        let out = w
            .cc_moss()
            .args(["restart", "--json"])
            .args(flags)
            .args(["--", &sid])
            .output()
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["results"][0]["reason"], "cwd_forbidden", "{value}");
        assert_eq!(mark(&w), before);
        assert!(moss_core::proc::is_alive(pid, None));
        assert!(!w.state().join("daemon/restart-queue.json").exists());
    }
}

#[test]
fn working_directory_adopt_pins_real_new_process_home_despite_tmux_pollution() {
    use std::os::unix::fs::PermissionsExt;
    let mut w = World::new("cwd-adopt-home");
    let script=std::fs::read_to_string(&w.claude).unwrap().replace("start \"$source\"", "printf '%s|%s|%s\\n' \"$sid\" \"$$\" \"$HOME\" >> \"$CC_MOSS_FAKE_DIR/homes\"\nstart \"$source\"");
    w.claude = w.path("observed-claude");
    std::fs::write(&w.claude, script).unwrap();
    std::fs::set_permissions(&w.claude, std::fs::Permissions::from_mode(0o700)).unwrap();
    w.new_session("keeper");
    w.session_id("keeper");
    w.tmux(&[
        "set-environment",
        "-g",
        "HOME",
        "/nonexistent/polluted-home",
    ]);
    let sid = uuid::Uuid::new_v4().to_string();
    let mut cmd = std::process::Command::new(&w.claude);
    for (key, value) in w.cc_moss().get_envs() {
        match value {
            Some(value) => {
                cmd.env(key, value);
            }
            None => {
                cmd.env_remove(key);
            }
        }
    }
    let mut outside = Process(
        cmd.args(["--session-id", &sid, "--name=outside"])
            .current_dir(w.path("work"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_for(|| {
        let raw: Option<String> = db(&w)
            .query_row(
                "SELECT provider_started FROM slots WHERE session_id=?1",
                [&sid],
                |r| r.get(0),
            )
            .ok()?;
        raw
    });
    let old_pid = outside.0.id();
    let out = w
        .cc_moss()
        .args(["adopt", "--json", &sid])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let observed = wait_for(|| {
        let text = std::fs::read_to_string(w.path("fifos/homes")).ok()?;
        text.lines()
            .filter(|line| line.starts_with(&format!("{sid}|")))
            .find_map(|line| {
                let parts: Vec<_> = line.split('|').collect();
                (parts.get(1).and_then(|pid| pid.parse::<u32>().ok()) != Some(old_pid))
                    .then(|| parts[2].to_string())
            })
    });
    assert_eq!(
        observed,
        w.path("home").canonicalize().unwrap().to_string_lossy()
    );
    assert!(outside.0.try_wait().unwrap().is_some());
}

#[test]
fn working_directory_adopt_denies_before_name_touch_claim_and_term() {
    let w = World::new("cwd-adopt-t12");
    let sid = uuid::Uuid::new_v4().to_string();
    let mut cmd = std::process::Command::new(&w.claude);
    for (key, value) in w.cc_moss().get_envs() {
        match value {
            Some(value) => {
                cmd.env(key, value);
            }
            None => {
                cmd.env_remove(key);
            }
        }
    }
    let mut process = Process(
        cmd.args(["--session-id", &sid])
            .current_dir(w.path("work"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_for(|| {
        if !w.state().join("registry.db").exists() {
            return None;
        }
        let snapshot: Option<String> = db(&w)
            .query_row(
                "SELECT provider_started FROM slots WHERE session_id=?1",
                [&sid],
                |r| r.get(0),
            )
            .ok()?;
        snapshot
    });
    db(&w).execute("UPDATE slots SET cwd='/'", []).unwrap();
    let before = mark(&w);
    let out = w
        .cc_moss()
        .args(["adopt", "--json", &sid])
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["reason"], "cwd_forbidden", "{value}");
    assert_eq!(mark(&w), before);
    assert!(process.0.try_wait().unwrap().is_none());
    assert!(w.session_names().is_empty());
}

#[test]
fn working_directory_explicit_configuration_root_is_not_a_project() {
    let w = World::new("cwd-config-t12");
    let root = w.path("work");
    let out = w
        .cc_moss()
        .env("CLAUDE_CONFIG_DIR", &root)
        .args(["check-directory", "--json", "--"])
        .arg(&root)
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["results"][0]["reason"], "cwd_forbidden");
    assert!(root.is_dir());
}

#[test]
fn working_directory_queued_restart_rechecks_and_stops_retrying() {
    let w = World::new("cwd-queue-t12");
    std::fs::write(w.config_path(), "[moss]\nenabled=false\n").unwrap();
    w.new_session("queued");
    let sid = w.session_id("queued");
    w.first_prompt(&sid, "queued");
    let out = w
        .cc_moss()
        .args(["restart", "--json", &sid])
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["results"][0]["result"], "queued", "{value}");
    w.send(&sid, "stop");
    wait_for(|| {
        let busy: bool = db(&w)
            .query_row("SELECT busy FROM slots", [], |r| r.get(0))
            .ok()?;
        (!busy).then_some(())
    });
    db(&w).execute("UPDATE slots SET cwd='/'", []).unwrap();
    let pid: u32 = db(&w)
        .query_row("SELECT pid FROM slots", [], |r| r.get(0))
        .unwrap();
    let _daemon = Process(
        w.cc_moss()
            .env("CC_MOSS_MOSS_SOCKET", &w.socket)
            .env("CC_MOSS_DAEMON_HEARTBEAT_MS", "100")
            .args(["daemon", "run"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_for(|| {
        let text = std::fs::read(w.state().join("daemon/restart-queue.json")).ok()?;
        let queue: serde_json::Value = serde_json::from_slice(&text).ok()?;
        let empty = queue.as_array().is_some_and(Vec::is_empty)
            || queue["entries"].as_array().is_some_and(Vec::is_empty);
        empty.then_some(())
    });
    assert!(moss_core::proc::is_alive(pid, None));
    assert!(mark(&w).0.is_none());
}

#[test]
fn working_directory_moss_has_no_role_exemption_or_layout_before_guard() {
    common::debug_only();
    let w = World::new("cwd-moss-t12");
    let state = w.path("home/.config/blocked-state");
    std::fs::write(w.config_path(), "[moss]\nenabled=true\n").unwrap();
    let _daemon = Process(
        w.cc_moss()
            .env("CC_MOSS_STATE_DIR", &state)
            .env("CC_MOSS_MOSS_SOCKET", &w.socket)
            .env("CC_MOSS_MOSS_CHECK_MS", "100")
            .args(["daemon", "run"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_for(|| {
        moss_core::moss::read_error(&state).filter(|text| text.contains("working directory"))
    });
    assert!(!state.join("moss").exists());
    assert!(w.session_names().is_empty());
}

#[test]
fn working_directory_query_rejects_root_and_home_but_allows_user_containers() {
    let w = World::new("cwd-query-t12");
    for name in ["Documents", "Desktop", "Downloads"] {
        std::fs::create_dir(w.path(&format!("home/{name}"))).unwrap();
    }
    let out = w
        .cc_moss()
        .args(["check-directory", "--json", "--", "/"])
        .arg(w.path("home"))
        .arg(w.path("home/Documents"))
        .arg(w.path("home/Desktop"))
        .arg(w.path("home/Downloads"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    for row in &value["results"].as_array().unwrap()[..2] {
        assert_eq!(row["reason"], "cwd_forbidden", "{row}");
    }
    for row in &value["results"].as_array().unwrap()[2..] {
        assert_eq!(row["ok"], true, "{row}");
    }
    assert!(!w.state().join("registry.db").exists());
}

#[test]
fn working_directory_query_is_quiet_and_does_not_consume_stdin() {
    let w = World::new("cwd-quiet-t12");
    let out = w
        .cc_moss()
        .args(["check-directory", "--"])
        .arg(w.path("work"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
}
