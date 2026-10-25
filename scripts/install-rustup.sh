#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Provision rustup, which is deliberately NOT a Guix package.
#
# Why this script exists
# ----------------------
# docs/dev-notes/toolchain-authority.md says rust-toolchain.toml is the only
# authority for the Rust version, and that the environment must provide rustup
# rather than a bare `rust` so nothing can shadow that pin. The second half of
# that plan did not survive contact with Guix: `rustup` is not in the Guix
# package collection (packages.guix.gnu.org, 2026-10-25, "Unknown package:
# rustup"), and the obvious fallback -- Guix's `rust` package -- is exactly the
# thing the rule forbids.
#
# So rustup joins Idris2 and trufflehog on the bootstrap list: a tool the
# environment cannot supply, provisioned on demand and excused by name in
# scripts/toolchain-parity.sh. The invariant is unchanged and now easier to
# state: Guix provides NO Rust at all, so a Guix shell cannot shadow
# rust-toolchain.toml by accident.
#
# Options
# -------
#   --profile minimal|default   toolchain profile (default: default, which
#                               includes the docs that `rustup doc` serves)
#   --dry-run                   print what would happen, change nothing
#
# Environment
# -----------
#   RUSTUP_HOME, CARGO_HOME  honoured; defaults are rustup's own (~/.rustup,
#                            ~/.cargo) so this script does not invent a second
#                            install location.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_DIR"

log()  { printf '\033[1;34m[rustup]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[rustup]\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31m[rustup]\033[0m %s\n' "$*" >&2; exit 1; }

PROFILE="default"
DRY_RUN=0
while [ $# -gt 0 ]; do
  case "$1" in
    --profile) PROFILE="${2:?--profile needs a value}"; shift 2 ;;
    --profile=*) PROFILE="${1#*=}"; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    -h|--help) sed -n '2,30p' "$0"; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done

# The pin this environment must end up with. Read from rust-toolchain.toml so
# there is one authority even for this script's own idea of "done".
[ -f rust-toolchain.toml ] || die "rust-toolchain.toml not found in $REPO_DIR"
CHANNEL="$(sed -n 's/^channel[[:space:]]*=[[:space:]]*"\(.*\)"/\1/p' rust-toolchain.toml | head -1)"
[ -n "$CHANNEL" ] || die "rust-toolchain.toml has no channel pin"
log "pinned channel: $CHANNEL"

# A bare `rustc` on PATH that is not rustup's shim is the failure mode this
# script exists to prevent, so report it rather than silently competing.
if command -v rustc >/dev/null 2>&1 && ! command -v rustup >/dev/null 2>&1; then
  warn "a rustc is on PATH but rustup is not: $(command -v rustc)"
  warn "that is the shadow this environment forbids. Remove it, or let rustup"
  warn "come first on PATH, before running this script for real."
fi

if command -v rustup >/dev/null 2>&1; then
  HAVE="$(rustup show active-toolchain 2>/dev/null || true)"
  case "$HAVE" in
    "$CHANNEL"*)
      log "already provisioned: $HAVE"
      # Components and targets can still be missing on an older install.
      if [ "$DRY_RUN" -eq 0 ]; then
        rustup component add rustfmt clippy rust-src rust-analyzer >/dev/null 2>&1 || true
        rustup target add wasm32-unknown-unknown >/dev/null 2>&1 || true
      fi
      exit 0
      ;;
    *) log "rustup present but active toolchain is '${HAVE:-none}'" ;;
  esac
else
  log "rustup not on PATH; will install from https://sh.rustup.rs"
fi

if [ "$DRY_RUN" -eq 1 ]; then
  log "--dry-run: would run rustup-init --default-toolchain none -y --no-modify-path"
  log "--dry-run: then 'rustup show' resolves $CHANNEL from rust-toolchain.toml"
  exit 0
fi

# --- prerequisites -----------------------------------------------------------
# rustup needs a linker to run its own post-install smoke test, and needs curl
# or wget to fetch. Check before downloading anything.
missing=()
command -v curl >/dev/null 2>&1 || command -v wget >/dev/null 2>&1 || missing+=("curl or wget")
command -v cc >/dev/null 2>&1 || command -v gcc >/dev/null 2>&1 || command -v clang >/dev/null 2>&1 \
  || missing+=("a C compiler (rustup's post-install check links a binary)")
if [ "${#missing[@]}" -ne 0 ]; then
  die "missing prerequisites: ${missing[*]}
  with the Guix environment: guix shell -D -f guix.scm -- just install-rustup
  on Debian/Ubuntu:           sudo apt-get install -y curl build-essential"
fi

# --- install -----------------------------------------------------------------
# --no-modify-path: the environment is entered explicitly (`guix shell`, `mise`,
#                   or a devcontainer), so rustup must not quietly edit rc files
#                   to make itself ambient. An install that changes the user's
#                   shell without being asked is a second, invisible authority.
# --default-toolchain none: do not install a toolchain that rust-toolchain.toml
#                   did not ask for. `rustup show` below resolves the pin.
# -y: non-interactive.
log "installing rustup (no default toolchain; rust-toolchain.toml decides)"
if command -v curl >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --no-modify-path --profile "$PROFILE" --default-toolchain none
else
  wget -qO- https://sh.rustup.rs \
    | sh -s -- -y --no-modify-path --profile "$PROFILE" --default-toolchain none
fi

# rustup installs into ~/.cargo, which this non-login shell may not have on PATH.
for d in "${CARGO_HOME:-$HOME/.cargo}/bin"; do
  case ":$PATH:" in *":$d:"*) ;; *) PATH="$d:$PATH" ;; esac
done
export PATH

command -v rustup >/dev/null 2>&1 || die "rustup installed but not found; add \${CARGO_HOME:-~/.cargo}/bin to PATH"

# --- resolve the pin ---------------------------------------------------------
# `rustup show` reads rust-toolchain.toml in the current directory and installs
# the requested channel, components and targets. This is the moment the pin
# becomes real, so its output is shown rather than swallowed.
log "resolving $CHANNEL from rust-toolchain.toml (downloads if needed)"
rustup show

log "done. Verify with: rustc --version   (expect: rustc ${CHANNEL})"
