//! 隔离 tmux PTY 驱动真实 picker 恢复；仅最后终端连接记录化。

mod common;

use common::{ls_json, wait_for, World};
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn picker_named_restore_ignores_uuid_shadow_and_attaches_exact_original() {
    common::debug_only();
    let w = World::new("picker-explicit-restore");
    let home = w.path("home");
    std::fs::create_dir_all(&home).unwrap();
    let created = w
        .cc_moss()
        .env("HOME", &home)
        .args(["new", "picker-original", "--detach", "-C"])
        .arg(w.path("work"))
        .output()
        .unwrap();
    assert!(created.status.success(), "{created:?}");
    let original = w.session_id("picker-original");
    w.first_prompt(&original, "picker-original");
    w.send(&original, "stop");
    let before = wait_for(|| {
        let listed = ls_json(&w.state(), true);
        listed["sessions"]
            .as_array()?
            .iter()
            .find(|slot| slot["session_id"] == original && slot["busy"] == false)
            .cloned()
    });
    // 显示名恰好等于目标完整 UUID；普通文本 resolver 会优先匹配此遮蔽会话。
    let shadow = w
        .cc_moss()
        .env("HOME", &home)
        .args(["new", &original, "--detach", "-C"])
        .arg(w.path("work"))
        .output()
        .unwrap();
    assert!(shadow.status.success(), "{shadow:?}");
    let shadow_id = w.session_id(&original);
    w.first_prompt(&shadow_id, &original);
    w.send(&shadow_id, "stop");
    let shadow_before = wait_for(|| {
        let listed = ls_json(&w.state(), true);
        listed["sessions"]
            .as_array()?
            .iter()
            .find(|slot| slot["session_id"] == shadow_id && slot["busy"] == false)
            .cloned()
    });
    assert_eq!(shadow_before["name"], original);
    w.tmux(&["kill-server"]);
    wait_for(|| {
        let listed = ls_json(&w.state(), true);
        listed["sessions"]
            .as_array()?
            .iter()
            .any(|slot| slot["session_id"] == original && slot["status"] == "interrupted")
            .then_some(())
    });

    // 仅测试数据库的观测触发器：捕获任何恢复 claim，不能事后清空来伪造零副作用。
    let probe = rusqlite::Connection::open(w.state().join("registry.db")).unwrap();
    probe
        .execute_batch(
            "CREATE TABLE picker_claim_probe(slot_id TEXT NOT NULL);
        CREATE TRIGGER picker_claim_probe AFTER UPDATE OF restore_started_at ON slots
        WHEN NEW.restore_started_at IS NOT NULL AND OLD.restore_started_at IS NULL
        BEGIN INSERT INTO picker_claim_probe VALUES(NEW.slot_id); END;",
        )
        .unwrap();

    let real_tmux = Command::new("which").arg("tmux").output().unwrap();
    assert!(real_tmux.status.success());
    let real_tmux = String::from_utf8(real_tmux.stdout)
        .unwrap()
        .trim()
        .to_string();
    let shim_dir = w.path("shim");
    std::fs::create_dir(&shim_dir).unwrap();
    let attach_log = w.path("attach.args");
    let shim = shim_dir.join("tmux");
    std::fs::write(
        &shim,
        r#"#!/bin/sh
case " $* " in
  *" switch-client "*|*" attach-session "*)
    printf '%s\000' "$@" > "$CC_MOSS_PICK_ATTACH_LOG"
    exit 0 ;;
esac
exec "$CC_MOSS_PICK_REAL_TMUX" "$@"
"#,
    )
    .unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();

    // 从 World 取得实际配置路径；不另猜 config.toml 或覆盖共享夹具。
    let template = w.cc_moss();
    let mut env_args = Vec::new();
    for (key, value) in template.get_envs() {
        if value.is_none() && key != "TMUX" && key != "TMUX_PANE" {
            env_args.extend(["-u".to_string(), key.to_string_lossy().into_owned()]);
        }
    }
    for (key, value) in template.get_envs() {
        if let Some(value) = value {
            env_args.push(format!(
                "{}={}",
                key.to_string_lossy(),
                value.to_string_lossy()
            ));
        }
    }
    env_args.extend([
        format!("HOME={}", home.display()),
        format!(
            "PATH={}:{}",
            shim_dir.display(),
            std::env::var("PATH").unwrap()
        ),
        format!("CC_MOSS_PICK_REAL_TMUX={real_tmux}"),
        format!("CC_MOSS_PICK_ATTACH_LOG={}", attach_log.display()),
        "TERM=xterm-256color".into(),
        env!("CARGO_BIN_EXE_cc-moss").into(),
        "pick".into(),
        "picker-original".into(),
    ]);
    let mut starter = Command::new(&real_tmux);
    for (key, value) in template.get_envs() {
        match value {
            Some(value) => {
                starter.env(key, value);
            }
            None => {
                starter.env_remove(key);
            }
        }
    }
    let driver = starter
        .env("HOME", &home)
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        // 独立 socket、不读任何 tmux 配置；保留退出窗格只为失败时可读屏。
        .args([
            "-L",
            &w.socket,
            "-f",
            "/dev/null",
            "start-server",
            ";",
            "set-option",
            "-g",
            "remain-on-exit",
            "on",
            ";",
            "new-session",
            "-d",
            "-s",
            "picker-driver",
            "-x",
            "100",
            "-y",
            "30",
            "-c",
        ])
        .arg(w.path("work"))
        .arg("/usr/bin/env")
        .args(env_args)
        .output()
        .unwrap();
    assert!(driver.status.success(), "{driver:?}");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let screen = w.tmux(&["capture-pane", "-p", "-t", "picker-driver"]);
        if screen.contains("picker-original") && screen.to_ascii_lowercase().contains("restore") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "picker没有绘制恢复行:\n{screen}\n{}",
            w.tmux(&[
                "list-panes",
                "-a",
                "-F",
                "#{pane_dead}:#{pane_start_command}"
            ])
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    w.tmux(&["send-keys", "-t", "picker-driver", "Enter"]);
    wait_for(|| {
        let wrong_claims: i64 = probe
            .query_row(
                "SELECT count(*) FROM picker_claim_probe WHERE slot_id=?1",
                [shadow_before["id"].as_str().unwrap()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(wrong_claims, 0, "显示名遮蔽不能让错误 slot 获得恢复 claim");
        attach_log.exists().then_some(())
    });

    let listed = ls_json(&w.state(), true);
    let sessions = listed["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 2, "显式恢复不能新增 UUID: {listed}");
    let restored = sessions
        .iter()
        .find(|slot| slot["session_id"] == original)
        .unwrap();
    let untouched = sessions
        .iter()
        .find(|slot| slot["session_id"] == shadow_id)
        .unwrap();
    assert_eq!(untouched["status"], "interrupted");
    assert_eq!(untouched["pid"], shadow_before["pid"]);
    assert_eq!(untouched["id"], shadow_before["id"]);
    let claims: Vec<String> = probe
        .prepare("SELECT slot_id FROM picker_claim_probe")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(!claims.is_empty(), "真正恢复必须经过 claim");
    assert!(
        claims.iter().all(|id| id == before["id"].as_str().unwrap()),
        "{claims:?}"
    );
    assert_eq!(restored["session_id"], original);
    assert_eq!(restored["id"], before["id"]);
    assert_eq!(restored["name"], "picker-original");
    assert_eq!(restored["status"], "running");
    assert_ne!(restored["pid"], before["pid"]);
    let target = restored["tmux"]["session_id"].as_str().unwrap();
    let socket = restored["tmux"]["socket"].as_str().unwrap();
    let attached = std::fs::read(&attach_log).unwrap();
    let words: Vec<_> = attached
        .split(|byte| *byte == 0)
        .filter(|word| !word.is_empty())
        .map(|word| String::from_utf8(word.to_vec()).unwrap())
        .collect();
    assert_eq!(words, ["-S", socket, "switch-client", "-t", target]);
    let command = w.tmux(&["list-panes", "-t", target, "-F", "#{pane_start_command}"]);
    assert!(
        command.contains(&format!("--resume {original}")),
        "{command}"
    );
    assert!(!command.contains("--session-id"), "{command}");
    let all = w.tmux(&["list-panes", "-a", "-F", "#{pane_start_command}"]);
    assert!(!all.contains(&format!("--resume {shadow_id}")), "{all}");
    assert_eq!(
        all.matches(&format!("--resume {original}")).count(),
        1,
        "{all}"
    );
}
