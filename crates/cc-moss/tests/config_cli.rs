//! `cc-moss config get|set` against a throwaway config file: no test reads or
//! writes the real `~/.config`.

mod common;

use std::fs;
use std::path::PathBuf;
use std::process::Output;

struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new() -> Env {
        Env {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    /// Not created up front: `set` must make the file and its folder.
    fn config(&self) -> PathBuf {
        self.dir.path().join("conf/cc-moss/config.toml")
    }

    fn write_config(&self, text: &str) {
        fs::create_dir_all(self.config().parent().unwrap()).unwrap();
        fs::write(self.config(), text).unwrap();
    }

    fn read_config(&self) -> String {
        fs::read_to_string(self.config()).unwrap_or_default()
    }

    fn cc(&self, args: &[&str]) -> Output {
        common::bin()
            .env("HOME", self.dir.path().join("home"))
            .env("CC_MOSS_CONFIG", self.config())
            .env("CC_MOSS_STATE_DIR", self.dir.path().join("state"))
            .args(args)
            .output()
            .unwrap()
    }

    fn json(&self, args: &[&str]) -> (bool, serde_json::Value) {
        let out = self.cc(args);
        let v = serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("{args:?}: not JSON ({e}): {}", text(&out)));
        (out.status.success(), v)
    }
}

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

fn ok(o: Output) -> String {
    assert!(o.status.success(), "{}", text(&o));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn failed(o: Output) -> String {
    assert!(!o.status.success(), "should fail: {}", text(&o));
    text(&o)
}

#[test]
fn get_defaults_to_auto() {
    let env = Env::new();
    assert_eq!(ok(env.cc(&["config", "get", "ui.language"])), "auto");
    assert!(!env.config().exists(), "get must not create the file");
    env.write_config("[ui]\nicons = \"text\"\n");
    assert_eq!(ok(env.cc(&["config", "get", "ui.language"])), "auto");
}

#[test]
fn set_then_get() {
    let env = Env::new();
    ok(env.cc(&["config", "set", "ui.language", "ZH"]));
    assert!(
        env.read_config().contains("language = \"zh\""),
        "{}",
        env.read_config()
    );
    assert_eq!(ok(env.cc(&["config", "get", "ui.language"])), "zh");
    ok(env.cc(&["config", "set", "ui.language", "en"]));
    assert_eq!(ok(env.cc(&["config", "get", "ui.language"])), "en");
    ok(env.cc(&["config", "set", "ui.language", "auto"]));
    assert_eq!(ok(env.cc(&["config", "get", "ui.language"])), "auto");
}

#[test]
fn keeps_comments_and_other_tables() {
    let env = Env::new();
    let before = "\
# my cc-moss settings
[claude]
default_args = [\"--model\", \"opus\"] # always opus

[ui]
# icons stay plain
icons = \"text\"
language = \"en\" # was english

[aliases]
tn = \"new\"
";
    env.write_config(before);
    ok(env.cc(&["config", "set", "ui.language", "zh"]));
    let after = env.read_config();
    assert_eq!(
        after,
        before.replace("language = \"en\"", "language = \"zh\""),
        "{after}"
    );
    // No temp file left next to it.
    let names: Vec<_> = fs::read_dir(env.config().parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 1, "{names:?}");
}

#[test]
fn unknown_key_is_refused() {
    let env = Env::new();
    env.write_config("[ui]\nicons = \"text\"\n");
    let msg = failed(env.cc(&["config", "set", "ui.icons", "nerd"]));
    assert!(msg.contains("ui.language"), "lists the known keys: {msg}");
    assert_eq!(env.read_config(), "[ui]\nicons = \"text\"\n");
    let msg = failed(env.cc(&["config", "get", "tmux.socket"]));
    assert!(msg.contains("ui.language"), "{msg}");
}

#[test]
fn bad_value_is_refused() {
    let env = Env::new();
    let msg = failed(env.cc(&["config", "set", "ui.language", "fr"]));
    for v in ["auto", "zh", "en"] {
        assert!(msg.contains(v), "names {v}: {msg}");
    }
    assert!(!env.config().exists());
}

#[test]
fn json_output() {
    let env = Env::new();
    let (good, v) = env.json(&["config", "get", "ui.language", "--json"]);
    assert!(good);
    assert_eq!(v["schema"], 1);
    assert_eq!(v["ok"], true);
    assert_eq!(v["key"], "ui.language");
    assert_eq!(v["value"], "auto");

    let (good, v) = env.json(&["config", "set", "ui.language", "Zh", "--json"]);
    assert!(good);
    assert_eq!(v["schema"], 1);
    assert_eq!(v["key"], "ui.language");
    assert_eq!(v["value"], "zh");
    let (_, v) = env.json(&["config", "get", "--json", "ui.language"]);
    assert_eq!(v["value"], "zh");

    let (good, v) = env.json(&["config", "set", "ui.language", "fr", "--json"]);
    assert!(!good);
    assert_eq!(v["schema"], 1);
    assert_eq!(v["ok"], false);
    assert_eq!(v["code"], "bad_value");
    assert!(v["error"].as_str().unwrap().contains("auto"));

    let (good, v) = env.json(&["config", "get", "nope", "--json"]);
    assert!(!good);
    assert_eq!(v["code"], "unknown_key");
}

#[test]
fn update_check_reads_but_never_sets() {
    let env = Env::new();
    assert_eq!(ok(env.cc(&["config", "get", "update.check"])), "true");
    env.write_config("[update]\ncheck = false\n");
    let (good, v) = env.json(&["config", "get", "update.check", "--json"]);
    assert!(good);
    assert_eq!(v["value"], "false");

    let (good, v) = env.json(&["config", "set", "update.check", "true", "--json"]);
    assert!(!good);
    assert_eq!(v["code"], "read_only");
    assert_eq!(env.read_config(), "[update]\ncheck = false\n");
}
