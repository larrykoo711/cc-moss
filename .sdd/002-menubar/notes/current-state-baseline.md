# 现状盘点：状态栏 App 需要的东西，CLI 有没有

核实日期：2026-10-16，基于 `main` 1c13430（v0.1.1）。✅ 生效 ⚠️ 有但没接通 ❌ 没有。

| 需要 | 判定 | 依据 |
|------|------|------|
| 读全部会话及状态 | ✅ | `cc-moss ls --json`（`crates/cc-moss/src/cmd/ls.rs:11`），先对账（死进程判 interrupted）再输出，schema 1 |
| 区分「在忙 / 在等你 / 已中断」 | ✅ | 选择器 `State::of`（`crates/cc-moss/src/pick/app.rs:26`）：running 且 `busy` = 在忙，running 且非 `busy` = 在等你，interrupted = 已中断；三者都能从 JSON 字段算出 |
| 按项目分组 | ⚠️ | 选择器按 `cwd` 分组（`pick/app.rs:234`），worktree 标记由 `moss_core::worktree::of` 算，**JSON 里没有** worktree 字段 |
| 从终端外打开某个会话 | ✅ | 在新终端窗口里执行 `cc-moss attach <name>`（`cmd/attach.rs`）即可；不在 tmux 里的会话 attach 会报错 |
| 一键恢复 | ✅ | `cc-moss restore --yes [name…]`，非交互；每个会话建在独立 tmux 会话里（detached） |
| 新建会话 | ✅ | `cc-moss new <name> -C <dir> --detach`（`cmd/new.rs:98`），建好不 attach |
| 写备注、结束提醒 | ✅ | `cc-moss note <name> <text>`、`cc-moss close <name>` |
| 新版本信息 | ⚠️ | 检查结果缓存在状态目录 `update.json`，只有 CLI 读；`cc-moss update --check` 可机读性未定 |
| GUI 进程找到 `cc-moss` | ❌ | GUI App 拿不到 shell 的 PATH；Homebrew 装在 `/opt/homebrew/bin`（Apple Silicon）或 `/usr/local/bin`（Intel） |
| 终端窗口里会话名唯一 | ✅ | T1.5：会话名唯一，可直接当 `attach`/`note`/`close` 的参数 |

## 结论

数据和写操作 CLI 都够用；需要补的只有两处小接口：`ls --json` 加 `project`/`worktree` 字段（让 App 不重写分组逻辑），
以及 App 自己解决「找到 cc-moss」。
