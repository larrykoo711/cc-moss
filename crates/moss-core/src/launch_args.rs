//! The subset of a Claude Code command line worth replaying on restore.

use std::process::Command;
use std::time::Duration;

use crate::command::output_with_timeout;

/// 已知 Claude 选项的参数数量；身份核验与来源采集共用，不扩大回放白名单。
pub fn option_arity(flag: &str) -> Option<usize> {
    const VALUES: &[&str] = &[
        "--append-system-prompt",
        "--system-prompt",
        "--append-system-prompt-file",
        "--system-prompt-file",
        "--model",
        "--effort",
        "--permission-mode",
        "--add-dir",
        "--agent",
        "--name",
        "-n",
        "--settings",
        "--setting-sources",
        "--mcp-config",
        "--allowedTools",
        "--disallowedTools",
        "--tools",
        "--output-format",
        "--input-format",
        "--max-turns",
        "--max-budget-usd",
        "--fallback-model",
        "--debug-file",
        "--session-id",
        "--resume",
        "-r",
    ];
    const FLAGS: &[&str] = &[
        "--dangerously-skip-permissions",
        "--allow-dangerously-skip-permissions",
        "--fork-session",
        "--continue",
        "-c",
        "--strict-mcp-config",
        "--bare",
        "--safe-mode",
        "--disable-slash-commands",
        "--verbose",
        "--no-session-persistence",
        "--include-partial-messages",
    ];
    if VALUES.contains(&flag) {
        Some(1)
    } else if FLAGS.contains(&flag) {
        Some(0)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellMode {
    Fresh,
    Native,
    NonSession,
    Blocked,
}

impl ShellMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Native => "native",
            Self::NonSession => "non-session",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Clone, Copy)]
enum ShellArity {
    Flag,
    Value,
    Optional,
    Many,
}

/// 原生 --help 的启动选项；不扩大破坏性操作身份核验的窄表。
fn shell_arity(flag: &str) -> Option<ShellArity> {
    match flag {
        "--debug" | "-d" | "--resume" | "-r" | "--prompt-suggestions" | "--remote-control" => {
            Some(ShellArity::Optional)
        }
        "--add-dir" | "--allowedTools" | "--allowed-tools" | "--disallowedTools"
        | "--disallowed-tools" | "--tools" | "--mcp-config" | "--betas" | "--file" => {
            Some(ShellArity::Many)
        }
        "--agents"
        | "--autocompact"
        | "--json-schema"
        | "--permission-prompts"
        | "--plugin-dir"
        | "--plugin-url"
        | "--remote-control-session-name-prefix"
        | "--system-prompt-snapshot"
        | "-C"
        | "--dir" => Some(ShellArity::Value),
        "--ax-screen-reader"
        | "--brief"
        | "--chrome"
        | "--no-chrome"
        | "--exclude-dynamic-system-prompt-sections"
        | "--forward-subagent-text"
        | "--ide"
        | "--include-hook-events"
        | "--replay-user-messages"
        | "--restricted"
        | "--bg"
        | "--background"
        | "--print"
        | "-p"
        | "--detach"
        | "--yes"
        | "--safe"
        | "--json" => Some(ShellArity::Flag),
        _ => match option_arity(flag) {
            Some(0) => Some(ShellArity::Flag),
            Some(1) => Some(ShellArity::Value),
            _ => None,
        },
    }
}

fn consume_shell_value(args: &[String], i: &mut usize, arity: ShellArity, inline: bool) -> bool {
    match arity {
        ShellArity::Flag => !inline,
        ShellArity::Value => {
            if inline {
                true
            } else if *i < args.len() {
                *i += 1;
                true
            } else {
                false
            }
        }
        ShellArity::Optional => {
            if !inline && args.get(*i).is_some_and(|value| !value.starts_with('-')) {
                *i += 1;
            }
            true
        }
        ShellArity::Many => {
            if !inline {
                if *i == args.len() {
                    return false;
                }
                *i += 1;
            }
            while args.get(*i).is_some_and(|value| !value.starts_with('-')) {
                *i += 1;
            }
            true
        }
    }
}

/// 模式仅识别选项边界；未知 arity 不允许借后面的 help 绕过目录守卫。
pub fn shell_mode(args: &[String]) -> ShellMode {
    let mut mode = ShellMode::Fresh;
    let mut positional = false;
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        i += 1;
        if arg == "--" {
            break;
        }
        let (flag, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(flag, value)| (flag, Some(value)));
        if matches!(flag, "--help" | "-h" | "--version" | "-v") && inline.is_none() {
            return ShellMode::NonSession;
        }
        if !flag.starts_with('-') && !positional {
            positional = true;
            if matches!(flag, "attach" | "respawn") {
                return ShellMode::Blocked;
            }
            if flag == "agents" {
                return if i == 1 {
                    agents_mode(&args[i..])
                } else {
                    ShellMode::Blocked
                };
            }
            // 来源：隔离 HOME 中的原生 --help；不把普通 prompt 词当子命令。
            if matches!(
                flag,
                "mcp"
                    | "auth"
                    | "doctor"
                    | "update"
                    | "upgrade"
                    | "install"
                    | "plugin"
                    | "plugins"
                    | "setup-token"
                    | "auto-mode"
                    | "logs"
                    | "rm"
                    | "stop"
                    | "kill"
                    | "purge"
                    | "import"
                    | "gateway"
            ) {
                return if i == 1 {
                    ShellMode::NonSession
                } else {
                    ShellMode::Native
                };
            }
            if flag == "ultrareview" {
                return ShellMode::Native;
            }
        }
        if matches!(
            flag,
            "--resume"
                | "-r"
                | "--continue"
                | "-c"
                | "--session-id"
                | "--print"
                | "-p"
                | "--bg"
                | "--background"
        ) {
            mode = ShellMode::Native;
        }
        if let Some(arity) = shell_arity(flag) {
            if !consume_shell_value(args, &mut i, arity, inline.is_some()) {
                return ShellMode::Blocked;
            }
        } else if flag.starts_with('-') {
            return ShellMode::Blocked;
        }
    }
    mode
}

/// agents --json 明确只读退出；交互界面可派发新会话，不能按当前 PWD 授权。
fn agents_mode(args: &[String]) -> ShellMode {
    let mut json = false;
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        i += 1;
        match arg.as_str() {
            "--help" | "-h" => return ShellMode::NonSession,
            "--json" => json = true,
            "--all" => {}
            "--cwd" if i < args.len() => i += 1,
            _ if arg.starts_with("--cwd=") => {}
            _ => return ShellMode::Blocked,
        }
    }
    if json {
        ShellMode::NonSession
    } else {
        ShellMode::Blocked
    }
}

/// fresh 请求不能被附带的身份参数改成恢复，也不能覆盖分配的名字与 UUID。
pub fn fresh_arguments(args: &[String]) -> bool {
    if shell_mode(args) != ShellMode::Fresh {
        return false;
    }
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        i += 1;
        if arg == "--" {
            break;
        }
        let (flag, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(flag, value)| (flag, Some(value)));
        if matches!(
            flag,
            "--name" | "-n" | "--json" | "--detach" | "--yes" | "--safe" | "-C" | "--dir"
        ) {
            return false;
        }
        if let Some(arity) = shell_arity(flag) {
            if !consume_shell_value(args, &mut i, arity, inline.is_some()) {
                return false;
            }
        }
    }
    true
}

/// Flags that take a value and are kept.
const WITH_VALUE: [&str; 5] = [
    "--permission-mode",
    "--model",
    "--effort",
    "--add-dir",
    "--agent",
];
/// Boolean flags that are kept.
const BOOLEAN: [&str; 1] = ["--dangerously-skip-permissions"];
/// Substrings that make a value look like a credential.
const SECRET_HINTS: [&str; 4] = ["sk-", "key", "token", "secret"];

/// Keep only whitelisted flags from `argv` (program name first). Everything
/// else, including session-specific flags and the prompt, is dropped.
pub fn whitelist(argv: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        let arg = argv[i];
        i += 1;
        if BOOLEAN.contains(&arg) {
            out.push(arg.to_string());
            continue;
        }
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) => (f, Some(v)),
            None => (arg, None),
        };
        if !WITH_VALUE.contains(&flag) {
            continue;
        }
        let value = match inline {
            Some(v) => v,
            None if i < argv.len() && !argv[i].starts_with('-') => {
                i += 1;
                argv[i - 1]
            }
            None => continue,
        };
        let lower = value.to_ascii_lowercase();
        if SECRET_HINTS.iter().any(|h| lower.contains(h)) {
            continue;
        }
        out.push(flag.to_string());
        out.push(value.to_string());
    }
    out
}

/// Whether `args` already choose a permission mode (`--permission-mode …`
/// or `--dangerously-skip-permissions`), so `cc-moss new` adds none of its own.
pub fn sets_permissions(args: &[String]) -> bool {
    args.iter().any(|a| {
        let flag = a.split_once('=').map_or(a.as_str(), |(f, _)| f);
        BOOLEAN.contains(&flag) || flag == WITH_VALUE[0]
    })
}

/// `args` without the flags that pick a permission mode: `--dangerously-skip-permissions`,
/// `--permission-mode <value>` and `--permission-mode=<value>`. For `cc-moss new --safe`.
pub fn without_permissions(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        i += 1;
        if BOOLEAN.contains(&arg.as_str()) || arg.starts_with("--permission-mode=") {
            continue;
        }
        if arg == WITH_VALUE[0] {
            if i < args.len() && !args[i].starts_with('-') {
                i += 1;
            }
            continue;
        }
        out.push(arg.clone());
    }
    out
}

/// `cc-moss new`'s own flags; everything else that looks like a flag is
/// Claude Code's.
const NEW_OWN_WITH_VALUE: [&str; 2] = ["-C", "--dir"];
const NEW_OWN_BOOLEAN: [&str; 6] = ["--detach", "--yes", "--safe", "--json", "-h", "--help"];

/// Let `cc-moss new` take Claude Code's flags directly, anywhere on the line:
/// `new --dangerously-skip-permissions login-page` becomes
/// `new login-page -- --dangerously-skip-permissions`. `args` are the words
/// after `new`. Flags Claude Code takes a value for (`--model opus`) keep it;
/// anything after an explicit `--` follows the moved flags unchanged.
pub fn normalize_new(args: &[String]) -> Vec<String> {
    let mut own = Vec::new();
    let mut claude = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        i += 1;
        if arg == "--" {
            claude.extend(args[i..].iter().cloned());
            break;
        }
        let flag = arg.split_once('=').map_or(arg.as_str(), |(f, _)| f);
        if matches!(flag, "-n" | "--name") {
            if let Some((_, value)) = arg.split_once('=') {
                own.push(value.to_string());
            } else if let Some(value) = args.get(i) {
                own.push(value.clone());
                i += 1;
            } else {
                // 缺值交给 CLI 拒绝，不把它当成无名新建。
                own.push(arg.clone());
            }
        } else if NEW_OWN_BOOLEAN.contains(&arg.as_str()) {
            own.push(arg.clone());
        } else if NEW_OWN_WITH_VALUE.contains(&flag) {
            own.push(arg.clone());
            if !arg.contains('=') && i < args.len() {
                own.push(args[i].clone());
                i += 1;
            }
        } else if arg.starts_with('-') && arg.len() > 1 {
            claude.push(arg.clone());
            let start = i;
            if let Some(arity) = shell_arity(flag) {
                consume_shell_value(args, &mut i, arity, arg.contains('='));
                claude.extend(args[start..i].iter().cloned());
            }
        } else {
            own.push(arg.clone());
        }
    }
    if !claude.is_empty() {
        own.push("--".into());
        own.extend(claude);
    }
    own
}

/// Whether `argv` (program name first) runs Claude Code headless (`-p`,
/// `--print`): a script's one-shot call, nothing a person would resume.
/// `claude agents` lists those as interactive too.
/// A cluster of short flags counts (`-cp`) when it is only flags that take
/// no value; anything after `--` is not a flag.
pub fn is_headless(argv: &[&str]) -> bool {
    argv.iter().skip(1).take_while(|a| **a != "--").any(|a| {
        *a == "--print"
            || a.starts_with("--print=")
            || a.strip_prefix('-').is_some_and(|short| {
                !short.starts_with('-')
                    && short.contains('p')
                    && short.chars().all(|c| SHORT_SWITCHES.contains(c))
            })
    })
}

/// Claude Code's one-letter switches that take no value (`-p` print, `-c`
/// continue, `-d` debug, `-v` version, `-h` help).
const SHORT_SWITCHES: &str = "pcdvh";

/// The arguments of a running process, exactly as it was given them
/// (macOS: `sysctl` KERN_PROCARGS2; Linux: `/proc/<pid>/cmdline`), else
/// from `ps` split on whitespace (where an argument with spaces in it
/// can't be told apart). None when neither works.
pub fn argv(pid: u32) -> Option<Vec<String>> {
    exact_argv(pid).or_else(|| ps_argv(pid))
}

#[cfg(target_os = "macos")]
pub fn exact_argv(pid: u32) -> Option<Vec<String>> {
    let pid = libc::c_int::try_from(pid).ok()?;
    let mut argmax: libc::c_int = 0;
    let mut size = std::mem::size_of::<libc::c_int>();
    let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
    // SAFETY: sysctl with a valid MIB and out-pointer of the stated size.
    let got = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            2,
            (&mut argmax as *mut libc::c_int).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if got != 0 || argmax <= 0 {
        return None;
    }
    let mut buf = vec![0u8; argmax as usize];
    let mut size = buf.len();
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    // SAFETY: as above; the kernel writes at most `size` bytes.
    let got = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buf.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if got != 0 {
        return None;
    }
    parse_procargs2(&buf[..size.min(buf.len())])
}

#[cfg(not(target_os = "macos"))]
pub fn exact_argv(pid: u32) -> Option<Vec<String>> {
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    if raw.is_empty() {
        return None;
    }
    let mut out: Vec<String> = raw
        .split(|b| *b == 0)
        .map(|w| String::from_utf8_lossy(w).into_owned())
        .collect();
    if out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    Some(out)
}

/// macOS's KERN_PROCARGS2: `argc` (a native int), the executable's path,
/// padding NULs, then `argc` NUL-terminated arguments (the environment
/// follows and is not read).
pub fn parse_procargs2(buf: &[u8]) -> Option<Vec<String>> {
    let argc = i32::from_ne_bytes(buf.get(..4)?.try_into().ok()?);
    let argc = usize::try_from(argc).ok()?;
    let rest = &buf[4..];
    let path_end = rest.iter().position(|b| *b == 0)?;
    let mut at = path_end;
    while rest.get(at) == Some(&0) {
        at += 1;
    }
    let mut out = Vec::with_capacity(argc);
    for word in rest[at..].split(|b| *b == 0).take(argc) {
        out.push(String::from_utf8_lossy(word).into_owned());
    }
    (out.len() == argc).then_some(out)
}

fn ps_argv(pid: u32) -> Option<Vec<String>> {
    let out = output_with_timeout(
        Command::new("ps")
            .args(["-o", "args=", "-p", &pid.to_string()])
            .env("LC_ALL", "C"),
        Duration::from_millis(500),
    )?;
    let line = String::from_utf8_lossy(&out.stdout);
    Some(line.split_whitespace().map(String::from).collect())
}

/// Whitelisted arguments of a running process.
pub fn capture(pid: u32) -> Vec<String> {
    argv(pid).map_or_else(Vec::new, |a| {
        whitelist(&a.iter().map(String::as_str).collect::<Vec<_>>())
    })
}
