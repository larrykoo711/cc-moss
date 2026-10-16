# PLAN — 001-cli：cc-moss Stage 1 终端版（4 Phase × 12 Task）

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
| P1 | MVP：重启后能列出并恢复会话 | T1.1–T1.5 | 直接解决立项的 D1–D3 |
| P2 | 会话选择器（替代 `t`） | T2.1–T2.5 | 日常入口；列表里能看到名字、状态、备注 |
| P3 | 终端资产并入 | T3.1–T3.2 | 解决 D4；本仓成为 tmux / starship / zsh 资产的正本 |
| P4 | 开源发布 | T4.1 | CI、README、Release、Homebrew，仓库公开 |

## 依赖图

```text
T1.1 登记与列出 ─┬─> T1.2 钩子安装 ─────────────────────────────┐
                 └─> T1.3 新建与名字同步 ─> T1.4 恢复 ─> T2.1 选择器 ─┴─> T3.1 终端资产 ─> T3.2 私有仓切换 ─> T4.1 开源发布
```

- 关键路径：T1.1 → T1.3 → T1.4 → T2.1 → T3.1 → T3.2 → T4.1
- 并行窗口：T1.1 完成后，T1.2 与 T1.3 可并行；T3.1 在 T1.2 和 T2.1 都完成后开工（复用 T2.1 的受管区块）
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
- 目标：`cc-moss setup` 一条命令接好 Claude Code 钩子，`--uninstall` 能干净移除
- 文件：`crates/cc-moss/src/cmd/setup.rs`、`crates/moss-core/src/settings_json.rs`
- 内容：合并写入 `~/.claude/settings.json` 的 SessionStart / UserPromptSubmit / Stop / SessionEnd 钩子
  （按命令识别，只增不改用户已有条目）；写之前先显示 diff、确认后写、带时间戳备份；
  `--dry-run`、`--yes`、`--uninstall`。zsh 集成挪到 T2.1（见日志 2026-10-15）
- TDD：settings.json 合并（文件不存在、空对象、已有其他钩子、已装过、旧路径升级、格式错误时拒绝写入）
- 验收：在临时 HOME 下 setup → 再次 setup 显示无变化 → uninstall 后文件与原来逐字节一致
- commit：`feat(setup): install and remove Claude Code hooks`
- 依赖：T1.1

### T1.3 新建与名字同步

- 仓：cc-moss
- 目标：`cc-moss new` 建出 tmux 名与 Claude 名一致的会话；`/rename` 后 tmux 名自动跟上
- 文件：`crates/moss-core/src/{tmux,naming,launch_args,config}.rs`、`crates/cc-moss/src/cmd/new.rs`、
  `crates/cc-moss/src/cmd/hook.rs`（扩展）
- 内容：tmux 适配层（命令执行可注入，支持 socket）；命名（主机前缀、sanitize、重名加后缀）；
  `cc-moss new [name] [-C dir] [--detach] [-- args]`（备注走 T1.4 的 `note`），读 `config.toml` 的默认参数，用 `--session-id` 固定 id；
  UserPromptSubmit 同步名字并 `rename-session`；启动参数白名单采集；不在 tmux 中时 SessionStart 返回提示
- TDD：sanitize 与重名；参数白名单（含凭据样式值被丢弃）；用假 `claude` 脚本在独立 socket 上跑 new → 改名
- 验收：独立 socket 上 `new job-a` 后 tmux 名为 `<prefix>_job-a`；模拟 `session_title=job-b` 的 UserPromptSubmit 后
  tmux 名变为 `<prefix>_job-b`
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

### T1.5 会话名唯一

- 仓：cc-moss
- 目标：一个名字只对应一个未关闭的会话；`mycc xxxx` 遇到同名在运行就进入、已中断就提示恢复，不再新建
- 文件：`crates/cc-moss/src/cmd/{new,restore,hook}.rs`、`crates/cc-moss/tests/unique_names_e2e.rs`
- 来源：用户 2026-10-16 试用前反馈（见 spec Source）
- commit：`feat(session): one open session per name`
- 依赖：T1.3、T1.4

### T2.1 会话选择器

- 仓：cc-moss
- 目标：`cc-moss pick`（`t` 指向它）替代 `t.zsh`，并修掉盘点出的 4 个 ⚠️
- 文件：`crates/cc-moss/src/pick/{mod,app,source,keys}.rs`、`crates/moss-core/src/managed_block.rs`、`assets/zsh/cc-moss.zsh`
- 内容：zsh 受管区块（`t` 指向 `cc-moss pick`，插在 starship init 之前，坑 #8；由 `cc-moss setup` 写入）；
  ratatui + nucleo；三个标签页（Sessions：tmux 会话合并 Claude 槽位，显示名字、状态、备注；
  Folders：有 zoxide 就用，选中目录即 `cc-moss new`；Interrupted：一键恢复）；快捷键改为 `ctrl-*`，不占用可搜索字符；
  删除前确认；一会话一窗口（R9），不设 Windows 标签页、不支持 tmuxinator；macOS 隐私权限（TCC）检测覆盖所有建会话路径
- TDD：键位映射、各数据源的合并与排序、删除确认流程（纯逻辑单测，不依赖终端）
- 验收：独立 socket 上手工走查三个标签页；盘点里的 4 个 ⚠️ 逐条复测不再出现（tmuxinator 相关的两条随 R9 一并消失）
- commit：`feat(pick): native session picker`
- 依赖：T1.4

### T2.2 选择器对齐 `claude agents`（反馈车道）

- 仓：cc-moss
- 目标：用户真机走查 T2.1 后的视觉反馈：分组单列表、整行选中色、底部输入框、灰色提示与快捷键面板
- 文件：`crates/cc-moss/src/pick/{app,ui,source,mod}.rs`、`crates/cc-moss/tests/pick_e2e.rs`
- commit：`feat(pick): agents-style picker`
- 依赖：T2.1

### T2.3 选择器第二轮打磨（反馈车道）

- 仓：cc-moss
- 目标：会话状态（Working / Waiting）与动画、最后一句话预览、品牌页头、匹配高亮、随选中项变化的提示
- 文件：`crates/moss-core/src/{event,registry,slot,agent/claude}.rs`、`crates/cc-moss/src/pick/*`
- commit：`feat(pick): activity, previews and polish`
- 依赖：T2.2

### T2.4 选择器重新设计：项目分组单行列表 + 详情卡片（反馈车道）

- 仓：cc-moss
- 目标：解决 T2.3 的「一大片」：按项目分组、每会话一行、细节只给选中项（右侧卡片）、去掉重复与零值
- 文件：`crates/cc-moss/src/pick/{app,ui}.rs`、`crates/cc-moss/tests/pick_e2e.rs`
- commit：`feat(pick): project-grouped list with a detail card`
- 依赖：T2.3

### T2.5 中英文国际化 + 友好文案

- 目标：界面、命令输出、帮助与钩子提示都有中英两种文案，按系统语言自动选择；文案口语、友好、带一点小幽默
- spec：`tasks/done/T2.5-i18n-and-copy.spec.md`

### T3.1 终端资产 install / doctor

- 仓：cc-moss
- 目标：`cc-moss install` 在新机器上一条命令装好 tmux.conf、starship.toml、zsh 区块；`doctor` 能体检
- 文件：`crates/cc-moss/assets/{tmux.conf,starship.toml,zsh/*}`（内嵌资产放在 crate 内，`cargo package` 才带得上）、
  `crates/cc-moss/src/cmd/{install,doctor}.rs`、`crates/moss-core/src/platform.rs`
- spec：`tasks/done/T3.1-install-doctor.spec.md`
- 内容：资产脱敏后迁入并内嵌（tmuxinator 模板不迁，R9）；整文件推送与受管区块替换（复用 T2.1 的 `managed_block`）；先 diff 后写、备份、`--dry-run`；
  平台检测（macOS / Debian 系）并列出缺失依赖及安装命令；`doctor` 覆盖全部资产与依赖
- TDD：临时 HOME 下 install 幂等；区块永远在 starship init 之前；坑 #8 #10 #11 #12 #13 转为测试
- 验收：临时 HOME 全量 install → doctor 全绿；私有词表扫描 `assets/` 零命中
- commit：`feat(assets): install and doctor for terminal assets`
- 依赖：T1.2

### T3.2 私有仓切换

- 仓：私有终端标准 skill（不在本仓，仅在本仓档案记录结果）
- 目标：私有仓只保留多主机下发，安装动作改为调用 `cc-moss install`；旧的 `t.zsh` 和模板退役
- spec：`tasks/done/T3.2-private-switch.spec.md`
- 内容：私有 skill 的同步脚本改为远程执行 `cc-moss install --yes`；删除迁走的模板和 tmuxinator 安装步骤；在本机切到 `cc-moss`
  （`t` → `cc-moss pick`，启动别名改为 `cc-moss new`）
- 验收：本机走一遍完整日常流程；私有 skill 的体检脚本全绿
- commit（私有仓）：按该仓规范
- 依赖：T2.1、T3.1

### T4.1 开源发布

- 仓：cc-moss
- 目标：仓库公开，别人能用 `brew install` 装上
- 文件：`README.md`、`LICENSE`、`.github/workflows/{ci,release}.yml`、homebrew tap 的 formula
- 内容：~~公开前清理 git 历史~~ ✅ 2026-10-16 提前完成（filter-repo 按私有词表替换全部历史、整理为规范提交，GitHub 仓库删除重建后推送）；README ✅ 2026-10-16 中英双语初版（发布时补「和其他方案的区别」）；CI 在 macOS 与 Ubuntu 跑 L3；打 tag 后构建
  多平台二进制发 GitHub Release；tap 仓加 formula；全仓私有词表扫描
- 验收：干净机器上 `brew install` → `cc-moss setup` → `new` → kill tmux → `restore` 走通
- commit：`docs: readme and license`、`ci: build, test and release workflows`
- 依赖：T3.2

### T4.2 新版本提醒与 `cc-moss update`

- 仓：cc-moss
- 目标：0.1.0 起就能发现新版本并一键升级（用户要求赶在首发前带上）
- spec：`tasks/done/T4.2-update.spec.md`
- 内容：每天最多一次查最新 Release（只在选择器、update、doctor；钩子永不联网，可关闭）；选择器页头提醒与「升级到 X.Y.Z」一项；
  `cc-moss update` 按 Homebrew / cargo / Release 二进制三种安装方式升级，二进制下载后校验 sha256、自检再原子替换
- 验收：离线 e2e（桩 curl/brew/cargo + 夹具 tarball）；发版后真实从旧版本升级一次
- commit：`feat: update notice and cc-moss update`
- 依赖：T4.1

## Kanban

| Task | 仓 | 依赖 | 状态 | SDD | 验收锚点 |
|------|----|------|------|-----|----------|
| **T1.1** 登记与列出 | cc-moss | — | ✅ Done | done/T1.1-registry-hook-ls.spec.md | 状态机测试 + hook p95 |
| **T1.2** 钩子安装 | cc-moss | T1.1 | ✅ Done | done/T1.2-setup-hooks.spec.md | setup/uninstall 逐字节还原 |
| **T1.3** 新建与名字同步 | cc-moss | T1.1 | ✅ Done | done/T1.3-new-and-name-sync.spec.md | 独立 socket 改名同步 |
| **T1.4** 恢复 | cc-moss | T1.1,T1.3 | ✅ Done | done/T1.4-restore.spec.md | 假 claude 全流程 e2e + 真机 |
| **T1.5** 会话名唯一 | cc-moss | T1.3,T1.4 | ✅ Done | done/T1.5-unique-names.spec.md | 同名进入或恢复 e2e |
| **T2.1** 会话选择器 | cc-moss | T1.4 | ✅ Done | done/T2.1-picker.spec.md | 4 个 ⚠️ 复测消失 |
| **T2.2** 选择器对齐 claude agents | cc-moss | T2.1 | ✅ Done | done/T2.2-picker-look.spec.md | 分组列表 + agents 皮肤 |
| **T2.3** 选择器第二轮打磨 | cc-moss | T2.2 | ✅ Done | done/T2.3-picker-polish.spec.md | 状态分组 + 预览 + 高亮 |
| **T2.4** 选择器重新设计（列表 + 详情卡片） | cc-moss | T2.3 | ✅ Done | done/T2.4-picker-master-detail.spec.md | 项目分组单行 + 详情卡片 |
| **T2.5** 中英文国际化 + 友好文案 | cc-moss | T2.4 | ✅ Done | done/T2.5-i18n-and-copy.spec.md | 按系统语言自动中英；文案友好 |
| **T3.1** 终端资产 install/doctor | cc-moss | T1.2,T2.1 | ✅ Done | done/T3.1-install-doctor.spec.md | 临时 HOME 幂等 + doctor 绿 |
| **T3.2** 私有仓切换 | 私有仓 | T2.1,T3.1 | ✅ Done | done/T3.2-private-switch.spec.md | 本机日常流程走通 |
| **T4.1** 开源发布 | cc-moss | T3.2 | ✅ Done | done/T4.1-open-source.spec.md | 干净机器 brew 全流程 |
| **T4.2** 新版本提醒与 update | cc-moss | T4.1 | ✅ Done | done/T4.2-update.spec.md | 旧版本经 update 升到新版本 |

## 日志

<!-- SKIP-GATE / AMEND / DISTILL / E2E / REPORT / RETRO 行追加在这里；空是健康态 -->
- E2E 2026-10-16 v0.1.0 发布：main c3276d3 CI 双平台绿 → tag v0.1.0 → release.yml 四目标构建、校验和、第三方许可证、GitHub Release（说明取自 CHANGELOG）全部成功；无 TAP_TOKEN 时按设计跳过公式更新，公式由 controller 用 SHA256SUMS 渲染推到 tap（brew audit --strict --online 通过）。验收：3 台 macOS 经私有同步脚本改为 brew 安装（先改钩子再停用旧副本），doctor 全绿、`update --check` 真实联网判定最新、运行中 tmux 已重载；干净 Linuxbrew x86_64（brew update 后）`brew install` → `--version` → `update --check` → `brew test` 全过；arm64 Linux 的 Homebrew 不受支持（Homebrew 自身报错），README 注明用 Release 二进制
- AMEND 2026-10-16 T4.2: 「发版后从旧版本真实升级」改为 deliberately_skipped——0.1.0 是首个带升级功能的版本，无更旧版本可升；以真实网络 `update --check` 替代，完整升级在 0.1.1 补做 approved-by=controller（用户要求赶在首发带上升级功能，此项客观上发版前不可验）
- REVIEW 2026-10-16 T4.2 安全向独立评审：高 1（sudo 升级时 tar 还原构建机 uid，新二进制归属本机其他用户）、中 4（brew/cargo 没升也报成功；tmux 弹窗里结果一闪而过；Ctrl-D 被当成「是」；测试用环境变量在正式版生效）、低 8。已修：tar 不还原属主 + 新文件沿用旧属主 + release 包属主为 0；下载包里的符号链接在执行前拒绝；包管理器升级后核对版本；EOF 不算同意；选择器升级后等回车；两个覆盖变量只在 debug 构建生效；预发布标签发成 pre-release；预发布 crate 版本不再 panic。未修（低）：Ctrl-C 残留临时目录、配置解析失败时 doctor 仍联网、doctor 检查会阻塞约 4 秒的措辞。真实 GitHub `releases/latest` 跳转格式已用公开仓库核实；无 Release 时不带 tag 的跳转按「未知」处理。release.yml 缺 TAP_TOKEN 时跳过公式更新并提示
- REVIEW 2026-10-16 T4.1 开源前五路评审（用户要求「全方位 review」）：隐私（中 1 低 5，含档案里一句描述个人设备的话与源自真实项目名的测试前缀，均已改）、文档与首次上手（高 3 中 9 低 15）、代码与打包（高 5 中 11 低 10）、仓库卫生与装修（10 项决策建议），以及合并后代码复审（中 2 低 8）。两名实施 agent 分线修复（稳健性：钩子在二进制删除后静默、大输出不卡死、/proc 判活与 busybox、僵尸进程判死、写锁内不起子进程、新版登记表只读、钩子永不非零退出、状态文件 0700/0600；交互：tmux<3.2 兼容、claude 绝对路径与启动即退出检测、install --uninstall、非 zsh 用户、ls --json 字段改为 sessions/id、pick 初始搜索、「中断」统一）。controller 自修：tmux 3.5 把 -F 输出里的制表符换成 `_` 导致 Linux 上 `new` 全挂（分隔符改为 `|:|`）、rust-version 实测改 1.89、卸载删文件前备份。不修：Linux 上预发布版本跨升级的起始时间格式（无已发布版本、无 Linux 安装）；restore 每个会话串行等 500 ms（待优化）。验证：macOS 295 测试全绿；Linux（Debian、tmux 3.5a、`--init`）全量绿；musl 静态二进制在 Ubuntu 20.04（tmux 3.0）/Debian 12/Ubuntu 24.04 clean-room 通过、Alpine busybox 判活正确；Debian 11 因官方安全源下线无法搭环境（非本项目问题）。LESSONS L16–L17
- CHANGE 2026-10-16 T4.1 仓库装修（用户要求）：简介、20 个 topics、只留 squash 合并且合并后删分支、关 Projects/Wiki、4 个标签；社区文件、CI、release（四目标 + sha256 + 第三方许可证 + 自动更新 tap）、Homebrew 公式模板；homebrew-tap 里的已有公式与其上游模板按 brew audit 修正（本地分支，未推送）
- AMEND 2026-10-16 T4.1: 范围扩大（非弱化）——四路开源前评审发现代码稳健性问题（高 5 / 中 11），纳入本任务修复；homebrew-tap 已存在，改为往现有 tap 加公式 approved-by=controller（扩大范围，按用户「全方位 review…清理」的要求）
- REVIEW 2026-10-16 T3.1/T3.2 多 agent 全方位评审（用户要求；正确性 / 用户文件安全 / 安全与隐私 / 测试与文案可维护性四路独立只读）：高危 0、中危约 10、低危若干，全部核实后修复——备份权限、Debian 缺包连坐、starship 安装器改先下载再执行、ZDOTDIR 下 PATH 区块落点、XDG 重载、CRLF 重复区块、悬空软链、tmux 弹窗路径与注入、钩子命令引号、代理段泄露凭据、doctor 误判与漏报、迁移脚本半行删除 / 备份撞名 / 先迁移后确认 / 失败仍继续等；新增 17 个测试（227 全绿）；干净设备 Ubuntu 24.04 + Debian 12 通过；迁移演练 14 项全过。LESSONS L15
- AMEND 2026-10-16 T3.1: 「已有 starship 配置 → 展示 diff、默认不覆盖」改为「提示已保留、可加 --replace-starship 替换」：整份配置的 diff 有数百行，会淹没其他改动（评审 UX 意见）；不覆盖的行为不变 approved-by=controller（待用户知悉）
- E2E 2026-10-16 T3.2: 私有 skill 改为下发 cc-moss（私有仓本地提交，未推送）；Docker 迁移演练（借空闲机，用旧模板造出老机器后跑新同步脚本）11 项检查全过、二次运行零改动；本机已切换：doctor 11 项全绿、私有行为验收 17 项全过、shell 启动无输出、隔离 tmux server 加载新配置无报错。待用户走查日常流程（第 3 条验收）；远端各机按设备清单逐台切换，需用户逐台确认
- E2E 2026-10-16 T3.2 收尾: 用户走查本机日常流程通过（第 3 条验收）；用户批准后两台远端 macOS 切换：同步脚本预览 → 迁移 → 私有区块 → install → doctor 全绿（其一提示用户手写的同名别名，属用户内容未动），隔离 tmux server 加载新配置、`prefix + t` / `prefix + r` 绑定正确，运行中的 tmux 已重载，`cc-moss ls` 识别出全部现有会话。顺带修了私有同步脚本两处（远端尚无 cc-moss 时 dry-run 调用它；dry-run 下非交互 read 读到 EOF 静默退出）和迁移脚本漏识别 `[[ -f … ]] && source` 写法。其余设备按用户决定暂不切换。观察：同一对话被两个 Claude 进程同时打开时，`cc-moss ls` 列出两行完全相同的记录（数据如实，展示上可提示「同一对话开在两处」，待排期）
- E2E 2026-10-16 T3.1 干净设备（用户建议用 Docker、借空闲机跑）：全新 Ubuntu 24.04 + 免密 sudo 普通用户，`install --yes --with aliases` 用真实 apt 与 starship 脚本装齐，doctor 11 项全 ✓，二次 install 零改动；途中修复两个只有干净设备才暴露的问题（先 `apt-get update`；先建 `~/.local/bin`），并在 root 下去掉 sudo。LESSONS L14
- DONE 2026-10-16 T3.1: `cc-moss install`（hooks/tmux/starship/shell 默认、aliases 可选；tmux 走顶部 source-file 区块、已有 starship 配置保留、`.zshrc` 区块恒在 starship init 前；缺依赖按 brew/apt 列命令、确认后执行）与 `cc-moss doctor`（✓/·/!/✗，坑 #8 #10 #12 #13 均为检查项，✗ 时退出码 1）；setup 的写文件逻辑抽为 `cmd/files.rs` 共用；资产脱敏后内嵌（私有词零命中）；`psg` 改为 `ps aux`（macOS 的 ps 不支持 `f`）。210 测试全绿；e2e 10 个全在临时 HOME + 桩工具；变异 3/3；本机 doctor 只读核对。LESSONS L13（测试差点执行真实 brew）
- PLAN-CHANGE 2026-10-16 T3.1: 用户拍板三项（已有配置叠加不覆盖；核心默认、别名可选；依赖列出命令、问过再装）；资产目录从仓根 `assets/` 改到 `crates/cc-moss/assets/`（打包需要）；迁移私有仓旧区块划给 T3.2
- CHANGE 2026-10-16 slogan 定为「文明会延续，对话也是。/ Civilizations carry on. So do conversations.」（用户从三轮候选中选定，呼应 MOSS 的使命）；`--version` 去掉功能说明行，只留名字版本、slogan、仓库地址
- DONE 2026-10-16 T2.5: 中英文国际化（判定顺序 CC_MOSS_LANG → 配置 → LC_ALL → LC_MESSAGES → macOS 系统语言 → LANG；只在要显示文字时判定，钩子热路径不受影响）；选择器、命令输出、`--help`（含 help 子命令）、钩子提示全覆盖，文案改为口语、友好、带一点小幽默；`moss-core` 的 resolve 改为带类型的错误，由 cc-moss 翻译；表格按显示宽度对齐中文；页头标志改为两格盲文 ⢎⡱（检查运行中会话时有点在游走）；`--version` 在终端里显示盲文字符画标志 + slogan（管道输出仍是 `cc-moss <版本>`）。192 测试全绿；变异 2/2；真实环境中文选择器截屏核对
- PLAN-CHANGE 2026-10-16: 新增 T2.5（中英文国际化 + 面向 C 端的友好文案），用户提出；页头标志改为两格盲文 ⢎⡱（用户：单字符 ∞ 太小）并入本任务
- CHANGE 2026-10-16 标志改版（用户：上一版太具象，参考 Claude/OpenAI 的抽象标志与终端动态小图形）：三个抽象方向中用户选 C「Continuum」——伯努利双纽线画成的 ∞，中心交叉处一条线从下方穿过；动画版光带沿环流动；终端里页头用品牌色 `∞`，「工作中」改为 4 个盲文点沿 8 字轨迹游走（测试锁定每帧 4 点、相邻帧共用 3 点）。旧的具象图标作废
- FIX 2026-10-16 选择器反馈三项（反馈车道）：(1) `t` 启动卡 1–2s：对账（`claude agents` 冷启动 1.1s+）挡在首帧前，改为后台线程，跑完立即无感刷新，首帧约 55ms；(2) git worktree 标记：按 `.git` 文件的 gitdir 识别（不限 `.claude/worktrees`），分组标题显示「worktree · 主仓名」，详情卡显示主仓路径 + worktree 名；用户选「图标 + 文字」，Nerd Font 图标由 `[ui] icons`（默认 auto = 检测 starship）控制；(3) Working 动画去掉形似「+」的帧。另：项目图标定稿（MOSS 之眼 + 地球轨道，16/32px 用简化版），横幅改用该图标
- FIX 2026-10-16 选择器「New session here」（反馈车道，用户：选中后会自动跳走、放在末尾不好用）：每 2 秒刷新时选中项保持特意排除了新建行，改为一并保持；新建行移到列表顶部，打开时仍落在第一个会话上，按一次 ↑ 即到。tmux 实测选中后跨两次刷新不跳；README 截图同步重拍
- FIX 2026-10-16 「工作中」误报（反馈车道，用户截图：显示 Working 的会话点进去并没在工作）：根因是对账用 `claude agents` 的 busy 覆盖了钩子状态，而它把一轮结束后仍在跑的后台 shell/agent 也算作 busy。改为钩子为准，只纠正卡住的标记：`claude agents` 报 idle 即清除（Esc 中断不发 Stop 钩子）；报 busy 时以会话记录末尾为准（`turn_duration`/`stop_hook_summary`/`end_turn`/用户中断 = 已结束，跳过元信息与本地斜杠命令）。变异 11/11；真实数据副本上只剩真正在跑的会话显示工作中
- FIX 2026-10-16 自动对账（反馈车道，用户：最好是自动的）：`ls` 与选择器每次打开时先对账——(1) 认领 `claude agents --json` 里在运行但未登记的交互会话（沿父进程找到 tmux 窗格，丢弃「目录名-两位十六进制」的自动名）；(2) 无名会话从会话记录的最后一次 `/rename` 补名字（`--resume` 后钩子不带名字），只读上次之后新增的部分；选择器名字兜底改为 Claude 名 → tmux 名 → id。测试不再可能调用真实 claude（helper 默认指向不存在的路径，假 claude 识别 `agents`）；变异 9 个杀 8 个，剩 1 个是纯性能等价变异；真实数据副本上认领 9 个会话，稳态 0.26s
- DONE 2026-10-15 T1.4: 两轮独立评审（reports/T1.4-review-1.md，共 2 Critical / 9 Important 全部修复）；L2 全绿；e2e 5/5；变异 8/8；三次真机复验（暗号 PELICAN-42 / HERON-7 / OTTER-3）
- DONE 2026-10-16 T2.4: 143 测试全绿；e2e 连跑 10/10（排查 flake 时发现搜索会命中长路径、选中不在最佳匹配，已修并补测试）；变异 2/2；宽窄两种布局截图核对
- PLAN-CHANGE 2026-10-16: 新增 T2.4（反馈车道）：用户认为 T2.3 「一大片、眼睛看花」，选定左列表右详情
- DONE 2026-10-16 T2.3: 实施中两次吸收用户反馈（去掉 Folders、两行会话、搜索框、分区；无需记忆的交互：方向键 + 回车 + → 菜单 + Esc，支持鼠标）；141 测试全绿；e2e 5/5；变异 2/2；演示数据渲染截图核对
- PLAN-CHANGE 2026-10-16: 新增 T2.3（反馈车道）：用户要求继续美化
- DONE 2026-10-16 T2.2: 对照实测的 `claude agents` 配色与结构重做；132 测试全绿；e2e 5/5；变异 2/2；真实数据渲染截图已核对
- PLAN-CHANGE 2026-10-16: 新增 T2.2（反馈车道）：用户走查 T2.1 认为界面太丑，要求对齐 `claude agents`，选定分组单列表
- DONE 2026-10-16 T2.1: e2e 在独立 tmux 窗格里用 send-keys 驱动（连跑 5/5）；131 个测试全绿；变异 3/3；controller 抽查（risk ui），真机走查交给用户
- DONE 2026-10-16 T1.5: controller 抽查（risk logic）；e2e 5/5；变异 2/2；全部 verify 原样通过
- PLAN-CHANGE 2026-10-16: 新增 T1.5 会话名唯一（用户：不可能开第二个同名会话），P1 由 4 个任务变为 5 个
- MILESTONE 2026-10-15 P1 MVP 闭环：T1.1–T1.4 全部 Done。按推进节奏，先由用户在本机真实使用一段时间，再开 T2.1
- AMEND 2026-10-15 T1.4: 评审第 2 轮新增 4 条验收项（认领快照、标记豁免时限、开机前 pid、去重挑可恢复的最新副本），均为加强 approved-by=controller
- AMEND 2026-10-15 T1.4: 评审第 1 轮（reports/T1.4-review-1.md）后，「恢复中」保护从 60 秒计时器改为记录被拉起进程的 pid（计时器只兜底拿不到 pid 的情况），可观测行为变化：运行中的会话再次 restore 显示 Nothing to restore 而不是「恢复中」；新增 6 条验收项、变异清单相应更新，均为加强 approved-by=controller
- DONE 2026-10-15 T1.2: 子 agent 在独立 worktree 实施；controller 抽查 diff 后补修 `CLAUDE_CONFIG_DIR`（先红后绿）；全部 verify 在 main 上原样通过；变异 3/3 变红
- DONE 2026-10-15 T1.3: e2e 跑在独立 tmux socket + 假 claude 上（连跑 5/5）；tmux 内 hook p95 21–24ms（首版 28ms，合并 tmux 查询、缓存主机名后降下来）；变异 3/3 变红；全部 verify 在 main 上原样通过
- DONE 2026-10-15 T1.1: 两轮独立评审（reports/T1.1-review-1.md）；L2 全绿；并发 debug/release 各 10/10；hook p95 7.1ms；变异 6/6 变红；spec 全部 verify 原样执行通过
- AMEND 2026-10-15 T1.1: busy_timeout 200ms → 1s（hook 设置里有 10s 超时，200ms 下首次并发打开会丢事件，评审 C1）；新增 6 条验收项（F10/F11、评审 I1–I5），全部为加强；并发验收改为 debug/release 各连跑 10 次 approved-by=controller（加强型 AMEND，不需用户批准）
- AMEND 2026-10-15 T1.1: 评审第 2 轮新增 3 条验收项（R1–R3），均为加强 approved-by=controller
- PLAN-CHANGE 2026-10-15: 用户定 R9「一个 tmux 会话一个窗口」：T1.3 去掉第二个 claude 改窗口名；T2.1 去掉 Windows 标签页与 tmuxinator；T3.1 不迁 tmuxinator 模板
- PLAN-CHANGE 2026-10-15: T1.3 去掉 `new -m`（槽位尚未登记，预登记会留下幽灵槽位），备注统一走 T1.4 `note`
- PLAN-CHANGE 2026-10-15: zsh 集成与 `managed_block` 从 T1.2 挪到 T2.1（T2.1 之前区块里没有可指向的命令，YAGNI）；T3.1 依赖相应加上 T2.1。T1.2 只装钩子

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
