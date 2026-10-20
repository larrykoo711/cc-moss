//! `cc-moss doctor`: check every piece cc-moss installs, and say how to fix
//! what is off. Exits non-zero when something is broken.

use std::io::IsTerminal;

use anyhow::Result;
use moss_core::managed_block;
use moss_core::platform::{on_path, Tool};

use super::files::read_optional;
use super::install::{self, Paths};
use super::{alias, app, daemon, setup};

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

/// True when nothing is broken. `fix` first recreates alias links that are
/// missing or broken (never touching a file that is not cc-moss's).
pub fn run(fix: bool) -> Result<bool> {
    if fix {
        let place = alias::Place::detect()?;
        let config = moss_core::config::Config::load()?;
        let done = alias::fix_links(&place, &config);
        if done.is_empty() {
            println!(
                "{}",
                tr!("No alias links to fix.", "没有需要修复的别名链接。")
            );
        }
        for line in &done {
            println!("{line}");
        }
        println!();
    }
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

/// How `cc-moss new` starts sessions: bypass by default, normal mode when
/// the config says so or a Claude Code setting turns bypass off.
fn permission_check() -> Check {
    use moss_core::config::{Config, PermissionMode};
    let what = tr!("new sessions", "新会话");
    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => return check(Mark::Fail, what, format!("{e:#}"), None),
    };
    if config.claude.permission_mode == PermissionMode::Default {
        return check(
            Mark::Ok,
            what,
            tr!(
                "normal mode: Claude Code asks before it acts",
                "普通模式：Claude Code 动手前会先问你"
            ),
            None,
        );
    }
    if let Some(path) = setup::bypass_blocked_by() {
        return check(
            Mark::Note,
            what,
            tr!(
                format!(
                    "bypass is turned off by a policy in {}; new sessions start in normal mode",
                    path.display()
                ),
                format!(
                    "{} 里的管理策略禁用了 bypass，新会话按普通模式开",
                    path.display()
                )
            ),
            None,
        );
    }
    check(
        Mark::Ok,
        what,
        tr!(
            "bypass: they don't stop to ask for permission",
            "bypass：不再逐条问你权限"
        ),
        None,
    )
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

/// The API provider Claude Code's settings name, and whether running
/// sessions still use an older one (T2.6). Folders whose own settings pick
/// a provider are pointed out: a switch doesn't reach them.
fn provider_check() -> Option<Check> {
    let now = super::provider::current()?;
    // A model alone (`/model` writes one) is still your Claude account.
    let own_key = ["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY", "apiKeyHelper"]
        .iter()
        .any(|k| now.keys.contains(*k));
    let mut detail = match &now.host {
        Some(host) => tr!(format!("sessions use {host}"), format!("会话连的是 {host}")),
        None if own_key => tr!(
            "sessions use an API key of your own",
            "会话用的是你自己的 API key"
        )
        .into(),
        None => tr!(
            "sessions use your Claude account",
            "会话用的是你的 Claude 账号"
        )
        .into(),
    };
    let path = moss_core::store_dir::registry_path();
    let slots = if path.exists() {
        moss_core::registry::Registry::open(&path)
            .and_then(|r| r.slots())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let running: Vec<_> = slots
        .iter()
        .filter(|s| !s.is_moss())
        .filter(|s| {
            s.effective_status(moss_core::proc::is_alive) == moss_core::slot::Status::Running
        })
        .collect();
    let stale = running
        .iter()
        .filter(|s| {
            super::provider::restart_reason(s, moss_core::slot::Status::Running, Some(&now))
                .is_some()
        })
        .count();
    let own: std::collections::BTreeSet<&str> = running
        .iter()
        .map(|s| s.cwd.as_str())
        .filter(|cwd| {
            ["settings.json", "settings.local.json"].iter().any(|f| {
                moss_core::provider::Snapshot::read(
                    &std::path::Path::new(cwd).join(".claude").join(f),
                )
                .is_some_and(|s| !s.keys.is_empty())
            })
        })
        .collect();
    if !own.is_empty() {
        detail.push_str(&tr!(
            format!(
                "; {} folder(s) pick their own, a switch doesn't reach them",
                own.len()
            ),
            format!("；{} 个目录自己指定了供应商，切换对它们不生效", own.len())
        ));
    }
    // Sessions whose Claude Code reads another settings folder than the
    // daemon (a CLAUDE_CONFIG_DIR of their own): a switch has to happen there.
    let daemon_reads = super::provider::settings_path();
    let elsewhere = running
        .iter()
        .filter(|s| {
            super::provider::settings_of(s).is_some_and(|p| Some(&p) != daemon_reads.as_ref())
        })
        .count();
    let mut mark = Mark::Note;
    if elsewhere > 0 {
        mark = Mark::Warn;
        detail.push_str(&tr!(
            format!(
                "; {elsewhere} session(s) read another Claude Code settings folder than cc-moss does; set [claude] config_dir to the folder you switch in"
            ),
            format!(
                "；{elsewhere} 个会话读的 Claude Code 设置目录和 cc-moss 看的不一样；把 [claude] config_dir 设成你切换供应商的那个目录"
            )
        ));
    }
    Some(if stale > 0 {
        detail.push_str(&tr!(
            format!("; {stale} session(s) need a restart to use it"),
            format!("；{stale} 个会话要重开才能用上")
        ));
        check(
            Mark::Warn,
            tr!("API provider", "API 供应商"),
            detail,
            Some("cc-moss restart --stale".into()),
        )
    } else {
        check(mark, tr!("API provider", "API 供应商"), detail, None)
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
    out.extend(app_check());
    out.extend(daemon_check());
    out.extend(super::moss::facts().map(|f| moss_check(&f)));
    out.extend(provider_check());

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
        // Only the question hooks: sessions are still recorded, they just
        // can't be answered from elsewhere until the next install.
        Ok(setup::HookStatus::Incomplete(missing))
            if missing
                .iter()
                .all(|m| matches!(*m, "PreToolUse" | "PermissionRequest")) =>
        {
            check(
                Mark::Warn,
                tr!("hooks", "钩子"),
                tr!(
                    "the hooks for answering a session's questions from elsewhere aren't set up yet",
                    "替会话接住问题的钩子还没装上"
                ),
                fix_setup.clone(),
            )
        }
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
    out.push(permission_check());
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
    out.extend(alias_checks(&zshrc, zsh, &zshenv));

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
                tr!("shell aliases", "shell 别名"),
                tr!("installed", "已安装"),
                None,
            )
        } else {
            check(
                Mark::Warn,
                tr!("shell aliases", "shell 别名"),
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
            tr!("shell aliases", "shell 别名"),
            tr!("optional, not installed", "可选，没装"),
            Some("cc-moss install --with aliases".into()),
        ));
    }
    Ok(out)
}

/// `t` and every other native alias: its link, its command, whether the
/// shell hides it, and whether `~/.local/bin` is on `PATH`.
fn alias_checks(zshrc: &str, zsh: bool, zshenv: &str) -> Vec<Check> {
    let mut out = Vec::new();
    let place = match alias::Place::detect() {
        Ok(p) => p,
        Err(e) => return vec![check(Mark::Fail, "t", format!("{e:#}"), None)],
    };
    let config = match moss_core::config::Config::load() {
        Ok(c) => c,
        Err(e) => {
            return vec![check(
                Mark::Fail,
                tr!("aliases", "别名"),
                format!("{e:#}"),
                None,
            )]
        }
    };
    let rows = alias::rows(&place, &config, true);
    let ignored = read_optional(&place.config)
        .ok()
        .flatten()
        .map(|t| moss_core::aliases::ignored_entries(&t))
        .unwrap_or_default();
    if !ignored.is_empty() {
        out.push(check(
            Mark::Warn,
            tr!("aliases", "别名"),
            tr!(
                format!(
                    "skipped in {}: {} (an alias is a string, e.g. t = \"pick\")",
                    place.config.display(),
                    ignored.join(", ")
                ),
                format!(
                    "{} 里跳过了 {}（别名要写成字符串，比如 t = \"pick\"）",
                    place.config.display(),
                    ignored.join("、")
                )
            ),
            Some(format!(
                "cc-moss alias add {} <{}>",
                ignored[0],
                tr!("command", "命令")
            )),
        ));
    }
    let fix_doctor = Some("cc-moss doctor --fix".to_string());
    if !rows.iter().any(|r| r.name == alias::DEFAULT_NAME) {
        out.push(check(
            Mark::Warn,
            "t",
            tr!("not set up", "还没设置"),
            Some("cc-moss setup".into()),
        ));
    }
    // The 0.1 `t` was a zsh function in cc-moss's block; it hides the link.
    if zsh && managed_block::contains(zshrc, setup::ZSH_TAG) {
        out.push(check(
            Mark::Warn,
            "t",
            tr!(
                "still the old zsh function from cc-moss 0.1",
                "还是 cc-moss 0.1 的旧 zsh 函数"
            ),
            Some("cc-moss setup".into()),
        ));
    }
    for row in &rows {
        let what = row.name.clone();
        let runs = format!("→ {}", row.command);
        out.push(match row.problem(&place) {
            None => check(Mark::Ok, &what, runs, None),
            Some((detail, fix)) => {
                let mark = match row.status() {
                    "invalid" | "broken" | "foreign" => Mark::Fail,
                    _ => Mark::Warn,
                };
                let fix = match row.status() {
                    "missing" | "broken" | "stale" => fix_doctor.clone(),
                    _ => Some(fix),
                };
                check(mark, &what, format!("{runs}: {detail}"), fix)
            }
        });
    }
    if !rows.is_empty() && !place.dir_on_path() {
        // install's .zshenv block puts ~/.local/bin on PATH in new shells.
        out.push(
            if zsh && managed_block::contains(zshenv, install::TAG_PATH) {
                check(
                    Mark::Note,
                    tr!("alias PATH", "别名 PATH"),
                    tr!(
                        format!(
                            "{} joins PATH in new terminals (~/.zshenv)",
                            place.dir.display()
                        ),
                        format!(
                            "新开的终端会把 {} 加进 PATH（~/.zshenv）",
                            place.dir.display()
                        )
                    ),
                    None,
                )
            } else {
                check(
                    Mark::Warn,
                    tr!("alias PATH", "别名 PATH"),
                    tr!(
                        format!(
                            "{} is not on PATH: your shell cannot find the aliases",
                            place.dir.display()
                        ),
                        format!("{} 不在 PATH 里：shell 找不到这些别名", place.dir.display())
                    ),
                    Some(if zsh {
                        "cc-moss install".to_string()
                    } else {
                        tr!(
                        "add `export PATH=\"$HOME/.local/bin:$PATH\"` to your shell's startup file",
                        "在 shell 启动文件里加一行 `export PATH=\"$HOME/.local/bin:$PATH\"`"
                    )
                        .to_string()
                    }),
                )
            },
        );
    }
    out
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

/// The resident daemon (macOS): running, on this version.
fn daemon_check() -> Option<Check> {
    if !daemon::applies() || app::as_root() {
        return None;
    }
    let what = tr!("daemon", "守护进程");
    let r = daemon::report();
    let current = env!("CARGO_PKG_VERSION");
    Some(if !r.running {
        if daemon::plist_present() {
            check(
                Mark::Warn,
                what,
                tr!("not running", "没在运行"),
                Some("cc-moss daemon start".into()),
            )
        } else {
            check(
                Mark::Warn,
                what,
                tr!("not set up", "还没设置"),
                Some("cc-moss install".into()),
            )
        }
    } else if r.version.as_deref() != Some(current) {
        let have = r.version.clone().unwrap_or_else(|| "?".into());
        check(
            Mark::Warn,
            what,
            tr!(
                format!("running {have}, not {current}"),
                format!("跑的是 {have}，不是 {current}")
            ),
            Some("cc-moss install".into()),
        )
    } else {
        let up = daemon::uptime(&r).unwrap_or_else(|| "?".into());
        check(
            Mark::Ok,
            what,
            tr!(format!("running for {up}"), format!("运行中，已 {up}")),
            None,
        )
    })
}

/// Moss (macOS): on and running, and what the daemon needs to start it.
fn moss_check(f: &super::moss::Facts) -> Check {
    let what = "Moss";
    if !f.enabled {
        return check(
            Mark::Note,
            what,
            tr!("off", "已关闭"),
            Some("cc-moss moss start".into()),
        );
    }
    let missing: Vec<&str> = [("claude", f.claude_found), ("tmux", f.tmux_found)]
        .iter()
        .filter(|(_, found)| !found)
        .map(|(name, _)| *name)
        .collect();
    if !missing.is_empty() {
        let names = missing.join(", ");
        return check(
            Mark::Warn,
            what,
            tr!(
                format!("the daemon can't find {names}, so it can't start Moss"),
                format!("守护进程找不到 {names}，所以没法启动 Moss")
            ),
            Some(tr!(
                format!("install {names}, then run `cc-moss install` so the daemon sees it"),
                format!("先装好 {names}，再运行 `cc-moss install` 让守护进程看得到它")
            )),
        );
    }
    if let Some(dir) = &f.fence_gap {
        let dir = dir.display();
        return check(
            Mark::Warn,
            what,
            tr!(
                format!("CLAUDE_CONFIG_DIR is {dir} here, but Moss isn't kept out of it"),
                format!("这里的 CLAUDE_CONFIG_DIR 是 {dir}，但 Moss 没被挡在外面")
            ),
            Some(tr!(
                format!("add `config_dir = \"{dir}\"` under [claude] in config.toml, then run `cc-moss install`"),
                format!("在 config.toml 的 [claude] 下加 `config_dir = \"{dir}\"`，再运行 `cc-moss install`")
            )),
        );
    }
    if f.running {
        check(Mark::Ok, what, tr!("running", "运行中"), None)
    } else {
        check(
            Mark::Warn,
            what,
            tr!("on, but not running", "已打开，但没在运行"),
            Some("cc-moss daemon start".into()),
        )
    }
}

/// The menubar app (macOS only): in an Applications folder, where its
/// notifications work, and the same version as this cc-moss.
fn app_check() -> Option<Check> {
    if !app::applies() {
        return None;
    }
    let what = tr!("menubar app", "状态栏 App");
    let current = moss_core::update::Version::current().to_string();
    let bundled = app::bundled();
    let fix_install = Some("cc-moss install".to_string());
    let Some(installed) = app::installed() else {
        return Some(if bundled.is_some() {
            check(
                Mark::Note,
                what,
                tr!(
                    "not in an Applications folder yet: notifications need it there",
                    "还没放进「应用程序」目录：放进去才能收到通知"
                ),
                fix_install,
            )
        } else {
            check(
                Mark::Note,
                what,
                tr!(
                    "not installed (it comes with the Homebrew install and the macOS release download)",
                    "没装（Homebrew 安装和 macOS 的 Release 下载包里都带着它）"
                ),
                None,
            )
        });
    };
    let have = app::version(&installed).unwrap_or_else(|| "?".into());
    let place = app::display(&installed);
    // Two copies run two menu bar icons and say everything twice.
    let extras = app::extra_copies(&installed);
    if !extras.is_empty() {
        let others: Vec<String> = extras.iter().map(|p| app::display(p)).collect();
        return Some(check(
            Mark::Warn,
            what,
            tr!(
                format!(
                    "two copies: {place} and {}; `cc-moss install` keeps the first and moves the other to the Trash",
                    others.join(", ")
                ),
                format!(
                    "有两份：{place} 和 {}；`cc-moss install` 留下前一份，另一份移到废纸篓",
                    others.join("、")
                )
            ),
            fix_install,
        ));
    }
    if have == current {
        return Some(check(
            Mark::Ok,
            what,
            tr!(format!("{have}, in {place}"), format!("{have}，在 {place}")),
            None,
        ));
    }
    // An older app with a bundled one of the right version is one `install`
    // away (install never downgrades); otherwise the CLI itself is behind or
    // ahead, and `update` sorts both out.
    let bundled_current = bundled
        .as_deref()
        .and_then(app::version)
        .is_some_and(|v| v == current)
        && app::is_older(&have, &current);
    Some(check(
        Mark::Warn,
        what,
        tr!(
            format!("{have} in {place}, but cc-moss is {current}"),
            format!("{place} 里是 {have}，但 cc-moss 是 {current}")
        ),
        Some(if bundled_current {
            "cc-moss install".to_string()
        } else {
            "cc-moss update".to_string()
        }),
    ))
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
    fn doctor_names_missing_claude_for_moss() {
        crate::i18n::set_for_test(crate::i18n::Lang::En);
        let facts = |claude_found, tmux_found| super::super::moss::Facts {
            enabled: true,
            running: false,
            claude_found,
            tmux_found,
            fence_gap: None,
        };
        let c = moss_check(&facts(false, true));
        assert_eq!(c.mark, Mark::Warn);
        assert!(
            c.detail.contains("claude") && !c.detail.contains("tmux"),
            "{}",
            c.detail
        );
        assert!(c
            .fix
            .as_deref()
            .is_some_and(|f| f.contains("cc-moss install")));
        let both = moss_check(&facts(false, false));
        assert!(both.detail.contains("claude, tmux"), "{}", both.detail);
        let fine = moss_check(&super::super::moss::Facts {
            enabled: true,
            running: true,
            claude_found: true,
            tmux_found: true,
            fence_gap: None,
        });
        assert_eq!(fine.mark, Mark::Ok);
        let gap = moss_check(&super::super::moss::Facts {
            enabled: true,
            running: true,
            claude_found: true,
            tmux_found: true,
            fence_gap: Some("/c".into()),
        });
        assert_eq!(gap.mark, Mark::Warn);
        assert!(gap.detail.contains("CLAUDE_CONFIG_DIR"), "{}", gap.detail);
        assert!(gap.fix.as_deref().is_some_and(|f| f.contains("config_dir")));
        let off = moss_check(&super::super::moss::Facts {
            enabled: false,
            running: false,
            claude_found: false,
            tmux_found: false,
            fence_gap: None,
        });
        assert_eq!(off.mark, Mark::Note, "off is a choice, not a problem");
    }

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
