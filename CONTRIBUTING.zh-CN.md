# 参与 cc-moss

[English](CONTRIBUTING.md) · **简体中文**

欢迎！cc-moss 的初心是：机器崩了，对话不丢。让这个承诺更结实的贡献都欢迎：报 bug、改文档、翻译、补测试、写代码。

## 开始之前

- **报 bug**：用 bug 模板开 issue，附上 `cc-moss doctor` 的输出，能省掉一半来回。
- **提需求**：改动超过几行的，先开 feature request 聊一下，免得两边重复造轮子。
- **安全问题**：请不要公开开 issue，见 [SECURITY.md](SECURITY.md)。

## 本地开发

需要 Rust stable（最低版本见 `Cargo.toml` 的 `rust-version`）、`tmux`、`bash`；`zsh` 只用于检查 shell 资产语法。

```sh
git clone https://github.com/larrykoo711/cc-moss
cd cc-moss
cargo test --all
```

端到端测试在私有 tmux socket、临时 `HOME` 和假的 `claude` 上运行，不会碰你正在用的 tmux 和 `~/.claude`。

提 PR 前请跑一遍和 CI 相同的命令：

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --all
for f in crates/cc-moss/assets/zsh/*.zsh; do zsh -n "$f"; done
```

## 约定

- **双语**：面向用户的文字都有英文和简体中文（`tr!(en, zh)`）；README 和 `docs/` 里的使用手册，两种语言一起改。只会其中一种也没关系，PR 里说一声即可。
- **改行为就带测试**；修 bug 先写一个修之前会失败的测试。
- **测试不碰真实环境**：用 `tests/common` 里的隔离 socket 和临时 `HOME`。
- **不写私密信息**：代码、测试、文档、`.sdd/` 里都不出现主机名、IP、账号、密钥、本机绝对路径；示例用 `host-a`、`192.0.2.10`。
- **提交信息**：英文 Conventional Commits，一行主题加要点；PR 以 squash 合并，PR 标题就是最终提交标题。
- **欢迎 AI 协作的贡献**，但你是作者：读过 diff、跑过检查、讲得清楚。

`.sdd/` 是项目的设计档案（中文），不是参与的前置阅读。

提交贡献即表示你同意以 [MIT 许可证](LICENSE) 授权你的贡献。
