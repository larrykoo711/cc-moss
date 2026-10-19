# REPORT T1.1 代答探针

Claude Code 2.1.302，结论在 `.sdd/004-moss-agent/notes/ask-probe.md`（commit f150399）。

## Success criteria
- 三问结论 + 证据：`rg -c '^## Q[123]'` → 3。含 hook 输入 JSON（脱敏）、界面截取、时间线
- 推荐：`rg -n '推荐'` → 156/158 行。推荐 PermissionRequest allow + updatedInput.answers；退路 B → C → K1 只提醒
- 错误路径：A 不表态、D 超时两种模式均记录（`rg -c '超时|不表态'` → 10）
- 不留痕：tmux -L cc-moss-probe 已 kill（no server）；shasum ~/.claude/settings.json 前后均 0b078f0d…ff66；scratch 已删

## 关键结论
- Q1 行：B（PreToolUse updatedInput+answers）、E（PermissionRequest allow+updatedInput+answers）都干净；C 能用但显示 "hook error"
- PermissionRequest 运行时终端对话框同时可答，先答者胜，hook 晚到的答案被丢弃（hook 进程不会被杀）；PreToolUse 运行时对话框被挡
- Q2：bypass 下 Bash 不触发 PermissionRequest，AskUserQuestion 仍触发
- Q3 行：A、D 都回落到原生对话框，终端照常答；D 期间用户干等到 timeout
- PermissionRequest 输入没有 tool_use_id
- bypass 风险确认框：排除用户设置后也未弹出，原因未分辨，交 T1.2 在临时 HOME 下核实

## deliberately_skipped
- 多选 / 多题 / 自填 / Esc 取消的 answers 形状
- CI 全量（只改了一个 .md，未跑 cargo）

## 待裁决
- 无
