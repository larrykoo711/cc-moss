# T2.20 探针：Moss 的 bypass 警告（2026-11-09）

- Claude Code：2.1.319（`claude --version`）
- 测试：`crates/cc-moss/tests/moss_bypass_probe.rs`（`#[ignore]`）
- 跑法：`cargo test -p cc-moss --test moss_bypass_probe -- --ignored --nocapture`
- 环境：独立 tmux socket `cc-moss-test-a-probe`（`-f /dev/null`，跑完 `kill-server`）；临时 HOME 和工作目录；`env -i` 干净环境（无供应商变量、无 `CLAUDE_CONFIG_DIR`）；临时 HOME 的 `.claude.json` 预置 `hasCompletedOnboarding` 和工作目录的信任。不发送任何按键，不登录（窗格底部显示「Not logged in」）。
- 参数：`--dangerously-skip-permissions --strict-mcp-config --setting-sources project,local`（与 Moss 相同，去掉 `--session-id` 和开场提示）。

## 结果

| 组 | 键写在哪 | 20 秒内出现「Bypass Permissions mode」 | 窗格 |
|---|---|---|---|
| 对照 | 工作目录 `.claude/settings.json`（项目级） | 是 | `WARNING: Claude Code running in Bypass Permissions mode … ❯ No, exit / Yes, I accept` |
| 实验 | 工作目录 `.claude/settings.local.json`（本地级） | 否（等满 20 秒） | 直接进入输入框，底栏 `⏵⏵ bypass permissions on` |

结论：Claude Code 2.1.319 只从本地级读这个键，项目级无效，与 `notes/claude-code-facts.md:10` 一致；T2.20 把键写进 Moss 的 `settings.local.json` 能让 Moss 不停在警告框上。

## 过程中的一次失败

第一次跑两组都停在「信任此文件夹」：`.claude.json` 里信任的是 `/var/folders/…`，Claude Code 按真实路径 `/private/var/folders/…` 认。测试改为先 `canonicalize` 再写信任后通过。真 Moss 的信任框由守护进程的 `accept_trust` 处理，不在探针范围内。

## 证据边界

只证明 2.1.319 的行为；Claude Code 升级后可重跑探针。未登录状态下验证，登录后的启动顺序（例如先出登录或账户提示）未覆盖。

# T2.25 探针：「信任此文件夹」怎么选 Yes（2026-11-09）

- 测试：同文件 `moss_trust_probe`（`#[ignore]`），`cargo test -p cc-moss --test moss_bypass_probe moss_trust_probe -- --ignored --nocapture`
- 环境同上（独立 socket、临时 HOME、`env -i`、未登录）；工作目录**没有**预置信任，键放本地级（与 Moss 相同）。按键只发给本测试自己的窗格；跑完 `kill-server`。
- Claude Code 2.1.319。

| 步骤 | 选中项（`❯`） |
|---|---|
| 首屏 | `No, exit` |
| `send-keys Down`（框刚出现） | 仍是 `No, exit`：这一下没生效 |
| 再 `Down` | `Yes, I trust this folder` |
| `Up` | `No, exit` |
| 移回 Yes 后 `Enter` | 进入 Claude Code 输入框，底栏 `⏵⏵ bypass permissions on` |

结论：`Down` 把选择从 No 移到 Yes，`Up` 移回；在 Yes 上回车即信任并启动。框刚出现时发的第一下可能被吞掉，所以守护进程每次只发一下 `Down`，重新抓窗格确认选中 Yes 才回车；没移过去就什么也不按，下一秒再试，最多 3 次，之后按卡住（`trust_folder`）报告。框文本即测试夹具 `reply.rs` 的 `TRUST_NO_SCREEN`。
