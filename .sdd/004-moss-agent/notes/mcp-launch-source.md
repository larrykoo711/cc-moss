# 恢复配置来源保真

日期：2026-10-22。实施与验证以T2.15 spec为准，本文是本轮设计与已确认根因，不冒充最终发布结果。

## 证据

用户反馈恢复会话后MCP失效。真实Claude在临时HOME、假stdio命令下完成四次配置查询：未设置CLAUDE_CONFIG_DIR时只发现HOME/.claude.json中的服务，显式设置为HOME/.claude时只发现该目录内.claude.json中的服务。假命令退出导致连接失败是预期，证明的是配置选择而非服务连通性；未使用真实凭据或生产MCP。

代码先将未设置/显式默认归一为同一settings.json路径，hook只保存provider source；resume_argv再取父目录无条件设置变量，restore/adopt/restart共享这一损失。CLI白名单另会丢弃mcp-config/strict/settings/setting-sources；原防参数注入动机仍有效，不能直接放开原始argv。

## 决策

在现有Snapshot JSON保存独立启动来源三态，缺字段是Unknown。hook在配置解析之前采集Unset/Explicit；坏settings保留来源且标settings_valid=false，不作为空provider比较。配置观察只更新seen keys，不覆盖初始来源；来源metadata不参与模型revision。

- Unset：通过env -u清除tmux/父shell继承变量。
- Explicit：验证并回放原值，等于默认目录也保持显式。
- Unknown：不按source路径猜来源，不从活进程完整环境补采，不终止旧进程；明确拒绝。
- 不支持CLI来源：精确argv仅记录固定参数名，绝不落内联JSON、凭据或完整环境；不可安全回放则预检拒绝。

三入口复用来源builder，adopt在改名/占有/TERM之前检查，restore在启动前，restart在busy入队前检查。来源可回放性与动态模型解析分离，排队执行仍读最新模型，取消不被未知来源阻止。

## 评审补充

扫描必须区分参数与参数值，值为双横线不能提前截断，值为选项名不能伪造来源。未理解的结构保守拒绝。旧Unknown不改成Unset以迁就旧测试，原配置与会话保留正例须继续验证；JSON restore的既有退出码语义不因测试误设而改变。

## 迁移边界

已错误恢复并重新记录为Explicit的会话无法自动恢复最初Unset；本次不移动/合并任何全局配置，不复制凭据，不自动重开用户会话。明确Unknown拒绝是行为变化，双语指南及更新说明需同步。不保证MCP可连，只保证不静默切换配置来源。

## 设计链接

[来源回放时序](../diagrams/mcp-launch-source.html) · [图源](../diagrams/src/mcp-launch-source.sequence.json) · [验收合同](../tasks/in-progress/T2.15-mcp-launch-source.spec.md)。图为本轮设计，不冒充旧发布代码已有能力；验证回执留在私有证据目录，不提交本机绝对路径。
