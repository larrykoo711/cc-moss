# F42/F43 共享实施纪律

- 当前分支 feature/004-provider-model-recovery；不切分支、不提交、不push（用户只授权实施与本机验收，未授权此轮提交/发版）。
- controller拥有PLAN、playground、IMPLEMENTATION_PLAN、spec生命周期；agent只改派发白名单路径。
- 先T2.7请求级探针，再产品代码；TDD给真实红/绿证据，不靠参数外观代替请求验证。
- 不碰真实~/.claude/registry/tmux/LaunchAgent/渠道设置。不复制凭据、不调用生产API。
- e2e沿用tests/common World隔离HOME/config/state/tmux；不并发格式化他人文件。cargo fmt可对自己文件rustfmt，最后controller统一fmt。
- 超2分钟命令用sdd_run.py --detach，随后wait --max120；同证据不重复跑。失败最多3尝试后上报。
- 发现契约歧义立即SendMessage并停，不擅改验收。每次保存可编译；变异只在临时副本，不在共享树。
- 保留检查日志路径、rc、当前步骤与下一步；最终通过SendMessage回传，不只写终稿。
