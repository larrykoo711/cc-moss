#!/usr/bin/env bash
# Regenerate Resources/AppIcon.icns from docs/assets/icon.svg. The .icns is
# checked in, so building the app needs neither this script nor a renderer;
# run it after changing the logo.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
root="$(cd "$here/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
"$here/scripts/swift.sh" "$here/scripts/make-icon.swift" "$root/docs/assets/icon.svg" "$work/AppIcon.iconset"
mkdir -p "$here/Resources"
iconutil -c icns -o "$here/Resources/AppIcon.icns" "$work/AppIcon.iconset"
echo "$here/Resources/AppIcon.icns"
