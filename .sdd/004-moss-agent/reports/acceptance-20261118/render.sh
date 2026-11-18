#!/usr/bin/env bash
# Off-screen acceptance matrix for the 16 historical criteria (cc-moss 004 T2.29).
# No mouse/keyboard on the real screen; temp HOME; demo/fixture data only.
set -uo pipefail
APP="$(cd "$(dirname "$0")/../../../../apps/macos" && pwd)"
BIN="$APP/.build/debug/CCMoss"
OUT="${1:?out dir}"
FIX="$APP/Tests/fixtures"
DEMO="$APP/demo"
HOMEDIR="$(mktemp -d)"
trap 'rm -rf "$HOMEDIR"' EXIT
mkdir -p "$OUT"
LOG="$OUT/render-output.txt"
: > "$LOG"
NOW=2026-10-22T12:05:00Z

r() { # r <spec> <name> <lang> [args...]
  local spec=$1 name=$2 lang=$3; shift 3
  mkdir -p "$OUT/$spec"
  local png="$OUT/$spec/$name.png"
  local res
  res=$(cd "$HOMEDIR" && env HOME="$HOMEDIR" CC_MOSS_LANG="$lang" "$BIN" --render "$png" --now "$NOW" "$@" 2>&1)
  local rc=$?
  printf '%s/%s rc=%s %s\n' "$spec" "$name" "$rc" "$(echo "$res" | tr '\n' ' ' | cut -c1-400)" >> "$LOG"
}

themes() { # call r for light and dark
  local spec=$1 name=$2 lang=$3; shift 3
  r "$spec" "$name-light" "$lang" "$@"
  r "$spec" "$name-dark" "$lang" "$@" --dark
}

# T2.16 provider/current-state card: normal / pending / failure (+ the other fixture states), 320/540
for id in NORMAL PLAN BUSY QUEUED UNKNOWN FAILURE ORDINARY; do
  for w in 320 540; do
    themes T2.16 "$id-$w" zh --demo "$FIX/provider-recovery.json" --provider restart --provider-plan "$id" --provider-card-width "$w"
  done
done

# T2.17 restore-source choice: untouched / default / directory / cancel, 320/540
for c in none default directory cancel; do
  for w in 320 540; do
    extra=(); [ "$c" != none ] && extra=(--restore-source-choice "$c")
    themes T2.17 "$c-$w" zh --demo "$FIX/restore-source-choice.json" --failure SOURCE:config_source_unknown ${extra[@]+"${extra[@]}"} --restore-source-width "$w"
  done
done

# T2.18 full row + expanded panel: unselected / default / directory / long path, 320/540
LONG="/Users/demo/configuration folders/a very long team profile name that keeps going/nested settings for this one project"
for c in unselected default directory longpath; do
  for w in 320 540; do
    case $c in
      unselected) extra=(--restore-source-choice advanced) ;;
      default) extra=(--restore-source-choice default) ;;
      directory) extra=(--restore-source-choice directory) ;;
      longpath) extra=(--restore-source-choice directory --restore-source-path "$LONG") ;;
    esac
    themes T2.18 "$c-$w" zh --demo "$FIX/restore-source-choice.json" --failure SOURCE:config_source_unknown ${extra[@]+"${extra[@]}"} --restore-source-width "$w"
  done
done

# T2.19 source-first card: Known (explicit, unset) / Unknown / missing / damaged / directory choice, 320/540
for c in READY UNSET UNKNOWN MISSING DAMAGED DIRCHOICE; do
  for w in 320 540; do
    if [ "$c" = DIRCHOICE ]; then
      args=(--restore-source-plan UNKNOWN --restore-source-choice directory)
    else
      args=(--restore-source-plan "$c")
    fi
    themes T2.19 "$c-$w" zh --demo "$FIX/restore-source-plan.json" "${args[@]}" --restore-source-width "$w"
  done
done

# T1.4 four card kinds in the answer area, zh/en x light/dark
for lang in zh en; do
  themes T1.4 "cards-$lang" "$lang" --demo "$DEMO/sessions.$lang.json" --asks "$DEMO/asks.$lang.json" --page queue
done

# T2.1 / T2.2 / T2.4 settings → Moss tab: status and switches
for lang in zh en; do
  for m in running restarted off failing nodaemon; do
    themes T2.2 "settings-moss-$m-$lang" "$lang" --demo "$DEMO/sessions.$lang.json" --settings moss --moss "$m" --daemon running --height 1060
  done
  for d in running stopped unknown; do
    case $d in running) m=running ;; stopped) m=nodaemon ;; unknown) m=offline ;; esac
    themes T2.1 "settings-daemon-$d-$lang" "$lang" --demo "$DEMO/sessions.$lang.json" --settings moss --daemon "$d" --moss "$m" --height 1060
  done
done

# T2.3 Moss page: empty / chat (action bar) / typing / queued+failed / off, zh/en x light/dark
for lang in zh en; do
  themes T2.3 "empty-$lang" "$lang" --demo "$DEMO/sessions.$lang.json" --page moss --moss running
  themes T2.3 "chat-actions-$lang" "$lang" --demo "$DEMO/sessions.$lang.json" --page moss --moss-chat "$DEMO/moss-chat.$lang.json" --moss running
  themes T2.3 "chat-typing-$lang" "$lang" --demo "$DEMO/sessions.$lang.json" --page moss --moss-chat "$DEMO/moss-chat.$lang.json" --moss running --moss-typing
  themes T2.3 "queued-$lang" "$lang" --demo "$DEMO/sessions.$lang.json" --page moss --moss-chat "$DEMO/moss-sending-message.$lang.json" --moss queued
  themes T2.3 "thinking-$lang" "$lang" --demo "$DEMO/sessions.$lang.json" --page moss --moss-chat "$DEMO/moss-chat.$lang.json" --moss thinking
  themes T2.3 "failing-$lang" "$lang" --demo "$DEMO/sessions.$lang.json" --page moss --moss-chat "$DEMO/moss-chat.$lang.json" --moss failing
  themes T2.3 "off-$lang" "$lang" --demo "$DEMO/sessions.$lang.json" --page moss --moss off
done

# T2.4 checkup bubble + brief on the Moss page
for lang in zh en; do
  themes T2.4 "checkup-$lang" "$lang" --demo "$DEMO/sessions.$lang.json" --page moss --moss-chat "$DEMO/moss-checkup.$lang.json" --moss running
done

echo "done: $(find "$OUT" -name '*.png' | wc -l | tr -d ' ') images; nonzero: $(grep -c ' rc=[^0]' "$LOG")"
