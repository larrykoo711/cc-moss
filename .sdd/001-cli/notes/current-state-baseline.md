# 现状盘点：待迁入的终端标准资产（2026-10-15）

来源：私有仓里的终端标准 skill（下文记作 `SS/`），只读盘点。本机已部署的 `t.zsh`、`tmux.conf`、
`starship.toml` 与模板逐字节一致。行号以盘点当天为准。

## 1. `t`：tmux 会话工具（`SS/templates/t.zsh`，v0.2.4，464 行）

| 能力 | 判定 | 证据 |
|---|---|---|
| fzf 三个标签页的选择器（sessions / windows / folders） | ✅ | :263-374 |
| 新建会话，名字为 `<主机前缀>_<目录名>` | ✅ | :344-359, :186-202 |
| attach / switch（区分是否已在 tmux 内） | ✅ | :168-177 |
| 一键启动已停止的 tmuxinator 模板 | ✅ | :150-155, :178-180 |
| `t stop` / `t rm` / `t clean` / `t update` / `--version` | ✅ | :437-451, :383-397 |
| 选择器里按 `x` 删除 | ⚠️ 不确认就直接删模板 | :36-46, :295 |
| 选择器里按 `r` 刷新 | ⚠️ 在 windows / folders 标签页也只刷新 sessions | :332 |
| 单字符快捷键 | ⚠️ 占用了 `1 2 3 d s x r`，搜索框里打不了这几个字符 | :324-332 |
| folders 标签页 | ⚠️ 依赖 sesh，但 Ubuntu 的安装步骤不装 sesh | :165 |
| macOS 隐私权限（TCC）检测与 tmux server 自愈 | ⚠️ 新建和模板启动两条路径有检测；经 `sesh connect -T` 的路径没有 | :99-118, :179, :190 vs :182, :370 |
| tmuxinator 版本告警抑制 | ⚠️ 经 sesh 间接调用时不生效（坑 #11 会复现） | :17 vs :370 |
| 自动生成模板里的 `claude` 窗口 | ⚠️ 只执行 `claude --version`，并不会启动会话 | :195 |
| Claude 会话管理（启动、恢复、列表、状态） | ❌ | 无 |

命名：`_t_sanitize_name`（:71-73）只保留 `a-zA-Z0-9._-` 并转小写；主机前缀在 macOS 上取
`scutil --get LocalHostName`，其他平台取 `hostname -s`（:79-87）。sanitize 会保留 `.`，但 tmux 会改写
会话名里的 `.` 和 `:`，带点号的目录可能导致会话名和模板名对不上（推断，未验证）。

外部依赖：tmux、fzf（要较新版本，用到了 `--footer`）、tmuxinator、sesh（zoxide 经 sesh 间接使用）。

## 2. 配置资产

| 资产 | 行数 | 内容 | 私有信息 |
|---|---|---|---|
| `tmux.conf` | 68 | 真彩、5 万行回滚、鼠标、vim 风格导航、vi 复制模式（`pbcopy`）、Catppuccin 状态栏、关闭自动改名 | 无 |
| `starship.toml` | 266 | gruvbox powerline；设备名（与 `t` 的主机前缀同源）、SSH 徽标、tmux 会话名、代理端口等自定义段 | 无，注释里有多机语境 |
| `zshrc-modern-cli.block` | — | 新名字的现代 CLI 别名（`ll`、`lg`、`catp`…），**不改写** ls/cat/grep/find/cd | 头注释写着私有仓名 |
| `zshrc-ssh-tty-restore.block` | — | `ssh()` 退出后复位鼠标上报等终端模式 | 同上 |
| `zshrc-prompt.block` | — | starship init（必须最后执行） | 无 |
| `zshenv-noninteractive-path.block` | — | 非交互 shell 的 PATH | 无 |
| tmuxinator 模板 ×2 | — | macOS / Linux 服务器窗口布局 | 无 |

## 3. 安装与同步脚本（`SS/scripts/sync.sh`，821 行）

| 部分 | 判定 | 迁移去向 |
|---|---|---|
| `push_file`：先 diff、确认后写入、dry-run | ✅ | 用 Rust 重写成通用安装逻辑 |
| `push_zshrc_block`：受管区块整块替换，插在 starship init 之前 | ✅ | 用 Rust 重写 |
| `install_deps` 平台矩阵（macOS brew / Ubuntu apt / OpenWrt opkg） | ✅ | 用 Rust 重写；OpenWrt 是否支持待定 |
| `detect_status` / `verify_deploy` | ⚠️ 只验证了一个区块 | 用 Rust 重写成 `doctor`，覆盖全部资产 |
| 安装 starship / eza / bat / fd / rg | ❌ 体检会检查这些工具，但没有安装步骤 | 补上 |
| 本机平台 | ⚠️ 写死为 macos | 改成自动检测 |
| 多主机清单、SSH 下发、菜单 | 私有 | **不进开源核心**，留在私有仓，改为调用本项目 |
| `snapshot-dotfiles.sh`、私有 `.zshrc` 的 bug 检查 | 私有 | 不迁移 |

## 4. 已知的坑（`SS/references/pitfalls.md`，共 14 条，迁入后作为测试用例和 LESSONS）

1. 不在 mesh 网络里的主机要填真正能连上的地址
2. macOS 系统 Ruby 执行 `gem install` 报 EACCES，要用 brew ruby
3. Ubuntu 要求免密 sudo
4. Linux 模板没有 claude 窗口，属于正常
5. apt 装的 fzf 太旧
6. 必须先配好 SSH 密钥登录
7. bash 的 `local` 用在函数外，`bash -n` 查不出来
8. 受管区块追加到末尾会落在 starship 之后，必须插在它之前
9. `while read` 循环里的 ssh 会吞掉循环的 stdin，要加 `ssh -n`
10. 部署区块后，原来手写的同名 alias 不会自动删除
11. tmuxinator 的 tmux 版本白名单会弹出阻塞提示，要加 `--suppress-tmux-version-warning=1`
12. 非交互 shell 只读 `.zshenv`
13. 远端 tmux 断线后，本地终端停在鼠标上报模式
14. 长期运行的 tmux server 被 macOS TCC 拒绝访问 `~/Documents`（根因未证实）

## 5. 开源前的脱敏要求

迁入时必须处理：设备清单文件（真实主机名、IP、用户名，**绝不复制**）、私有 SSH 密钥文件名、指向私有仓
决策文档的路径、坑的记录和变更日志里的真实主机名、区块头注释里的私有仓名、帮助文本里的主机名示例、
私有的 skill 治理文件（整份不迁移）。迁入时逐个文件用词表扫描。
