#!/usr/bin/env bash
# Build cc-moss.app, put it in ~/Applications and start it. Notifications
# only work for an app that lives in an Applications folder (see
# .sdd/002-menubar/notes/probes.md), so testing from build/ is not enough.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
app="$("$here/scripts/bundle.sh" | tail -1)"
dest="$HOME/Applications/cc-moss.app"
# Quit the running copy and wait for it to go: opening while it is still
# shutting down fails with LaunchServices error -609. (Never `pkill cc-moss`:
# that name is also the CLI the hooks run.)
osascript -e 'tell application id "io.github.larrykoo711.cc-moss" to quit' >/dev/null 2>&1 || true
for _ in $(seq 1 50); do
  pgrep -f "/cc-moss.app/Contents/MacOS/cc-moss" >/dev/null || break
  sleep 0.1
done
mkdir -p "$HOME/Applications"
rm -rf "$dest"
cp -R "$app" "$dest"
open "$dest"
echo "started $dest"
