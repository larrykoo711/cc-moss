#!/usr/bin/env bash
# Run `swift` with a toolchain that can build SwiftUI apps. SwiftUI's macros
# (@State and friends) ship with Xcode, not with the Command Line Tools, so
# when the active developer directory is the CLT, borrow Xcode's for this
# command only (the global `xcode-select` setting is left alone).
set -euo pipefail
if [[ -z "${DEVELOPER_DIR:-}" ]] && [[ "$(xcode-select -p 2>/dev/null)" == *CommandLineTools* ]]; then
  for xcode in /Applications/Xcode.app /Applications/Xcode-beta.app; do
    if [[ -d "$xcode/Contents/Developer" ]]; then
      export DEVELOPER_DIR="$xcode/Contents/Developer"
      break
    fi
  done
fi
cd "$(dirname "$0")/.."
exec swift "$@"
