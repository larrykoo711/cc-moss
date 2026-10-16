//! Chinese `--help`. Built only when help or a usage error is shown, so the
//! hooks never pay for choosing a language.

use clap::{Arg, ArgAction, Command};

/// The help layout, with our own full-width headings: clap's `{all-args}`
/// would add ASCII colons. Sections a command does not have are left out.
fn template(cmd: &Command) -> String {
    let mut t = String::from("{about-with-newline}\n用法：{usage}\n");
    if cmd.has_subcommands() {
        t.push_str("\n命令：\n{subcommands}\n");
    }
    if cmd.get_positionals().any(|a| !a.is_hide_set()) {
        t.push_str("\n参数：\n{positionals}\n");
    }
    t.push_str("\n选项：\n{options}{after-help}");
    t
}

const AFTER_HELP: &str = "不带命令直接运行 `cc-moss` 会打开会话选择器（就是 `t`）。";

/// (subcommand, argument id, help). An empty argument id is the command's own
/// description; an empty subcommand is `cc-moss` itself.
const TEXT: &[(&str, &str, &str)] = &[
    (
        "",
        "",
        "崩溃也不怕：可恢复的 Claude Code 会话，住在 tmux 里",
    ),
    ("ls", "", "列出会话：先列中断的，再列运行中的"),
    ("ls", "all", "也列出主动关闭的会话"),
    ("ls", "json", "机器可读的输出（schema 1）"),
    (
        "events",
        "",
        "会话刚发生了什么，按先后列出（给 App 和脚本用）",
    ),
    ("events", "json", "机器可读的输出（schema 1）"),
    (
        "events",
        "since",
        "只要这个序号之后的事件；不给就是最近 50 条",
    ),
    ("events", "limit", "最多返回这么多条（至少 1）"),
    (
        "new",
        "",
        "新建一个命名的 Claude Code 会话，独占一个 tmux 会话",
    ),
    ("new", "name", "会话名，默认用目录名"),
    ("new", "dir", "工作目录（默认当前目录）"),
    ("new", "detach", "建好后先不进入"),
    ("new", "yes", "名字属于一个中断的会话时，不问直接恢复"),
    (
        "new",
        "claude_args",
        "传给 claude 的额外参数，写在 `--` 之后",
    ),
    (
        "setup",
        "",
        "安装 Claude Code 钩子，让会话自动登记到 cc-moss",
    ),
    ("setup", "dry_run", "只看 diff，什么都不写"),
    ("setup", "yes", "不问直接应用（stdin 不是终端时必须加）"),
    ("setup", "uninstall", "移除 cc-moss 钩子，其余设置原样保留"),
    (
        "setup",
        "settings",
        "要修改的设置文件 [默认：~/.claude/settings.json]",
    ),
    (
        "setup",
        "zshrc",
        "要移除旧版 `t` 函数的 zsh 启动文件 [默认：~/.zshrc]",
    ),
    (
        "install",
        "",
        "一次装好终端：钩子、tmux、starship、shell 区块，以及它们需要的工具",
    ),
    ("install", "with", "额外安装可选部分：aliases"),
    (
        "install",
        "skip",
        "不装某一部分：hooks、tmux、starship、shell 或 aliases",
    ),
    (
        "install",
        "replace_starship",
        "用 cc-moss 的 starship 配置替换你现有的",
    ),
    (
        "install",
        "uninstall",
        "撤销 install：移除它加的钩子、配置块和文件",
    ),
    ("install", "dry_run", "只看会改什么，不做任何改动"),
    ("install", "yes", "不问直接应用"),
    (
        "doctor",
        "",
        "体检钩子、别名、tmux、starship、shell 区块和工具",
    ),
    ("doctor", "fix", "把缺失或失效的别名链接重新建好"),
    (
        "alias",
        "",
        "给 cc-moss 命令起你自己的名字，任何 shell 里都能用（`t` 就是一个）",
    ),
    ("alias ls", "", "列出你的别名，以及每个能不能用"),
    ("alias ls", "json", "机器可读的输出（schema 1）"),
    ("alias add", "", "添加别名，或者修改它要运行的命令"),
    ("alias add", "force", "名字已经是 PATH 上的命令时也照样用"),
    ("alias add", "json", "机器可读的输出（schema 1）"),
    ("alias add", "name", "别名：只能用 a-z、0-9、- 和 _"),
    ("alias add", "command", "它要运行的 cc-moss 命令和参数"),
    ("alias rm", "", "删除别名和它的链接"),
    ("alias rm", "json", "机器可读的输出（schema 1）"),
    ("alias rm", "name", "要删除的别名"),
    ("update", "", "看看 cc-moss 有没有新版本，有就装上"),
    ("update", "check", "只看看，不做任何改动"),
    ("update", "yes", "不问直接升级"),
    ("pick", "", "选择会话、中断的会话或目录（就是 `t`）"),
    ("pick", "query", "打开时先搜这个，比如 `cc-moss pick api`"),
    (
        "restore",
        "",
        "把中断的会话恢复回来，每个独占一个 tmux 会话",
    ),
    ("restore", "yes", "不问直接恢复"),
    ("restore", "dry_run", "只列出会恢复哪些，不做任何改动"),
    (
        "restore",
        "names",
        "只恢复这些会话（Claude 名、tmux 名或 id 前缀）",
    ),
    ("attach", "", "跳到会话所在的 tmux 会话"),
    ("attach", "name", "会话名"),
    ("close", "", "不再提示恢复这些会话"),
    ("close", "names", "会话名"),
    ("note", "", "查看或设置会话备注：它在做什么、停在哪里"),
    ("note", "name", "会话名"),
    ("note", "goal", "这个会话是做什么的"),
    ("note", "state", "停在哪里 / 在等什么"),
    ("note", "text", "随手记的备注"),
    ("note", "clear", "清空全部备注"),
];

const ALIAS_ADD_LONG: &str = "添加别名，或者修改它要运行的命令。\n\n\
`cc-moss alias add tn new` 之后，`tn 登录页` 就等于 `cc-moss new 登录页`，任何 shell 里马上能用。\
只给一个带引号的参数时，它会被当成整条命令来读：`cc-moss alias add tr \"restore --yes\"`。\
--force 和 --json 要写在名字前面。";

const SETUP_LONG: &str = "安装 Claude Code 钩子，让会话自动登记到 cc-moss。\n\n\
写入前会用 diff 展示对 settings.json 的改动并征求同意；原文件旁会留一份带时间戳的备份。";

pub fn localize(cmd: Command) -> Command {
    let mut cmd = translate(cmd, "")
        .after_help(AFTER_HELP)
        .mut_arg("version", |a| a.help("显示版本"));
    let names: Vec<String> = cmd
        .get_subcommands()
        .map(|c| c.get_name().to_string())
        .collect();
    for name in names {
        cmd = cmd.mut_subcommand(&name, |c| {
            let nested: Vec<String> = c
                .get_subcommands()
                .map(|s| s.get_name().to_string())
                .collect();
            let mut c = translate(c, &name);
            for sub in nested {
                let key = format!("{name} {sub}");
                c = c.mut_subcommand(&sub, |s| translate(s, &key));
            }
            c
        });
    }
    let mut cmd = cmd
        .mut_subcommand("setup", |c| c.long_about(SETUP_LONG))
        .mut_subcommand("alias", |c| {
            c.mut_subcommand("add", |s| s.long_about(ALIAS_ADD_LONG))
        });
    // clap adds its `help` command while building; translate it afterwards.
    cmd.build();
    const HELP: &str = "显示这份帮助，或某个命令的帮助";
    cmd.mut_subcommand("help", |c| c.about(HELP))
        .mut_subcommand("alias", |c| c.mut_subcommand("help", |h| h.about(HELP)))
}

/// One command's description, argument help, headings and help flag.
fn translate(cmd: Command, name: &str) -> Command {
    let template = template(&cmd);
    let mut cmd = cmd.help_template(template).disable_help_flag(true).arg(
        Arg::new("help")
            .short('h')
            .long("help")
            .action(ArgAction::Help)
            .help("显示帮助"),
    );
    for (command, arg, help) in TEXT.iter().filter(|(c, _, _)| *c == name) {
        debug_assert_eq!(*command, name);
        cmd = if arg.is_empty() {
            cmd.about(*help).long_about(None::<&str>)
        } else {
            cmd.mut_arg(*arg, |a| a.help(*help))
        };
    }
    cmd
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    #[test]
    fn every_translated_argument_exists() {
        // mut_arg panics on an unknown id; debug_assert checks the rest.
        super::localize(crate::Cli::command()).debug_assert();
    }

    #[test]
    fn every_command_and_argument_is_translated() {
        let cmd = crate::Cli::command();
        let mut commands: Vec<(String, &clap::Command)> = Vec::new();
        for sub in cmd.get_subcommands().filter(|c| !c.is_hide_set()) {
            commands.push((sub.get_name().to_string(), sub));
            for nested in sub.get_subcommands() {
                commands.push((format!("{} {}", sub.get_name(), nested.get_name()), nested));
            }
        }
        for (name, sub) in commands {
            assert!(
                super::TEXT
                    .iter()
                    .any(|(c, a, _)| *c == name && a.is_empty()),
                "{name}"
            );
            for arg in sub.get_arguments() {
                let id = arg.get_id().as_str();
                assert!(
                    super::TEXT.iter().any(|(c, a, _)| *c == name && *a == id),
                    "{name} {id}"
                );
            }
        }
    }
}
