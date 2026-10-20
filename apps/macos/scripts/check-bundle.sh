#!/usr/bin/env bash
# Check a built cc-moss.app before it ships: its version is the Cargo
# workspace's (the CLI and the app always match), it stays out of the Dock,
# it carries its icon and the changelog, and its ad-hoc signature holds.
#
#   scripts/check-bundle.sh [path/to/cc-moss.app]   (default: build/cc-moss.app)
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
root="$(cd "$here/../.." && pwd)"
app="${1:-$here/build/cc-moss.app}"
plist="$app/Contents/Info.plist"
fail() { echo "check-bundle: $*" >&2; exit 1; }

[[ -f "$plist" ]] || fail "no $plist (run scripts/bundle.sh first)"
read_key() { /usr/libexec/PlistBuddy -c "Print :$1" "$plist" 2>/dev/null || true; }

cargo="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)"
[[ -n "$cargo" ]] || fail "no version in $root/Cargo.toml"
# The short version is the whole version (0.2.0-rc.1); CFBundleVersion must
# be plain numbers, so it is the part before any pre-release suffix (0.2.0).
have="$(read_key CFBundleShortVersionString)"
[[ "$have" == "$cargo" ]] || fail "CFBundleShortVersionString is '$have' but Cargo.toml says $cargo"
build="${cargo%%-*}"
have="$(read_key CFBundleVersion)"
[[ "$have" == "$build" ]] || fail "CFBundleVersion is '$have' but should be $build (Cargo.toml says $cargo)"
[[ "$have" =~ ^[0-9]+(\.[0-9]+)*$ ]] || fail "CFBundleVersion '$have' is not plain numbers"
[[ "$(read_key CFBundleIdentifier)" == "io.github.larrykoo711.cc-moss" ]] || fail "unexpected CFBundleIdentifier"
[[ "$(read_key LSUIElement)" == "true" ]] || fail "LSUIElement is not true: the app would show in the Dock"
icon="$(read_key CFBundleIconFile)"
[[ -n "$icon" ]] || fail "no CFBundleIconFile"
[[ -s "$app/Contents/Resources/$icon.icns" || -s "$app/Contents/Resources/$icon" ]] || fail "icon $icon is not in Contents/Resources"
[[ -x "$app/Contents/MacOS/cc-moss" ]] || fail "no executable at Contents/MacOS/cc-moss"
changelog="$app/Contents/Resources/changelog.json"
[[ -s "$changelog" ]] || fail "no changelog.json in Contents/Resources (the \"what's new\" card reads it)"
grep -q '"versions"' "$changelog" || fail "changelog.json has no versions"
codesign --verify --strict "$app" || fail "the signature does not verify"
# Captured first: `grep -q` quitting early would fail the pipe under pipefail.
signature="$(codesign -dv "$app" 2>&1)"
[[ "$signature" == *"Signature=adhoc"* ]] || fail "not ad-hoc signed"
echo "check-bundle: ok ($cargo, $(lipo -archs "$app/Contents/MacOS/cc-moss"))"
