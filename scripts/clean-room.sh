#!/usr/bin/env bash
# clean-room.sh — `cc-moss install` and `doctor` on a fresh Ubuntu, in Docker.
#
# A new user with passwordless sudo and zsh, no package lists, no tmux, no
# starship: install must get everything in place with real apt and the real
# starship installer, doctor must come out green, and a second install must
# change nothing.
#
# Usage: scripts/clean-room.sh <linux cc-moss binary> [image]   (default ubuntu:24.04)
set -euo pipefail

bin="${1:?usage: clean-room.sh <linux cc-moss binary> [image]}"
image="${2:-ubuntu:24.04}"
dir="$(cd "$(dirname "$bin")" && pwd)"

docker run --rm -i -v "$dir":/in:ro -e BIN="/in/$(basename "$bin")" "$image" bash -s <<'SCENARIO'
set -euo pipefail
step() { printf '\n=== %s ===\n' "$*"; }

step "a fresh machine"
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq && apt-get install -y -qq zsh sudo ca-certificates curl >/dev/null
useradd -m -s /bin/zsh dev
echo 'dev ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/dev
install -m 755 "$BIN" /usr/local/bin/cc-moss
# A stand-in for Claude Code: this run checks the terminal setup, not Claude.
printf '#!/bin/sh\nexit 0\n' > /usr/local/bin/claude && chmod +x /usr/local/bin/claude
# New machines often have no package lists yet.
rm -rf /var/lib/apt/lists/*
as_dev() { su - dev -c "export CC_MOSS_LANG=en; $1"; }

step "install"
as_dev 'cc-moss install --yes --with aliases' || echo "(install reported a problem; doctor decides)"

step "doctor (in a new interactive zsh)"
as_dev 'zsh -i -c "cc-moss doctor"' 

step "install again changes nothing"
again=$(as_dev 'zsh -i -c "cc-moss install --yes --with aliases"')
echo "$again"
grep -q "already up to date" <<<"$again"
# Core tools must be there; an optional one an older release lacks (eza on
# Debian 12) is allowed to stay missing.
if grep -E "Still missing:.*(tmux|starship)" <<<"$again"; then echo "core tools still missing"; exit 1; fi

step "the aliases and the prompt work"
as_dev 'zsh -i -c "alias catp ff; command -v starship tmux"'
echo
echo "CLEAN ROOM PASSED"
SCENARIO
