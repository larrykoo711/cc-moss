#!/usr/bin/env bash
# Render the Homebrew formula for a release from its SHA256SUMS file.
# release.yml runs it to bump the tap, and in a dry run to check the result.
#
#   packaging/homebrew/render.sh <tag> <SHA256SUMS>  > cc-moss.rb
set -euo pipefail
[[ $# -eq 2 ]] || { echo "usage: render.sh <tag> <SHA256SUMS>" >&2; exit 2; }
tag="$1"
sums="$2"
here="$(cd "$(dirname "$0")" && pwd)"
version="${tag#v}"
sum() {
  local s
  s="$(awk -v f="cc-moss-${tag}-$1.tar.gz" '$2 == f { print $1 }' "$sums")"
  [[ "$s" =~ ^[0-9a-f]{64}$ ]] || { echo "render.sh: no checksum for cc-moss-${tag}-$1.tar.gz in $sums" >&2; exit 1; }
  echo "$s"
}
# One assignment each, so a missing checksum stops the script (set -e).
darwin_arm="$(sum aarch64-apple-darwin)"
darwin_x64="$(sum x86_64-apple-darwin)"
linux_arm="$(sum aarch64-unknown-linux-musl)"
linux_x64="$(sum x86_64-unknown-linux-musl)"
out="$(sed -e "s/@VERSION@/$version/g" \
           -e "s/@SHA_DARWIN_ARM@/$darwin_arm/" \
           -e "s/@SHA_DARWIN_X64@/$darwin_x64/" \
           -e "s/@SHA_LINUX_ARM@/$linux_arm/" \
           -e "s/@SHA_LINUX_X64@/$linux_x64/" \
           "$here/cc-moss.rb.in")"
if grep -q '@[A-Z_]*@' <<<"$out"; then
  echo "render.sh: unfilled placeholder" >&2
  exit 1
fi
printf '%s\n' "$out"
