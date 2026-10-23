# 同进程 Resume 的持久身份绑定

状态：**拟议 / 实施中，按 T2.14 AMEND-1 获批合同设计；不代表实现或安全验收已完成。** 日期：2026-10-22。

权威合同：[T2.14 AMEND-1](../tasks/in-progress/T2.14-resume-identity.spec.md)。
[本地流程图](../diagrams/resume-process-identity.html) · [Archify 源](../diagrams/src/resume-process-identity.workflow.json)

## 结论与批准范围

新增独立持久 `process_bindings` 表，只记录前向观测并验证的进程来源与 owner 转移。新可信 startup 建立 root，匹配的 End(resume) 建立 pending，随后同进程、同 pane 的 Start(resume) 消费 pending 一次，完成 A → B 单跳转移。archive/adopt/restart 共用 resolver，副作用前再核对操作 claim 与身份。

用户已批准持久表、兼容迁移代码、前向采集与安全失效。不授权真实数据库迁移、真实会话操作、安装、commit/push 或发布。本次设计产物仅在本地生成，所有 A/B 都是假数据，不含真实会话或聊天内容。

旧只读 events 链方案永久否决：历史事件无 PID/start provenance，不能用当前 slot 身份反填历史 generation。`events.detail` 作为唯一授权存储的替代方向也已撤回：events 默认保留 7 天，不能让长运行进程的操作身份随事件清理消失。这两个方向均不是当前实现方案。

## 问题与已核实基线

同进程 A `/resume` 到 B 后，OS argv 仍可能是 `--session-id A`，registry 却正确地把当前运行归属转到 B。A 为 closed/resume，B 为 running；B 的聊天历史仍只有 B。原 `still_it` 只接受本 slot 当前或历史 session ID，因而误拒绝 B 的 archive/adopt/restart。不能通过把 A 加进 B 历史或取消身份检查绕过问题。

以下锚点基于改动前 `3b5b005d9f98fc5dfc40b6af9f51895a4b3334ce`，是问题证据，不是新绑定能力的实现证明。

| 代码仓相对位置 | 证据 |
|---|---|
| `crates/moss-core/src/registry.rs:294–312,348–378` | Resume 选择目标 slot，更新 PID/start、running 与 tmux 位置；COALESCE 保留旧值不能当作可信来源证明。 |
| `crates/moss-core/src/registry.rs:380–407` | 记录 started 与目标历史，关闭同进程其他 running slot。 |
| `crates/moss-core/src/registry.rs:448–470,490–510` | End 校验当前 session/进程，记录符合状态转换的 ended。 |
| `crates/moss-core/src/registry.rs:1443–1451,1674–1681` | events 没有事件发生时的 PID/start provenance。 |
| `crates/moss-core/src/registry.rs:1875–1881,1884–1915` | switched 防御性关闭不构成 End(resume) 证明；touch_and_repair 可无 started 改绑。 |
| `crates/cc-moss/src/cmd/adopt.rs:140–150` | 旧 still_it 只以本 slot ID 历史验证 argv。 |
| `crates/cc-moss/src/cmd/archive.rs:151–187` | busy 门禁后检查身份，再标记有意退出并发 SIGTERM。 |
| `crates/cc-moss/src/cmd/restart.rs:836–853,1093–1107` | 多个 restart 路径复用身份检查，并有 pane/进程额外守卫。 |
| `crates/moss-core/tests/state_machine.rs:365–422` | 已覆盖同进程 resume 的登记变化与缺 End 时 switched；未证明操作授权。 |

## 持久数据与不变量

`process_bindings` 独立于聊天历史、launch_args 和短期 events。概念字段为绑定 generation、PID、非空 start、启动 session、当前 owner slot/session、必要 socket/pane、状态和单跳标记；具体 SQL 名称与索引由实现合同落定，图不预设未实施的列定义。

- generation 表示**已观测绑定代次**，不是 OS exec 代次，也不能只用 PID 猜测。
- 同一 process generation 只能有唯一 owner，约束与事务检查共同保证；目标 slot 必须与当前绑定一致。
- 绑定只保存安全身份，不保存完整 argv、正文或凭据。拒绝日志也不得输出完整命令行。
- 仅真实 SessionStart hook 加 OS 身份采集的专用入口可以签发 root；普通 source 枚举、普通 apply Startup、reconcile 或 adopt 补登记不能签发。初始 ID、hook session 与实际进程入口必须一致。程序识别检查实际可执行入口或明确支持的解释器脚本，启动参数精确解析；任意 argv 单词含 `claude` 不构成证据。
- 缺 PID/start、冲突参数、入口或身份不可观测时，不签发可信绑定。孤立 resume 不创建 root。
- A/B 保持独立 slot 与聊天历史。保留 origin=A 是进程来源证明，不是把 A 并入 B 的对话。

## 生命周期与事务边界

| 当前状态 / 输入 | 同事务行为与门禁 |
|---|---|
| 无绑定 + 真实 SessionStart hook 的可信 Startup(A) | 通过 OS 身份采集校验程序入口、启动 ID、hook session、PID/非空 start 与位置；与 slot 写入一起建立 root，owner=A，hop=0。普通 source 枚举、普通 apply Startup、reconcile/adopt 补登记不得签发。 |
| root + End(A,resume) | 严格匹配 generation、进程及当前 owner 后变 pending_resume；不是见到任意 End 就授权。 |
| pending_resume + Start(B,resume) | 校验同进程与 socket/pane、唯一新 owner；一次性消费 pending，转 transferred，owner=B、origin=A、hop=1。slot 与 binding 同事务更新。 |
| 重复 Startup/End/Start | 幂等；不得重建 generation、重新签发已失效绑定或把已消费 hop 清零。 |
| 缺 pending、仅 switched、孤立 resume、多跳 | 不授权跨 slot；相关旧绑定按合同失效，不能补造来源。 |
| 无证据 repair、PID/start/session/pane 变化、跨代接管 | 修改身份字段的同一事务使相关绑定 invalid，避免留下可信残留。 |
| 不支持的后续 clear/compact | 旧跨 slot 绑定失效；不因本 slot 历史检查成功而复活它。 |
| invalid/consumed 且同 PID/start 为 Alive 或 Unknown | 保留墓碑，拒绝迟到 Startup 重新签发；失效不等于删除。 |
| 精确 PID/start 已确认 Dead | 才允许清理对应墓碑；Unknown 不等于 Dead，不以仅 PID 或探测失败推断退出。 |

实施必须逐一审计所有写 PID/start/session/pane/owner 的入口，包括 repair、adopt/restore/restart 和防御性关闭。只在正常 Resume 路径更新表而遗漏其他写入者，不满足合同。

## 共享校验与操作前重核

resolver 是共享的只读身份入口；可信绑定的签发与失效由 hook/registry 写入事务负责，不由 resolver 根据旧事件猜测补写。

1. 保留本 slot 当前/历史 ID 的既有快路及 fork 兼容语义，但统一程序身份校验，不能通过快路复活 invalid 的跨 slot 绑定。
2. 对 argv 为 A、目标为 B 的跨 slot 情况，在同一读事务检查 binding 与 slots：generation、PID/非空 start、origin、owner、状态、hop 和 socket/pane 必须一致，owner/live 归属必须唯一。
3. 与当前 OS 进程、可执行入口和 pane 归属核对。shell 包装路径保留已有祖先/终端检查，不能只比 pane 字符串，也不能混淆不同 socket 的同名 pane。
4. 返回有限期的身份快照，不返回永久许可。信号、输入、重建等副作用前，操作 claim 与 generation/owner/OS 身份重核必须匹配；变化、冲突、未知或读取失败即拒绝，不在旧授权上切换目标。
5. archive 的 idle/attention、adopt 的接管条件、restart 的模式与退出后二阶段守卫全部保留。身份有效不等于业务操作允许；working/finished 也不是身份凭据。

绑定有效且 events 被清空时，仍可通过身份验证。旧现场没有 binding 时则清楚说明“无法确认启动关联”，拒绝跨 slot 操作；不得自动回填或强制归档。

DB 事务与 OS 副作用不是同一原子操作。重核缩短 TOCTOU 窗口，但不能保证完全消除重核后切换，也不能以 PID/start 检测同 PID、同执行体 exec。设计明确保留这些不可观测边界，不承诺零竞态。

## 迁移、旧 writer 与回退

- 新版本打开旧数据库：以兼容迁移建立空的 process_bindings，保留原 slots/session history，不扫描旧 events/argv 回填授权。
- events 超过 7 天、裁剪或全部删除：有效持久绑定不受影响；绑定不能以 events 的存在作为存活条件。
- schema 版本必须阻止旧 writer 修改新版数据库，优先复用已有拒写机制。必须用旧版本打开新版临时数据库验证拒写；只在新代码联表检查不够。
- 旧二进制回退不意味着可继续写新 schema。真实升级前需独立安装授权、SQLite backup 与明确回退方案；不能靠删除绑定表或降低版本号伪装回退。
- 本轮仅在临时数据库验证迁移；不得打开真实数据库触发升级，不操作真实 tmux/会话。

## 验收矩阵

| 场景 | 预期 |
|---|---|
| 新可信 startup → 匹配 End resume → 同进程 Start resume | root → pending → transferred；三操作身份判定一致，历史不合并。 |
| 缺/异 PID/start、非支持程序、冲突 argv、不同 socket/pane | 不签发或失效；拒绝操作。 |
| 无 pending、缺 End、仅 switched、孤立 resume | 不追认跨 slot 身份。 |
| 多 owner/live、重复事件、多跳、目标换代 | 歧义拒绝；重复不重置 generation/hop；多跳不授权。 |
| repair 无可信来源、旧结果重放、后续不支持的 clear/compact | 同事务 invalid，不能复活旧绑定。 |
| events 清空或超过 7 天 | 有效绑定仍可验证。 |
| 旧 DB 升级 | 绑定表为空，slots/history 保持，不回填旧记录。 |
| 旧 writer 打开新 DB | schema 保护拒写；不留下未失效可信绑定。 |
| claim 后 owner/generation/OS/pane 变化 | 副作用前失败，零信号/零输入/零重建。 |
| invalid/consumed 后迟到 Startup，同 PID/start 为 Alive 或 Unknown | 墓碑保留，不能重签 root；精确 Dead 才清理。 |
| 仅 source=Startup、reconcile/adopt 补登记，无真实 hook 身份采集 | 不签发 root，不追认旧现场。 |
| archive B 隔离端到端 | 旧 PID 确证退出、B 归档成功，A/B history 独立。 |
| 本 slot/fork/clear 兼容及 restart 二阶段保护 | 原安全与业务门禁不回归。 |

产品验证命令以 T2.14 spec 为准，包括 `cargo test resume_identity`、`cargo test process_binding`、隔离 archive/adopt/restart 测试与 Rust 全量质量检查。本设计子任务不运行产品实现测试，不声称这些验收已经通过。

## 图表验证

AMEND-1 图已通过 Archify 自动 validate/deliver/check/browser-check；未人工视觉审阅。首次图布局检查发现 mainPath 列方向回退，改为操作主路径后通过，未改变设计语义。验证回执仅存仓内 `.git/sdd/t214-design-evidence/`，不进入公开档案；无网络发布。自动图检查只证明产物可渲染，不等于产品安全评审或实现验收。
