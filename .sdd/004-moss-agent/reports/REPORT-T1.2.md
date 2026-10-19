# REPORT T1.2 — new sessions start in bypass mode

Branch feature/004-T1.2-bypass (from 3764c51). Squash subject: `feat: new sessions start in bypass mode`.

## Success criteria

1. Default bypass, `--safe` skips, no repeats — `crates/cc-moss/src/cmd/new.rs` (`wants_bypass`), `main.rs` (`--safe`), `launch_args::sets_permissions` + `--safe` in NEW_OWN_BOOLEAN.
   verify: `cargo test -p cc-moss --test new_cli permission_mode` → new_defaults_to_bypass / new_safe_skips_bypass / new_does_not_repeat_permission_args ok.
2. Install writes `skipDangerousModePermissionPrompt` once, uninstall undoes only its own — `moss_core::settings_json::{accept_bypass_prompt, added_keys, removed_keys, forget_owned}`, `setup::settings_change` / `settle_owned` (record in `<state>/claude-settings-owned.json`, written only after apply succeeded), called from setup, install, install --uninstall.
   verify: `cargo test -p moss-core --test settings_json bypass_prompt` → install_accepts_bypass_prompt_once, keeps_users_own_prompt_setting ok; plus `setup_cli::setup_owns_only_the_prompt_key_it_wrote` (temp HOME, end to end, user true/false byte-identical after install+uninstall).
3. Policy / bad values — `setup::bypass_blocked_by` (user settings + managed settings, read only; `CC_MOSS_CLAUDE_MANAGED_SETTINGS` overrides the path for tests, common::bin points it at /nonexistent), doctor `permission_check` ("new sessions" row).
   verify: `new_cli::permission_mode::bypass_disabled_by_policy`, `permission_mode_rejects_unknown` ok; moss-core config tests permission_mode_* ok.
4. App switch — `apps/macos/Sources/MossKit/PermissionMode.swift`, `SettingsView.swift` GeneralSettings "New sessions" card.
   verify: `swift.sh test --filter PermissionModeTests` 4/4 passed; render screenshots (all checked):
   <scratchpad>/t12-shots/general-{en,zh}-{light,dark}.png
5. e2e — `crates/cc-moss/tests/new_e2e.rs::bypass_in_pane` ok (isolated socket, temp HOME, fake claude checks in; `--safe` pane has no flag).

Spec verify → actual test names: `new_defaults_to_bypass` etc. live in `new_cli.rs` module `permission_mode` (filter by name works with `cargo test -p cc-moss -- <name>`); `install_accepts_bypass_prompt_once`, `keeps_users_own_prompt_setting` in `moss-core/tests/settings_json.rs` module `bypass_prompt`; `bypass_in_pane` in new file `new_e2e.rs`.

Full CI after last commit: fmt/clippy/cargo test --all exit 0; swift.sh test 297 tests passed.

## Commits
6507216 feat(core) · fd8ac4b feat: new sessions start in bypass mode · 3cdf1ef feat(app) · 0763513 docs · 75bf29c fix(cli) translate --safe help · 3d4f760 feat: --safe also drops permission flags from default_args

## deliberately_skipped
- Real Claude Code warning-dialog confirmation (per spec, T1.1 probe).
- site tests not run (no node_modules; not part of CI command).

## 裁决（controller）与落实
1. setup 也写该键：保持现状。
2. `--safe` 保证本次普通模式：已改。`launch_args::without_permissions` 去掉 default_args 里的 `--dangerously-skip-permissions`、`--permission-mode <值>`、`--permission-mode=<值>`，只影响本次、不改配置。测试 `new_cli::permission_mode::new_safe_strips_bypass_from_default_args`（含下一次照常 bypass、配置文件未变）与 moss-core `strips_every_permission_flag` 通过；`--safe` 帮助（中英）、guide 中英已说明。
3. permission_mode = default 时仍写键：保持现状。

最后一个 commit 3d4f760 后工作树干净，CI 同款再跑：fmt/clippy/cargo test --all exit 0（522 passed, 0 failed）。App 未再改，swift 未重跑。
