# Record-target review — 2026-11-17

## Scope and decision

Review baseline: `53a87a7..0b2ab02`, plus the stage-clean corrections. This is a merge candidate, not a release. No new feature iteration is started.

Two independent reads covered registry lifecycle safety and Rust/Swift target resolution. The controller checked the cited callers and restoration planner. The original implementation had been installed locally for a trial before this review; passing its earlier tests did not establish safety at the missing boundaries.

| Finding | Evidence | Disposition |
|---|---|---|
| Recency-based closure can hide the only restorable stopped copy | `Registry::close_superseded_duplicates` versus `restore::plan`; existing `duplicates_pick_the_newest_restorable_copy` allows an older copy when the newer folder is missing | User approved withdrawing the new no-running-copy cleanup on 2026-11-17. Registry implementation returns to the base revision; stopped copies remain selectable. |
| Archived copies and restoration claims are not interchangeable with unarchived stopped rows | Cleanup snapshot and write predicate did not preserve those distinctions | Withdrawn new cleanup, not papered over with more heuristics. Pre-existing live-copy cleanup races remain outside this patch and are not claimed fixed. |
| A legal session name can equal another record's id | `restore::resolve` put record identity before names | Explicit `IdentityConflict` refusal prevents a wrong-row action. Follow-up review found a remaining Medium availability limitation when no unique tmux/name exists: T2.28 must provide an explicit identity channel. User chose to push the feature branch without merging on 2026-11-17. |
| New App can use an old standalone CLI | `Session.target` changed but no positive capability existed | `ls --json` advertises `record_target: 1`; App checks the latest snapshot before affected commands and shows an update requirement. No name or conversation-id fallback. |
| Batch restart receipts still match by conversation id | `ArchiveViews.restartAll` and `RecoveryOperation.complete` | Existing limitation, not caused by selecting a single record. Not expanded into a batch protocol change in this patch; retain as a follow-up, not a completed fix. |
| Test fixtures contained identifiers copied from the live report | `restore_plan.rs` fixture | Replace with synthetic ids, including the abbreviated negative case. No live record identifiers in the corrected source. |
| An unrelated current test fixture retained a private project label | `moss/inbox.rs` named separator fixture | Replace with `demo-docs`; keep behavior and tests unchanged. |

## Validation

- Rust review run: 1243 passed, 0 failed, 8 ignored; fmt and clippy passed. Ledger check `twin-records-rust-review`, tree `785bd3e6` (2026-11-17). The earlier run had one failing newly revised test because its unnamed, silent fixture was closed by the existing silent-session rule. The fixture was corrected to represent two named copies; targeted and full reruns passed.
- Swift review run: 1002 tests in 135 suites passed via `apps/macos/scripts/swift.sh test`. Ledger check `twin-records-swift-review`, tree `6576323f`.
- These are development-tree results. Final clean-commit L2 evidence and PR checks must be recorded before merge.
- No tests operated on the user's real session registry or default tmux server. No mouse or keyboard automation was used.

## Public-content and history audit

Local Gitleaks 8.30.1 scanned all refs: 118 commits, about 15.74 MB. Its 11 findings were individually checked against source: all are deliberately synthetic redaction test inputs (test key strings, truncated key envelopes, sample JWT and authorization headers). No real credential was identified by this scan. This is not proof that no secret exists.

Private-word checks scanned 2196 unique reachable blobs and commit bodies, plus current tracked files. Public URLs and documented shell command names were distinguished from private labels. Current-source private label and live fixture identifiers were corrected. Some older public commits still contain historical private project/tool labels. A separate local-only backup ref contains additional pre-publication material; it must never be pushed. Public refs reached 103 commits at this audit; the other reachable commits belong to local retained history.

No main branch or release tag history was rewritten. Correcting the tip or squashing the feature branch does not remove old objects from remote caches, pull requests or existing clones. Historical removal needs a separate coordinated decision; do not describe this audit as a clean historical privacy bill.

Raw reports, the private word list and matching excerpts stay local, outside tracked files. Public reports deliberately contain no original private labels, real session ids, private paths, IPs or credentials. Author attribution and intentionally published repository/site identities are not treated as leaked content.

## Convention review

- Entry point: session commands continue through `moss_core::restore::resolve`; App uses the shared record-target policy.
- Coverage: resolver, exact archive selection, snapshot capability and Swift capability decoding have regression assertions. Capability-guard call sites were manually inspected.
- Documentation: guides in both languages and Unreleased notes describe full record ids, collision refusal, CLI capability checks and withdrawal of recency-based cleanup.
- DRY: no per-command copy of the id resolver; App gate reuses the latest snapshot instead of spawning extra probes.
- Dead code: no new unreferenced exports identified in this correction.
- Comments: existing English source-comment convention retained.
- Stable hooks: no visual markup or native control identity changes. The historical HTML board's existing style warnings were not suppressed; this round updates status text, not its design.

## Self-review and handoff

Earlier work bypassed spec-first and independent pre-install review and copied real ids into tests. This report records the deviation rather than pretending that the current review preceded implementation. Existing workflow rules already prohibit those actions; the remedy is applying those rules and retaining concrete evidence, not weakening them.

No skill change is required for the five-question self-review: the failures are already covered by review-before-merge, synthetic fixtures, tree-bound verification and honest historical acceptance. The evidence-ledger run was blocked by a stale, ignored prior-round CHECKPOINT; that file was preserved under `.git/stage-clean/`, allowing the existing ledger to run. No repository ignore rule or test gate was disabled.
