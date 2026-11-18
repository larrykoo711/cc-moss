# Historical acceptance evidence — 2026-11-18 (T2.29)

Closes the 16 unticked criteria listed in `../historical-acceptance-review.md`. Every screenshot is an
off-screen render of the menubar app from demo or test-fixture data, under a temporary `HOME`; no mouse or
keyboard event touched the real screen, and no real session, registry or tmux server was read or changed.
The two real-device criteria were checked by the user on this Mac, as recorded below.

## How to reproduce

```sh
apps/macos/scripts/swift.sh build --product CCMoss
.sdd/004-moss-agent/reports/acceptance-20261118/render.sh  /tmp/acceptance   # 152 renders
.sdd/004-moss-agent/reports/acceptance-20261118/render2.sh /tmp/acceptance   # 48 renders + probes
uv run --with pillow python .sdd/004-moss-agent/reports/acceptance-20261118/compose.py /tmp/acceptance <sheets dir>
```

`render-output.txt` and `probe-output.txt` are the outputs of the run these sheets come from: 200 runs, all exit 0.
Sheets are labelled `<state>-<width|lang>-<theme>`; renders are halved and palette-compressed for size.

## Evidence by criterion

| Spec | Criterion | Evidence |
|---|---|---|
| T1.4 ② | Four card kinds, zh/en, light/dark | `T1.4.png` card-*: two single-choice, one multiple-choice, one two-question and one permission card (the four kinds; each offers "write your own answer"), each expanded, 20 renders |
| T1.4 ③ | Clicking an option sends the right `ask answer`; each result explained | `probe-output.txt` click-*: an in-process press on "放在路径里" sends `cc-moss ask answer 3f2a9c4e81d07b55 --choice 放在路径里 --by desktop --json`; the card after answered / late / not-waiting / refused is in `T1.4.png` click-* |
| T1.4 ⑦ | Real session: answer on the panel, the session carries on | User on 2026-11-18, this round's build: a Claude Code session asked a single-choice question with AskUserQuestion; the user answered on the To Do page and the session replied. User: "T1.4 真机验收通过" |
| T2.1 | Moss page shows the daemon's status | `T2.1.png`: Settings › Moss with the daemon running / stopped (Start button) / unknown, zh/en, light/dark |
| T2.2 | Settings shows Moss status and switch | `T2.2.png`: Moss running / restarted / off / failing / no daemon, zh/en, light/dark |
| T2.3 | Moss page states | `T2.3.png`: empty, conversation with action bar, typing bar, queued, thinking, failing, off; zh/en, light/dark |
| T2.4 ⑥ | Daily once, same request as the button; switches work | Rust tests `daily_checkup_once_a_day`, `daily_checkup_off`, `daily_brief_off_is_silent_unless_asking` (`crates/cc-moss/src/cmd/checkup.rs`). User on 2026-11-18 sent the brief-off daily request to Moss: "安静，没弹通知". A decision still arriving with options was seen in the ⑧ checkup (brief on); the brief-off-plus-decision combination was not separately observed |
| T2.4 ⑦ | Two switches in Settings | `T2.2.png` / `T2.1.png`: Daily checkup and Daily brief under Checkup |
| T2.4 ⑧ | Real device: Checkup fixes, briefs, offers options | User on 2026-11-18 pressed Checkup on this Mac: "T2.4 也验证过了" |
| T2.5 | Unarchive takes effect at once; Clean Up All talks to Moss and opens its page | `T2.5.png`; `probe-output.txt`: `cc-moss unarchive --json -- 01C`, archived 2→1, notice "修发票 回到列表了。"; Clean Up All sends `cc-moss moss say --json -- 清理全部归档会话` and ends on page `moss` |
| T2.6 | Switch notice, row restart, restart all | `T2.6.png`, both directions (to an API provider: `provider_restart=model`; back to the subscription: `removed`); row restart sends `cc-moss restart --json -- 01B`; "Apply and Restart All" sends `cc-moss restart --stale --provider-default --json`; both rows then wait for session confirmation |
| T2.14 | Final fmt, clippy and full tests | Local (not offloaded) L2 on the final merge tree; see PLAN.md, this round's final L2 line |
| T2.16 | Normal / pending / failure, narrow and wide, light and dark | `T2.16.png`: seven fixture states × 320/540 × light/dark |
| T2.17 | Default / directory / cancel; no automatic choice or launch | `T2.17.png`; every `restore-source:` line in `render-output.txt` shows `calls=0`; cancel removes the card |
| T2.18 | Full row + panel, default / directory / long path / unselected | `T2.18.png`: one title, one primary action, Cancel beside it; unselected disables the primary action |
| T2.19 | Known / Unknown / missing / damaged / directory choice | `T2.19.png`: Known rows show the grey settings path without a card; the others show one card with one primary action |

## Limits

- The demo runner stands in for the CLI. These probes prove what the app sends and how it shows the
  answer, not what a real CLI does with it; the CLI side is covered by its own end-to-end tests.
- The demo runner gained one-line-per-session answers for `restart --stale` and an `unarchive`
  handler for this run. Batch receipts that match by conversation id remain the known limitation T2.26.
- Real-device checks are the user's statements on 2026-11-18. There are no screenshots of the user's
  own screen, because those would show real session names.
