# PLAN — 001-cli：cc-moss Stage 1 终端版（4 Phase × 8 Task）

## 配套文档

| 文档 | 用途 |
|------|------|
| `DESIGN-cli.md` | 权威设计（R1–R8 已拍板） |
| `notes/hook-probe.md` | 钩子行为实测 F1–F9，状态机的事实依据 |
| `notes/current-state-baseline.md` | 待迁入资产的现状、14 条坑、脱敏要求 |
| `diagrams/` | 架构图、钩子时序图 |

## 铁律

- 每个任务先有 spec 且过 `sdd-gate.sh`，再动代码；实施在 `feature/001-<task>` 分支上
- 每个合入候选 L2 绿（`cargo clippy --all-targets -- -D warnings && cargo test --all`）
- 测试和验收绝不碰用户正在用的 tmux server 和真实的 `~/.claude`：独立 socket + 临时 HOME
- 公开仓：提交前用私有词表扫描新增内容
- 宿主事实见 `.sdd/config.md` 的 `{REPOS}` `{CHECK_TIERS}` `{E2E_ENV}`

## 总览

| Phase | 主题 | Task | 价值 |
|-------|------|------|------|
| P1 | MVP：重启后能列出并恢复会话 | T1.1–T1.4 | 直接解决立项的 D1–D3 |
| P2 | 会话选择器（替代 `t`） | T2.1 | 日常入口；列表里能看到名字、状态、备注 |
| P3 | 终端资产并入 | T3.1–T3.2 | 解决 D4；本仓成为 tmux / starship / zsh 资产的正本 |
| P4 | 开源发布 | T4.1 | CI、README、Release、Homebrew，仓库公开 |

## 依赖图

```text
T1.1 登记与列出 ─┬─> T1.2 钩子安装 ──────────────┬─> T3.1 终端资产 install/doctor ─┐
                 └─> T1.3 新建与名字同步 ─> T1.4 恢复 ─> T2.1 选择器 ──────────────┴─> T3.2 私有仓切换 ─> T4.1 开源发布
```

- 关键路径：T1.1 → T1.3 → T1.4 → T2.1 → T3.2 → T4.1
- 并行窗口：T1.1 完成后，T1.2 与 T1.3 可并行；T1.2 完成后，T3.1 可与 T1.3 / T1.4 / T2.1 并行
- P1 完成即可在本机真实使用（MVP 验收点）

## 任务

### T1.1 登记与列出

- 仓：cc-moss
- 目标：Claude Code 的钩子事件能落进登记表，`cc-moss ls` 能列出会话及其状态
- 文件：`Cargo.toml`（workspace）、`crates/moss-core/src/{lib,registry,slot,agent/mod,agent/claude,proc}.rs`、
  `crates/cc-moss/src/{main,cmd/hook,cmd/ls}.rs`、`crates/moss-core/tests/state_machine.rs`
- 内容：Cargo workspace 骨架；SQLite（WAL，busy timeout 200ms）登记表与 schema 迁移；槽位模型；
  `Agent` trait 与 Claude 实现（解析钩子输入、判定 SessionEnd reason）；`cc-moss hook <event>`（读 stdin、
  永远 exit 0、错误写 `hook.log`）；`cc-moss ls [--all] [--json]`，含 pid 存活与 pid 复用判定
- TDD：表驱动状态机测试，事件序列取自 `notes/hook-probe.md` 的真实序列（startup / clear / resume /
  exit / other / 无 SessionEnd）
- 验收：状态机测试全绿；`cc-moss hook` 收到非法 JSON 时 exit 0 且写日志；单次 hook 执行 p95 < 30ms
- commit：`feat(core): session registry, hook ingestion and ls`
- 依赖：无

### T1.2 钩子安装

- 仓：cc-moss
- 目标：`cc-moss setup` 一条命令接好钩子和 zsh 集成，`--uninstall` 能干净移除
- 文件：`crates/cc-moss/src/cmd/setup.rs`、`crates/moss-core/src/{settings_json,managed_block}.rs`、`assets/zsh/cc-moss.zsh`
- 内容：合并写入 `~/.claude/settings.json` 的 SessionStart / UserPromptSubmit / SessionEnd 钩子（带标记，
  只增不改用户已有条目）；写入 zsh 受管区块（插在 starship init 之前，坑 #8）；写之前先显示 diff、确认后写、
  带时间戳备份；`--dry-run`、`--yes`、`--uninstall`
- TDD：settings.json 合并（空文件、已有其他钩子、已装过、格式错误时拒绝写入）；受管区块的插入、替换、移除
- 验收：在临时 HOME 下 setup → 再次 setup 显示无变化 → uninstall 后文件与原来逐字节一致
- commit：`feat(setup): install and remove Claude Code hooks and zsh integration`
- 依赖：T1.1

### T1.3 新建与名字同步

- 仓：cc-moss
- 目标：`cc-moss new` 建出 tmux 名与 Claude 名一致的会话；`/rename` 后 tmux 名自动跟上
- 文件：`crates/moss-core/src/{tmux,naming,launch_args,config}.rs`、`crates/cc-moss/src/cmd/new.rs`、
  `crates/cc-moss/src/cmd/hook.rs`（扩展）
- 内容：tmux 适配层（命令执行可注入，支持 socket）；命名（主机前缀、sanitize、重名加后缀）；
  `cc-moss new [name] [-C dir] [-m note] [-- args]`，读 `config.toml` 的默认参数，用 `--session-id` 固定 id；
  UserPromptSubmit 同步名字并 `rename-session`；启动参数白名单采集；不在 tmux 中时 SessionStart 返回提示
- TDD：sanitize 与重名；参数白名单（含凭据样式值被丢弃）；用假 `claude` 脚本在独立 socket 上跑 new → 改名
- 验收：独立 socket 上 `new em-a` 后 tmux 名为 `<prefix>_em-a`；模拟 `session_title=em-b` 的 UserPromptSubmit 后
  tmux 名变为 `<prefix>_em-b`
- commit：`feat(session): new command and tmux name sync`
- 依赖：T1.1

### T1.4 恢复

- 仓：cc-moss
- 目标：重启后一条 `cc-moss restore` 把意外中断的会话按原名、原目录、原参数拉回各自的 tmux 会话
- 文件：`crates/cc-moss/src/cmd/{restore,attach,close,note}.rs`、`crates/cc-moss/tests/e2e_restore.rs`、`tests/fixtures/fake-claude`
- 内容：`restore [--yes] [name…]`（列出待恢复项、检查会话记录是否存在、勾选确认、逐个拉起）；
  `attach`；`close`；`note`（goal / state / note）
- TDD：假 `claude` 全流程：new → rename → clear → kill-server → restore，断言恢复命令用的是 `/clear` 之后的新 id、
  原名、原 cwd、白名单参数；空会话不尝试恢复
- 验收：上述 e2e 绿；真机验收：在独立 socket 上用真实 claude 复现 hook-probe 序列，restore 后对话接上
- commit：`feat(session): restore, attach, close and notes`
- 依赖：T1.1、T1.3

### T2.1 会话选择器

- 仓：cc-moss
- 目标：`cc-moss pick`（`t` 指向它）替代 `t.zsh`，并修掉盘点出的 4 个 ⚠️
- 文件：`crates/cc-moss/src/pick/{mod,app,source,keys}.rs`
- 内容：ratatui + nucleo；四个标签页（Sessions：tmux 会话合并 Claude 槽位，显示名字、状态、备注；Windows；
  Folders：有 zoxide 就用；Interrupted：一键恢复）；快捷键改为 `ctrl-*`，不占用可搜索字符；删除前确认；
  tmuxinator 模板装了就列出；macOS 隐私权限（TCC）检测覆盖所有建会话路径
- TDD：键位映射、各数据源的合并与排序、删除确认流程（纯逻辑单测，不依赖终端）
- 验收：独立 socket 上手工走查四个标签页；盘点里的 4 个 ⚠️ 逐条复测不再出现
- commit：`feat(pick): native session picker`
- 依赖：T1.4

### T3.1 终端资产 install / doctor

- 仓：cc-moss
- 目标：`cc-moss install` 在新机器上一条命令装好 tmux.conf、starship.toml、zsh 区块；`doctor` 能体检
- 文件：`assets/{tmux.conf,starship.toml,zsh/*.block}`、`crates/cc-moss/src/cmd/{install,doctor}.rs`、
  `crates/moss-core/src/platform.rs`
- 内容：资产脱敏后迁入并内嵌；整文件推送与受管区块替换（复用 T1.2）；先 diff 后写、备份、`--dry-run`；
  平台检测（macOS / Debian 系）并列出缺失依赖及安装命令；`doctor` 覆盖全部资产与依赖
- TDD：临时 HOME 下 install 幂等；区块永远在 starship init 之前；坑 #8 #10 #11 #12 #13 转为测试
- 验收：临时 HOME 全量 install → doctor 全绿；私有词表扫描 `assets/` 零命中
- commit：`feat(assets): install and doctor for terminal assets`
- 依赖：T1.2

### T3.2 私有仓切换

- 仓：私有终端标准 skill（不在本仓，仅在本仓档案记录结果）
- 目标：私有仓只保留多主机下发，安装动作改为调用 `cc-moss install`；旧的 `t.zsh` 和模板退役
- 内容：私有 skill 的同步脚本改为远程执行 `cc-moss install --yes`；删除迁走的模板；在本机切到 `cc-moss`
  （`t` → `cc-moss pick`，启动别名改为 `cc-moss new`）
- 验收：本机走一遍完整日常流程；私有 skill 的体检脚本全绿
- commit（私有仓）：按该仓规范
- 依赖：T2.1、T3.1

### T4.1 开源发布

- 仓：cc-moss
- 目标：仓库公开，别人能用 `brew install` 装上
- 文件：`README.md`、`LICENSE`、`.github/workflows/{ci,release}.yml`、homebrew tap 的 formula
- 内容：README（MOSS 叙事、30 秒上手、和其他方案的区别）；CI 在 macOS 与 Ubuntu 跑 L3；打 tag 后构建
  多平台二进制发 GitHub Release；tap 仓加 formula；全仓私有词表扫描
- 验收：干净机器上 `brew install` → `cc-moss setup` → `new` → kill tmux → `restore` 走通
- commit：`docs: readme and license`、`ci: build, test and release workflows`
- 依赖：T3.2

## Kanban

| Task | 仓 | 依赖 | 状态 | SDD | 验收锚点 |
|------|----|------|------|-----|----------|
| **T1.1** 登记与列出 | cc-moss | — | 🟦 Ready | — | 状态机测试 + hook p95 |
| **T1.2** 钩子安装 | cc-moss | T1.1 | ⬜ Backlog | — | setup/uninstall 逐字节还原 |
| **T1.3** 新建与名字同步 | cc-moss | T1.1 | ⬜ Backlog | — | 独立 socket 改名同步 |
| **T1.4** 恢复 | cc-moss | T1.1,T1.3 | ⬜ Backlog | — | 假 claude 全流程 e2e + 真机 |
| **T2.1** 会话选择器 | cc-moss | T1.4 | ⬜ Backlog | — | 4 个 ⚠️ 复测消失 |
| **T3.1** 终端资产 install/doctor | cc-moss | T1.2 | ⬜ Backlog | — | 临时 HOME 幂等 + doctor 绿 |
| **T3.2** 私有仓切换 | 私有仓 | T2.1,T3.1 | ⬜ Backlog | — | 本机日常流程走通 |
| **T4.1** 开源发布 | cc-moss | T3.2 | ⬜ Backlog | — | 干净机器 brew 全流程 |

## 日志

<!-- SKIP-GATE / AMEND / DISTILL / E2E / REPORT / RETRO 行追加在这里；空是健康态 -->

## 推进节奏

1. 第一波：T1.1（单人，地基）
2. 第二波：T1.2 与 T1.3 并行
3. 第三波：T1.4；同时可开 T3.1
4. P1 收尾：Phase 闭环评审 → 本机真实切换使用一段时间（MVP 验收点）→ 再开 T2.1

## 开放问题

| # | 问题 | 何时定 |
|---|------|--------|
| O1 | 许可证（建议 MIT，与已有公开仓一致） | T4.1 开工前 |
| O2 | 是否在 P4 之前先把仓库以私有形式推到 GitHub（便于备份和跨设备） | 现在可定 |
| O3 | 最低支持的 Rust 版本（MSRV） | T1.1 spec |

## 已定调（防翻案）

- 设计 R1–R8（见 `DESIGN-cli.md` §0），2026-10-15 用户批准
- 开机自动恢复不在本迭代；`setup` 只给启动别名的改法示例，不自动改（用户 2026-10-15）
