//! `cc-moss doctor`: check every piece cc-moss installs, and say how to fix
//! what is off. Exits non-zero when something is broken.

use std::io::IsTerminal;

use anyhow::Result;
use moss_core::managed_block;
use moss_core::platform::{on_path, Tool};

use super::files::read_optional;
use super::install::{self, Paths};
use super::setup;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Ok,
    /// Fine as it is; worth knowing.
    Note,
    /// Not installed or out of date; nothing is broken.
    Warn,
    /// Broken: doctor exits non-zero.
    Fail,
}

struct Check {
    mark: Mark,
    what: String,
    detail: String,
    fix: Option<String>,
}

fn check(mark: Mark, what: &str, detail: impl Into<String>, fix: Option<String>) -> Check {
    Check {
        mark,
        what: what.to_string(),
        detail: detail.into(),
        fix,
    }
}

/// True when nothing is broken.
pub fn run() -> Result<bool> {
    let checks = checks()?;
    let color = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    println!("{}", tr!("cc-moss doctor", "cc-moss 体检"));
    for c in &checks {
        let (sign, code) = match c.mark {
            Mark::Ok => ("✓", "32"),
            Mark::Note => ("·", "2"),
            Mark::Warn => ("!", "33"),
            Mark::Fail => ("✗", "31"),
        };
        let sign = if color {
            format!("\x1b[{code}m{sign}\x1b[0m")
        } else {
            sign.to_string()
        };
        println!("  {sign} {} {}", super::pad(&c.what, 18), c.detail);
        if let Some(fix) = &c.fix {
            println!("      → {fix}");
        }
    }
    let broken = checks.iter().filter(|c| c.mark == Mark::Fail).count();
    let todo = checks.iter().filter(|c| c.mark == Mark::Warn).count();
    println!();
    println!(
        "{}",
        match (broken, todo) {
            (0, 0) => tr!(
                "Everything looks healthy. MOSS approves.".to_string(),
                "一切正常，MOSS 很满意。".to_string()
            ),
            (0, n) => tr!(
                if n == 1 {
                    "Nothing broken; one thing could be better.".to_string()
                } else {
                    format!("Nothing broken; {n} things could be better.")
                },
                format!("没有坏掉的；有 {n} 处可以更好。")
            ),
            (n, _) => tr!(
                if n == 1 {
                    "One thing needs fixing.".to_string()
                } else {
                    format!("{n} things need fixing.")
                },
                format!("有 {n} 处需要修。")
            ),
        }
    );
    Ok(broken == 0)
}

/// How to install `tools`: the commands for this system, else in words.
fn install_hint(tools: &[Tool]) -> Option<String> {
    let cmds = install::commands_for(tools);
    Some(if cmds.is_empty() {
        install::manual_install_hint().to_string()
    } else {
        cmds.join(" && ")
    })
}

fn checks() -> Result<Vec<Check>> {
    let paths = Paths::detect()?;
    let zshrc = read_optional(&paths.zshrc)?.unwrap_or_default();
    let zshenv = read_optional(&paths.zshenv)?.unwrap_or_default();
    let has = managed_block::contains;
    let fix_install = Some("cc-moss install".to_string());
    let fix_setup = Some("cc-moss setup".to_string());
    let zsh = install::zsh_in_use(&paths);
    let shell = install::login_shell().unwrap_or_default();
    let skipped = |what: &str| {
        check(
            Mark::Note,
            what,
            tr!(
                format!("skipped: zsh only (your shell is {shell})"),
                format!("跳过：只支持 zsh（你用的是 {shell}）")
            ),
            None,
        )
    };
    let mut out = vec![version_check()];

    // Claude Code and the hooks that register its sessions.
    out.push(if on_path(&["claude"]) {
        check(Mark::Ok, "Claude Code", tr!("installed", "已安装"), None)
    } else {
        check(
            Mark::Fail,
            "Claude Code",
            tr!("not found", "没找到"),
            Some("https://code.claude.com/docs".into()),
        )
    });
    out.push(match setup::hook_status() {
        Ok(setup::HookStatus::Current) => check(
            Mark::Ok,
            tr!("hooks", "钩子"),
            tr!("installed", "已安装"),
            None,
        ),
        Ok(setup::HookStatus::Outdated) => check(
            Mark::Fail,
            tr!("hooks", "钩子"),
            tr!(
                "missing or outdated: new sessions are not recorded",
                "没装或已过期：新会话不会被登记"
            ),
            fix_setup.clone(),
        ),
        Ok(setup::HookStatus::Incomplete(missing)) => check(
            Mark::Fail,
            tr!("hooks", "钩子"),
            tr!(
                format!(
                    "{} hook(s) missing ({}): session events are incomplete",
                    missing.len(),
                    missing.join(", ")
                ),
                format!(
                    "少了 {} 个钩子（{}）：会话事件不完整",
                    missing.len(),
                    missing.join("、")
                )
            ),
            fix_setup.clone(),
        ),
        Ok(setup::HookStatus::Broken { path, missing }) => check(
            Mark::Fail,
            tr!("hooks", "钩子"),
            if missing {
                tr!(
                    format!("run {path}, which is missing: new sessions are not recorded"),
                    format!("指向的 {path} 不见了：新会话不会被登记")
                )
            } else {
                tr!(
                    format!("run {path}, which is not executable: new sessions are not recorded"),
                    format!("指向的 {path} 不能执行：新会话不会被登记")
                )
            },
            fix_setup.clone(),
        ),
        Err(e) => check(Mark::Fail, tr!("hooks", "钩子"), format!("{e:#}"), None),
    });
    if let Some(n) = sessions_without_pid().filter(|n| *n > 0) {
        out.push(check(
            Mark::Warn,
            tr!("process ids", "进程号"),
            tr!(
                if n == 1 {
                    "1 recent session was recorded without a process id; crash recovery cannot see it"
                        .to_string()
                } else {
                    format!("{n} recent sessions were recorded without a process id; crash recovery cannot see them")
                },
                format!("{n} 个最近的会话没有记下进程号，中断后没法找回它们")
            ),
            Some(
                tr!(
                    "update Claude Code: cc-moss reads the CLAUDE_PID it sets for hooks",
                    "升级 Claude Code：cc-moss 靠它给钩子的 CLAUDE_PID 认进程"
                )
                .into(),
            ),
        ));
    }
    out.push(if !zsh {
        skipped("t")
    } else if has(&zshrc, setup::ZSH_TAG) {
        check(Mark::Ok, "t", tr!("ready", "可用"), None)
    } else {
        check(
            Mark::Warn,
            "t",
            tr!("not set up", "还没设置"),
            fix_setup.clone(),
        )
    });

    // tmux and its configuration.
    out.push(if on_path(&["tmux"]) {
        let version = tmux_version();
        match version {
            Some(v) if v < (3, 2) => check(
                Mark::Warn,
                "tmux",
                tr!(
                    format!(
                        "{}.{}: prefix + t needs 3.2 or newer; everything else works",
                        v.0, v.1
                    ),
                    format!("{}.{}：prefix + t 需要 3.2 及以上，其他都能用", v.0, v.1)
                ),
                install_hint(&[Tool::Tmux]),
            ),
            Some(v) => check(Mark::Ok, "tmux", format!("{}.{}", v.0, v.1), None),
            None => check(Mark::Ok, "tmux", tr!("installed", "已安装"), None),
        }
    } else {
        check(
            Mark::Fail,
            "tmux",
            tr!("not found", "没找到"),
            install_hint(&[Tool::Tmux]),
        )
    });
    let own_tmux = read_optional(&paths.tmux_conf)?.unwrap_or_default();
    let moss_tmux = read_optional(&paths.moss_tmux_conf)?;
    out.push(match (has(&own_tmux, install::TAG_TMUX), moss_tmux) {
        (false, _) => check(
            Mark::Warn,
            tr!("tmux config", "tmux 配置"),
            tr!("not installed", "还没装"),
            fix_install.clone(),
        ),
        (true, None) => check(
            Mark::Fail,
            tr!("tmux config", "tmux 配置"),
            tr!(
                format!("{} is missing", paths.moss_tmux_conf.display()),
                format!("{} 不见了", paths.moss_tmux_conf.display())
            ),
            fix_install.clone(),
        ),
        (true, Some(t)) if t != install::rendered_tmux_conf(&paths) => check(
            Mark::Warn,
            tr!("tmux config", "tmux 配置"),
            tr!("out of date", "不是最新"),
            fix_install.clone(),
        ),
        _ => check(
            Mark::Ok,
            tr!("tmux config", "tmux 配置"),
            tr!("installed", "已安装"),
            None,
        ),
    });

    // starship: the binary, its config, and running last (pitfall #8).
    // A hand-written `starship init` errors on every shell start without
    // starship; cc-moss's prompt block checks first and just does nothing.
    let by_hand = install::starts_starship_by_hand(&zshrc);
    let ours = has(&zshrc, install::TAG_PROMPT);
    out.push(match (install::installed(Tool::Starship), by_hand, ours) {
        (true, _, _) => check(Mark::Ok, "starship", tr!("installed", "已安装"), None),
        (false, true, _) => check(
            Mark::Fail,
            "starship",
            tr!(
                "started in .zshrc but not installed",
                ".zshrc 里启用了，但没安装"
            ),
            install_hint(&[Tool::Starship]),
        ),
        (false, false, true) => check(
            Mark::Warn,
            "starship",
            tr!(
                "not installed: plain prompt for now",
                "还没装：暂时是普通提示符"
            ),
            install_hint(&[Tool::Starship]),
        ),
        (false, false, false) => check(
            Mark::Warn,
            "starship",
            tr!("not installed", "还没装"),
            fix_install.clone(),
        ),
    });
    out.push(match read_optional(&paths.starship)? {
        None => check(
            Mark::Warn,
            tr!("starship config", "starship 配置"),
            tr!("none yet", "还没有"),
            fix_install.clone(),
        ),
        Some(t) if t == install::STARSHIP_TOML => check(
            Mark::Ok,
            tr!("starship config", "starship 配置"),
            tr!("cc-moss's", "cc-moss 的"),
            None,
        ),
        Some(_) => check(
            Mark::Note,
            tr!("starship config", "starship 配置"),
            tr!("your own (kept as is)", "你自己的（原样保留）"),
            None,
        ),
    });
    let late = blocks_after_starship(&zshrc);
    out.push(if !zsh {
        skipped(tr!("prompt order", "提示符顺序"))
    } else if late.is_empty() {
        check(
            Mark::Ok,
            tr!("prompt order", "提示符顺序"),
            tr!("starship runs last", "starship 最后执行"),
            None,
        )
    } else {
        check(
            Mark::Fail,
            tr!("prompt order", "提示符顺序"),
            tr!(
                format!("after starship init: {}", late.join(", ")),
                format!("在 starship init 之后：{}", late.join("、"))
            ),
            fix_install.clone(),
        )
    });

    // Shell blocks: ssh reset (pitfall #13), PATH for scripts (pitfall #12).
    if !zsh {
        out.push(skipped(tr!("shell blocks", "shell 配置块")));
        return Ok(out);
    }
    out.push(if has(&zshrc, install::TAG_SSH) {
        check(
            Mark::Ok,
            tr!("ssh reset", "ssh 复位"),
            tr!("installed", "已安装"),
            None,
        )
    } else {
        check(
            Mark::Warn,
            tr!("ssh reset", "ssh 复位"),
            tr!(
                "a dropped ssh can leave the terminal in mouse mode",
                "ssh 断线后终端可能卡在鼠标上报模式"
            ),
            fix_install.clone(),
        )
    });
    out.push(if has(&zshenv, install::TAG_PATH) {
        check(
            Mark::Ok,
            tr!("script PATH", "脚本 PATH"),
            tr!("installed", "已安装"),
            None,
        )
    } else {
        check(
            Mark::Warn,
            tr!("script PATH", "脚本 PATH"),
            tr!(
                "`ssh host cmd` may not find Homebrew tools",
                "`ssh 主机 命令` 可能找不到 Homebrew 装的工具"
            ),
            fix_install.clone(),
        )
    });

    // Aliases: optional; their tools, and hand-written twins (pitfall #10).
    if has(&zshrc, install::TAG_ALIASES) {
        let twins = aliases_outside_block(&zshrc);
        out.push(if twins.is_empty() {
            check(
                Mark::Ok,
                tr!("aliases", "别名"),
                tr!("installed", "已安装"),
                None,
            )
        } else {
            check(
                Mark::Warn,
                tr!("aliases", "别名"),
                tr!(
                    format!("also defined by hand: {}", twins.join(", ")),
                    format!("手写的同名别名：{}", twins.join("、"))
                ),
                Some(
                    tr!(
                        "remove the hand-written ones so the two do not fight",
                        "删掉手写的那些，免得两边打架"
                    )
                    .into(),
                ),
            )
        });
        let missing: Vec<Tool> = install::wanted_tools(false, false, true)
            .into_iter()
            .filter(|t| !install::installed(*t))
            .collect();
        if !missing.is_empty() {
            let names: Vec<&str> = missing.iter().map(|t| t.name()).collect();
            out.push(check(
                Mark::Note,
                tr!("alias tools", "别名工具"),
                tr!(
                    format!("not installed: {}", names.join(", ")),
                    format!("没装：{}", names.join("、"))
                ),
                install_hint(&missing),
            ));
        }
    } else {
        out.push(check(
            Mark::Note,
            tr!("aliases", "别名"),
            tr!("optional, not installed", "可选，没装"),
            Some("cc-moss install --with aliases".into()),
        ));
    }
    Ok(out)
}

/// This version, and whether a newer one is out. Doctor is interactive, so
/// it may ask GitHub (briefly, once a day) unless checking is turned off.
fn version_check() -> Check {
    use moss_core::update::{self, Version};
    let what = tr!("version", "版本");
    let current = Version::current();
    let config = moss_core::config::Config::load().unwrap_or_default();
    if !update::enabled_here(&config) {
        return check(
            Mark::Note,
            what,
            tr!(
                format!("{current} (new-version check skipped: turned off)"),
                format!("{current}（新版本检查已关闭，跳过）")
            ),
            None,
        );
    }
    match update::latest_checking_if_stale(update::QUICK_TIMEOUT) {
        Some(latest) if latest.is_newer_than(&current) => check(
            Mark::Warn,
            what,
            tr!(
                format!("{current}; {latest} is out"),
                format!("{current}；{latest} 已经发布")
            ),
            Some("cc-moss update".into()),
        ),
        Some(_) => check(
            Mark::Ok,
            what,
            tr!(
                format!("{current}, the latest"),
                format!("{current}，已是最新")
            ),
            None,
        ),
        None => check(
            Mark::Note,
            what,
            tr!(
                format!("{current} (could not check for a newer one)"),
                format!("{current}（没能检查有没有新版本）")
            ),
            None,
        ),
    }
}

/// Sessions active in the last week that the hooks recorded without a pid
/// (Claude Code did not set `CLAUDE_PID`). Without a pid a crashed session
/// looks like it is still running, so restore never offers it. `None` when
/// there is no registry or it cannot be read; doctor creates nothing.
fn sessions_without_pid() -> Option<usize> {
    const WEEK_MS: i64 = 7 * 24 * 60 * 60 * 1000;
    let path = moss_core::store_dir::registry_path();
    if !path.exists() {
        return None;
    }
    let since = moss_core::slot::now_ms() - WEEK_MS;
    let slots = moss_core::registry::Registry::open(&path)
        .ok()?
        .slots()
        .ok()?;
    Some(
        slots
            .iter()
            .filter(|s| s.pid.is_none() && s.last_active_at_ms >= since)
            .count(),
    )
}

/// `tmux -V` as (major, minor); `3.5a` reads as (3, 5).
fn tmux_version() -> Option<(u32, u32)> {
    let out = std::process::Command::new("tmux").arg("-V").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let v = text.split_whitespace().nth(1)?;
    let mut parts = v.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor: String = parts
        .next()
        .unwrap_or("0")
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    Some((major, minor.parse().unwrap_or(0)))
}

/// cc-moss's `.zshrc` blocks that open after `starship init`.
fn blocks_after_starship(zshrc: &str) -> Vec<String> {
    let Some(init) = managed_block::starship_init_line(zshrc) else {
        return Vec::new();
    };
    managed_block::tags(zshrc)
        .into_iter()
        .filter(|(line, tag)| *line > init && tag != install::TAG_PROMPT && install::is_ours(tag))
        .map(|(_, tag)| tag)
        .collect()
}

/// Names the aliases block defines that `.zshrc` also defines elsewhere.
fn aliases_outside_block(zshrc: &str) -> Vec<String> {
    let names = alias_names(install::ALIASES_ZSH);
    let outside = managed_block::remove(zshrc, install::TAG_ALIASES).unwrap_or_default();
    let theirs = alias_names(&outside);
    names.into_iter().filter(|n| theirs.contains(n)).collect()
}

/// Every alias name defined in `text`, wherever `alias` appears (also after
/// `then`, `&&` or `;`), sorted and without repeats.
fn alias_names(text: &str) -> Vec<String> {
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|' | '{' | '('))
        .filter(|w| !w.is_empty())
        .collect();
    let names: std::collections::BTreeSet<String> = words
        .windows(2)
        .filter(|w| w[0] == "alias")
        .filter_map(|w| {
            let (name, _) = w[1].split_once('=')?;
            (!name.is_empty()).then(|| name.to_string())
        })
        .collect();
    names.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_blocks_after_starship() {
        let rc = "# >>> cc-moss: ssh-tty-restore >>>\n# <<< cc-moss: ssh-tty-restore <<<\n\
                  eval \"$(starship init zsh)\"\n# >>> cc-moss >>>\n# <<< cc-moss <<<\n";
        assert_eq!(blocks_after_starship(rc), ["cc-moss"]);
        let other = "eval \"$(starship init zsh)\"\n# >>> conda >>>\n# <<< conda <<<\n";
        assert!(blocks_after_starship(other).is_empty(), "not ours");
        assert!(blocks_after_starship("# starship init is described here\n").is_empty());
    }

    #[test]
    fn finds_hand_written_twins() {
        let rc = "alias ll='ls -lh'\nalias x=1\n# >>> cc-moss: aliases >>>\nalias ll='ls -l'\n# <<< cc-moss: aliases <<<\n";
        assert_eq!(aliases_outside_block(rc), ["ll"]);
    }

    #[test]
    fn finds_aliases_after_then_and_and() {
        let names = alias_names(super::install::ALIASES_ZSH);
        for want in ["ll", "catp", "catn", "ff", "psp", "lg"] {
            assert!(names.contains(&want.to_string()), "{want}: {names:?}");
        }
        assert_eq!(
            names.iter().filter(|n| *n == "catp").count(),
            1,
            "no repeats"
        );
    }
}
