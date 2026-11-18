# Historical acceptance review — 2026-11-17

## Result

The user requested a full evidence check of the 16 previously completed specs with unticked criteria, rather than a blanket exception. Each criterion was compared with existing implementation, tests and archived reports. 81 of 97 criteria have supporting evidence and are now checked with a dated source map in the corresponding spec. The remaining 16 are still unchecked. Their presence in `done/` records the old delivery state; it does not make the current stage-clean gate pass.

This is historical evidence reconciliation, not a claim that every old external probe or manual UI test was rerun today. This round's Rust suite (1243 passed, 0 failed, 8 ignored) and Swift suite (1002 passed) support the cited current automated tests. Old-version external behavior is explicitly limited to the version in its original report.

| Spec | Supported / total | Remaining evidence |
|---|---:|---|
| T1.1 | 4/4 | None; original external probe report, not a new probe |
| T1.2 | 5/5 | None; historical report plus current implementation/tests |
| T1.3 | 6/6 | None; distinguish original performance/real-session results from current tests |
| T1.4 | 4/7 | Four card types across languages/themes; off-screen answer click output; real panel answer/resume receipt |
| T2.1 | 5/6 | Moss settings status screenshot |
| T2.2 | 7/8 | Moss settings state/switch screenshots |
| T2.3 | 12/13 | Moss page state/language/theme matrix |
| T2.4 | 5/8 | Daily/report switch behavior and decision reminders; two switches screenshot; checkup action sequence |
| T2.5 | 5/6 | Unarchive immediately updates the app; clean-all opens Moss |
| T2.6 | 6/7 | Both directions of provider switching and row/all restart UI receipts |
| T2.14 | 4/5 | Offloaded quality evidence tied to the final code subtree |
| T2.15 | 4/4 | None; cited source propagation/negative tests and prior reports |
| T2.16 | 4/5 | Normal/pending/failure narrow/wide light/dark matrix |
| T2.17 | 4/5 | Default/directory selection and cancellation narrow/wide light/dark matrix |
| T2.18 | 3/4 | Full row and expanded panel 320/540 light/dark, long path/unselected cases |
| T2.19 | 3/4 | Full 320/540 light/dark source-status and directory-choice matrix |

No missing screenshot was fabricated and no checkbox was changed merely because a release or Done label exists. Current off-screen record-target capability testing does not replace the unrelated older UI acceptance requirements.

## Decision and next work

On 2026-11-17 the user chose to organize and push the feature branch, leaving main unchanged and stage-clean explicitly incomplete. No skip-gate approval was granted. T2.29 tracks the 16 outstanding evidence items. Existing P3/P4/R1 backlog remains open.

The old HTML board also carries pre-existing design-hook warnings (gradient text, contrast and small text). This round changes status text only: no new ignore rules or suppression were added, and those design warnings are not claimed resolved.

## Follow-up — 2026-11-18

The user chose to close the 16 remaining criteria with evidence. All 16 are now ticked: off-screen render matrices and in-process probes for the visual and app-action items, the final merge tree's local L2 for T2.14, and the user's own checks on this Mac for the three real-device items. Evidence, reproduction scripts and limits: `acceptance-20261118/README.md`. Result: 97 of 97.
