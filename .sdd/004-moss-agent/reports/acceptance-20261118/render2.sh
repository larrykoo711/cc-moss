#!/usr/bin/env bash
# Second batch: one expanded card per ask kind, in-process click probes, archive and
# provider-switch receipts. Same rules: off-screen, temp HOME, demo data only.
set -uo pipefail
APP="$(cd "$(dirname "$0")/../../../../apps/macos" && pwd)"
BIN="$APP/.build/debug/CCMoss"
OUT="${1:?out dir}"
DEMO="$APP/demo"
HOMEDIR="$(mktemp -d)"
trap 'rm -rf "$HOMEDIR"' EXIT
LOG="$OUT/probe-output.txt"
: > "$LOG"
NOW=2026-10-16T10:06:00Z

r() { # r <spec> <name> <lang> [args...]; stdout+stderr of the run go to the probe log
  local spec=$1 name=$2 lang=$3; shift 3
  mkdir -p "$OUT/$spec"
  local res
  res=$(cd "$HOMEDIR" && env HOME="$HOMEDIR" CC_MOSS_LANG="$lang" "$BIN" --render "$OUT/$spec/$name.png" --now "$NOW" "$@" 2>&1)
  printf '## %s/%s rc=%s\n%s\n' "$spec" "$name" "$?" "$res" >> "$LOG"
}
themes() {
  local spec=$1 name=$2 lang=$3; shift 3
  r "$spec" "$name-light" "$lang" "$@"
  r "$spec" "$name-dark" "$lang" "$@" --dark
}

# Fixture: demo sessions with two archived rows and two rows flagged after a provider switch.
fixture() { # fixture <lang> <provider_restart value> <out>
  python3 - "$DEMO/sessions.$1.json" "$2" "$3" <<'EOF'
import json, sys
d = json.load(open(sys.argv[1]))
for s in d["sessions"]:
    if s["id"] in ("01C", "01E"):
        s["archived_at"] = "2026-10-15T09:00:00Z"
    if s["id"] in ("01B", "01D") and s.get("tmux"):
        s["provider_restart"] = sys.argv[2]
        s["provider_model"] = {"model": "example-provider/demo-model", "source": "settings", "revision": "demo-revision-1", "uncertain": False}
d["capabilities"] = {"working_directory": 1, "new_receipt": 1, "exact_restore": 1, "record_target": 1}
json.dump(d, open(sys.argv[3], "w"), ensure_ascii=False)
EOF
}

# T1.4: each ask kind expanded on its own; zh/en x light/dark
for lang in zh en; do
  for id in 3f2a9c4e81d07b55 7b1e44d0a9c35f12 c40d8e2f6b1a9375 e9a51b7c3d2f8064 52c7e0b9f4a1d683; do
    themes T1.4 "card-$id-$lang" "$lang" --demo "$DEMO/sessions.$lang.json" --asks "$DEMO/asks.$lang.json" --ask "$id" --page queue
  done
done
# T1.4: click the first option of the single-choice card; each CLI outcome
for result in answered late not-waiting refused; do
  r T1.4 "click-$result" zh --demo "$DEMO/sessions.zh.json" --asks "$DEMO/asks.zh.json" --ask 3f2a9c4e81d07b55 --page queue \
    --click 280,182 --answer "$result"
done

for lang in zh en; do
  fixture "$lang" model "$HOMEDIR/to-api.$lang.json"
  fixture "$lang" removed "$HOMEDIR/to-subscription.$lang.json"
  # T2.5: the Archived group open; bring one back; clean up all
  themes T2.5 "archived-$lang" "$lang" --demo "$HOMEDIR/to-api.$lang.json" --expand-archived
  r T2.5 "unarchive-$lang" "$lang" --demo "$HOMEDIR/to-api.$lang.json" --expand-archived --archive-action unarchive --action-session 01C
  r T2.5 "clean-all-$lang" "$lang" --demo "$HOMEDIR/to-api.$lang.json" --expand-archived --archive-action clean-all
  # T2.6: both directions; banner + flagged rows; one row's restart; restart all
  for dir in to-api to-subscription; do
    themes T2.6 "$dir-banner-$lang" "$lang" --demo "$HOMEDIR/$dir.$lang.json" --provider restart
    r T2.6 "$dir-row-restart-$lang" "$lang" --demo "$HOMEDIR/$dir.$lang.json" --provider restart --provider-plan 01B --provider-action restart
    r T2.6 "$dir-restart-all-$lang" "$lang" --demo "$HOMEDIR/$dir.$lang.json" --provider restart --archive-action restart-stale
  done
done

echo "done: $(grep -c '^## ' "$LOG") runs; nonzero: $(grep -c '^## .* rc=[^0]' "$LOG")"
