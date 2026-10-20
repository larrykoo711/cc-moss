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
# CFBundleVersion takes only numbers: a pre-release (0.2.0-rc.1) is 0.2.0
# there, while CFBundleShortVersionString shows the whole version.
build_version="${version%%-*}"

build_args=(build -c release --product CCMoss)
case "$arch" in
  native) ;;
  arm64) build_args+=(--arch arm64) ;;
  x86_64) build_args+=(--arch x86_64) ;;
  universal) build_args+=(--arch arm64 --arch x86_64) ;;
  *) echo "bundle.sh: --arch must be arm64, x86_64 or universal" >&2; exit 2 ;;
esac
# CC_MOSS_BUILD_SYSTEM=native: SwiftPM's older build system, for a machine
# where the default one stalls (its SDK stat cache waits on FSEvents).
if [[ -n "${CC_MOSS_BUILD_SYSTEM:-}" ]]; then build_args+=(--build-system "$CC_MOSS_BUILD_SYSTEM"); fi
"$here/scripts/swift.sh" "${build_args[@]}"
bin_dir="$("$here/scripts/swift.sh" "${build_args[@]}" --show-bin-path)"

# Shown by macOS the first time cc-moss opens a session in a terminal.
automation_en="cc-moss opens your sessions in the terminal you picked, in a new window."
automation_zh="cc-moss 要在你选的终端里新开一个窗口，打开你的会话。"

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
  <key>CFBundleURLTypes</key>
  <array><dict>
    <key>CFBundleURLName</key><string>io.github.larrykoo711.cc-moss</string>
    <key>CFBundleURLSchemes</key><array><string>cc-moss</string></array>
  </dict></array>
  <key>CFBundleExecutable</key><string>cc-moss</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$build_version</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>LSUIElement</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
  <key>NSHumanReadableCopyright</key><string>MIT License</string>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleLocalizations</key><array><string>en</string><string>zh-Hans</string></array>
  <key>NSAppleEventsUsageDescription</key><string>$automation_en</string>
</dict>
</plist>
PLIST
# Finder, the notification banners and System Settings show this icon
# (scripts/make-icon.sh renders it from docs/assets/icon.svg).
cp "$here/Resources/AppIcon.icns" "$app/Contents/Resources/AppIcon.icns"
# The Automation prompt in the user's language.
mkdir -p "$app/Contents/Resources/en.lproj" "$app/Contents/Resources/zh-Hans.lproj"
printf '"NSAppleEventsUsageDescription" = "%s";\n' "$automation_en" > "$app/Contents/Resources/en.lproj/InfoPlist.strings"
printf '"NSAppleEventsUsageDescription" = "%s";\n' "$automation_zh" > "$app/Contents/Resources/zh-Hans.lproj/InfoPlist.strings"
codesign --force --sign - --timestamp=none "$app"
echo "$app"
