# sdd 项目配置（cc-moss）

> 位置：cc-moss/.sdd/config.md。本仓为公开开源仓，档案随仓公开——正文不得出现主机名、内网地址、
> 账号、凭据、私有仓名；示例一律用 `host-a` / `192.0.2.x`（RFC 5737）这类占位值。

## {WORKSPACE}
- 值：`.sdd/`；入 git：是（公开）；目录命名：`<ID>-<slug>`，ID 为三位自增序号

## {EDITION}
- 当前无 tracker：用三位自增序号（`001`、`002`…）。仓库公开到 GitHub 后改用 GitHub issue，
  届时由用户拍板是否把既有档案映射到 issue 号
- 规划：`001-cli` = Stage 1 终端版；`002-menubar` = Stage 2 macOS 状态栏 App；之后 `003-pet` 桌面宠物、`004-pet-approve` 宠物代办确认、`005-voice` 语音对话

## {REPOS}
| 子项目 | 语言 | 完整 CI 命令（L3 的组成） |
|--------|------|-------------------------|
| cc-moss（Cargo workspace） | Rust | `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test --all` |
| shell 资产（zsh / tmux / starship） | zsh、toml | `zsh -n` 语法检查 + 资产安装的集成测试（`cargo test` 内用临时 HOME 跑） |

- 分支流：仓库公开后一律走 PR（squash 合并）；公开前的早期阶段脚手架与档案曾直接提交 `main`；进入 S4 实施后一律走
  `feature/<ID>-<slug>` 分支，S6 集成 squash 回 `main`
- commit message：英文 Conventional Commits（`feat:` `fix:` `docs:` `chore:` `refactor:` `test:`），
  一行主题 + 要点列表，不带生成信息
- 历史聚合台账：记 PLAN.md 日志

## {CHECK_TIERS}
- L1：spec 的 `verify:`（通常是 `cargo test -p <crate> <过滤>`）
- L2：`cargo clippy --all-targets -- -D warnings && cargo test --all`
- L3：`{REPOS}` 全部命令
- SDD_KEY_ENV：`TZ`

## {REVIEW_AGENTS}
- general-purpose + 自拟维度 prompt；涉及 shell 注入、文件覆盖、钩子写 `~/.claude/settings.json` 的任务
  加一名安全维度评审者

## {REQUIRED_SKILLS}
| 阶段 | 子 skill |
|------|----------|
| S1 设计出图 | `archify` |
| S6 发版 | 无宿主 skill，用 `references/release-protocol.md` 兜底 |

## {BACKUP_SKILL}
- 档案入 git，无需备份

## {TRACKER}
- 无（见 `{EDITION}`）

## {REGISTER}
- 暂无独立的 constitution；跨迭代约定写在仓库根 `CLAUDE.md` 与 `CONTRIBUTING.md`

## {RELEASE}
- 用 release-protocol 兜底；分发渠道：GitHub Releases + `larrykoo711/homebrew-tap`

## {LESSONS_TEMPLATE}
- 三段式「现象 → 根因 → 规避」

## {E2E_ENV}
- 无 mirror。端到端验证在本机用独立 tmux socket（`tmux -L cc-moss-test`）与临时 `HOME` 跑，
  不得碰用户正在用的默认 tmux server 与真实 `~/.claude`
