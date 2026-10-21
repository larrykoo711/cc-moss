# T2.7 请求级探针结果（2026-10-21）

## 结论

Claude Code 2.1.306；本机假 Anthropic API、合成会话、假凭据；bare 与非 bare safe-mode 各五组，10/10 通过。controller独立读取 results.json 和 cleanup.json，确认下表及资源退出断言。

| 保存模型 A 后的恢复条件 | 首个主对话请求模型 | 两模式 |
|---|---|---|
| 顶层 model=B | B | 通过 |
| settings env ANTHROPIC_MODEL=B | B | 通过 |
| 顶层 B + env B + --model C | C | 通过 |
| 无固定模型 + --model default | 与 fresh 默认相同、非 A | 通过 |
| model=opus + OPUS family=B + --model opus | B | 通过 |

每组均断言：seed请求确为A；resume返回原UUID；首个resume请求携带seed历史；同一transcript保留原前缀且追加。没有使用 fork-session。

## 证据与复跑

脚本：`scripts/provider-model-probe.py`，只绑定127.0.0.1临时端口；临时HOME/config/state/cwd、清空继承环境、禁MCP/工具、自定义、外部代理拒绝CONNECT；不打印headers，不复制真实配置。

```sh
uv run --no-project --offline python -I scripts/provider-model-probe.py --claude "$(command -v claude)" --mode bare
uv run --no-project --offline python -I scripts/provider-model-probe.py --claude "$(command -v claude)" --mode safe
```

成功运行通过 sdd_run detach：bare rc=0（2.5s）、safe rc=0（3.0s）。原始证据分别在本机临时目录 `cc-moss-model-probe-cs3csski`、`cc-moss-model-probe-y5in3gze`；包括 environment/results/requests/cleanup。这些是本机证据，不是可跨机器引用的仓内附件。

首轮 rc=1：fresh-default 请求最后一条消息可能是无标记的附加提示，采样器错误地只查最后一条。修复为逆序定位用户标记后复验；没有改变模型预期。首轮证据 `cc-moss-model-probe-skiu__wg` 保留，不能计作通过。

三轮共33个CLI子进程均已退出；服务线程/监听均关闭。controller核验两轮成功的cleanup断言。

## 对实现的约束

- 显式 `--resume ID --model TARGET` 可用于渠道应用；default分支在两种隔离模式下有请求级依据。
- 顶层B在此实验中也覆盖了保存A；不能继续笼统断言“只要resume就一定沿用旧模型”。真实故障仍需检查实际启动来源/环境，不能用文档或本实验倒推用户当时环境。
- 明确区分启动确认、模型选择与生产API可用性。假API接受合成模型，并不证明用户渠道配置的默认模型可用。

## 未验证

真实OAuth/订阅切换、生产模型可用性、项目/local/managed覆盖、继承启动环境、常驻交互会话热切、400重试路径。不得将本实验扩写成这些路径已通过。
