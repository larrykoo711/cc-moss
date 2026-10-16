#!/usr/bin/env bash
# Build cc-moss.app: a release build of the CCMoss target wrapped in an app
# bundle, ad-hoc signed (Apple silicon runs only signed code). The version is
# the Cargo workspace's, so the app and the CLI always match.
#
#   scripts/bundle.sh [--arch arm64|x86_64|universal]   → build/cc-moss.app
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
root="$(cd "$here/../.." && pwd)"
arch="native"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --arch) arch="$2"; shift 2 ;;
    *) echo "bundle.sh: unknown argument $1" >&2; exit 2 ;;
  esac
done

version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)"
[[ -n "$version" ]] || { echo "bundle.sh: no version in Cargo.toml" >&2; exit 1; }

build_args=(build -c release --product CCMoss)
case "$arch" in
  native) ;;
  arm64) build_args+=(--arch arm64) ;;
  x86_64) build_args+=(--arch x86_64) ;;
  universal) build_args+=(--arch arm64 --arch x86_64) ;;
  *) echo "bundle.sh: --arch must be arm64, x86_64 or universal" >&2; exit 2 ;;
esac
"$here/scripts/swift.sh" "${build_args[@]}"
bin_dir="$("$here/scripts/swift.sh" "${build_args[@]}" --show-bin-path)"

app="$here/build/cc-moss.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin_dir/CCMoss" "$app/Contents/MacOS/cc-moss"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>io.github.larrykoo711.cc-moss</string>
  <key>CFBundleName</key><string>cc-moss</string>
  <key>CFBundleDisplayName</key><string>cc-moss</string>
  <key>CFBundleExecutable</key><string>cc-moss</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>LSUIElement</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
  <key>NSHumanReadableCopyright</key><string>MIT License</string>
</dict>
</plist>
PLIST
codesign --force --sign - --timestamp=none "$app"
echo "$app"
