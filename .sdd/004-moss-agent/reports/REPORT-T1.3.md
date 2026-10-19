# REPORT T1.3 — 代答中枢（CLI）

分支 feature/004-T1.3-ask，起点 010bc76。

## Commits
- 69e1d23 test: compare observing hook latency with a baseline build
- 224c8d8 feat(core): ask protocol for questions a session waits on（含 notes/T1.3-design.md）
- e0587ee feat: catch a session's question and answer it from outside

## Success criteria
1. 有接手方时答案经 PermissionRequest 回填 —— `cargo test -p cc-moss --test ask_e2e answer_reaches_session` ok（假 claude + 独立 tmux；PreToolUse 写的题被 PermissionRequest 沿用，只有一份；输出 `decision.behavior=allow` + `updatedInput.answers {"Pick one?":"B"}`；closed = `{"reason":"answered"}`）。真 Claude Code 2.1.302 上也已跑通。
2. 先答者胜 —— `cargo test -p moss-core -- first_answer_wins` ok（8 线程同时写，只 1 个成功，其余 AlreadyAnswered）；CLI 层 `first_answer_is_kept`：第二次 answer 报 "someone already answered"。
3. 五种回落 —— disabled_no_wait / no_taker_no_wait / broken_state_fails_open / times_out_quietly / withdrawn_quietly 全 ok；前三种断言 < 100 ms（debug 构建）。broken 覆盖：坏 JSON、asks 目录不可写、config 坏、状态目录是文件、panic。
4. 终端先答 1 秒内撤回 —— `answered_in_session_closes_ask` ok；真机实测 hook 在下一次轮询（≤150 ms）退出。
5. 6 个观察型 hook 延迟（release，基线 = 010bc76 release 二进制，与本构建逐样本交替跑，各 200 次）：
   - 改动前（HEAD 只加了测试，同代码对比，看噪声）：session-start 17.9/19.2，user-prompt-submit 13.6/11.1，stop 8.9/9.0，notification 9.6/10.6，post-tool-use 5.26/5.21，session-end 13.0/14.3（this/baseline p95，ms）
   - 改动后：session-start 26.3/27.9，user-prompt-submit 15.7/15.3，stop 15.5/16.5，notification 16.2/15.8，post-tool-use 6.59/6.63，session-end 18.4/18.4 —— 最大差 +0.35 ms < 5 ms，测试通过（机器同时有其它 agent 在编译，绝对值偏高，两边同受影响）
6. 多选 / 自由文字 / 多题经真机确认 —— `notes/ask-probe.md` §补测：多选 `"苹果, 樱桃"`，自由文字原文，多题每题一个键；CLI 回填 `--choice ×3`、`--text` 真机通过；另测了普通模式 Bash 权限的 allow / deny / 终端先答，以及 Esc（hook 被 Claude Code 结束、无 PostToolUse，靠 Stop 撤回）。不留痕：settings.json shasum 前后均 0b078f0d…，probe2/probe3 tmux 已 kill，scratch 已删。

Mutations（spec 三条）均被杀：answer 改 create+truncate → first_answer_wins FAILED；去掉接手方判断 → no_taker_no_wait FAILED（3.16 s）；withdraw 去掉 PostToolUse → answered_in_session_closes_ask FAILED。

CI 同款（工作树干净、最后 commit 之后）：fmt --check ok，clippy -D warnings ok，cargo test --all 553 passed / 0 failed。注：本机 cargo 联网会卡住，全部用 `--offline` 跑（Cargo.lock 未变）。

## 偏离 / 补充（请 controller 留意）
- PostToolUse 撤回按「同会话 + 同工具名」，比 spec 的「PostToolUse(AskUserQuestion)」宽：`--safe` 会话在终端批准了 Bash 后，对应权限题也撤回。最坏情形是 hook 提早不表态 = 今天的行为。
- 撤回理由映射（契约只有 4 个值）：PostToolUse / UserPromptSubmit / Stop → `answered-in-session`；SessionEnd → `session-ended`。
- `ask answer` 加了隐藏参数 `--by cli|desktop|slack|moss`（缺省 cli），给 App/守护进程复用 CLI 写答案。
- `ask.enabled` 可经 `config set` 改（写成 TOML 布尔）；`wait_secs` 只能手改。
- 工作区 Cargo.toml：`time` 加 `parsing` feature（解析 expires_at）。
- doctor 文案未改（T2.1 在改 doctor.rs）：老安装会显示 "4 hook(s) missing (... PreToolUse, PermissionRequest): session events are incomplete"，文案对 ask 钩子不太贴切，可在合并后顺手调。
- hook_latency.rs 新建（spec 允许）；e2e 计时前先启动一次二进制，避开 macOS 对新二进制首次启动的检查（约 0.7 s）。
- site 测试未跑（site/node_modules 不在）；guide 新增锚点 `#替会话接住问题` / `#answering-a-sessions-questions-from-elsewhere`。

## deliberately_skipped
- 等待中途接手方掉线就提早退出（spec 只要求开始时判断）
- kqueue 盯文件（150 ms 轮询足够）
- CLI 多题里的自由文字（`--text` 只限单题；App/守护进程可直接写答案文件）

## 待裁决
- 无阻塞项。上面「偏离」第 1、2 条若不认可，改动都在 `cmd/ask_hook.rs::withdraw` 一处。
