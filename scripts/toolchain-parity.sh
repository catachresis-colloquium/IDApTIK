#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Toolchain parity: prove that guix.scm, mise.toml and rust-toolchain.toml
# describe the same environment, or say exactly where they do not.
#
# Why this exists
# ---------------
# Guix is the environment policy. mise.toml pins the tool versions and
# rust-toolchain.toml pins Rust. Those are three declarations of one intent, and
# before this check they had already drifted: guix.scm provided neither Nickel
# nor the container runtime, and provided a bare `rust` that would silently
# shadow the pinned compiler.
#
# The check is structural, not version-exact: it proves that every tool the
# version files name is either provided by the environment or excused by name
# with a reason. It deliberately does NOT try to predict which version Guix's
# package carries — that depends on the Guix revision, and a check that guessed
# it would be wrong on most machines. A guess presented as a check is the defect
# docs/DOCUMENTATION-CLAIMS.md exists to prevent.
#
# Usage:
#   scripts/toolchain-parity.sh              # check this repository
#   scripts/toolchain-parity.sh --self-test  # prove the check can fail
set -uo pipefail
SCRIPT_ABS="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
# Operate on the repository root, or on PARITY_ROOT when the self-test points
# the check at a deliberately broken copy of it.
cd "${PARITY_ROOT:-$(dirname "$SCRIPT_ABS")/..}" || exit 1

failures=0
pass() { printf '  \033[1;32m✓\033[0m %s\n' "$1"; }
fail() { printf '  \033[1;31m✗\033[0m %s\n' "$1"; failures=$((failures + 1)); }
note() { printf '    %s\n' "$1"; }

# Tools pinned in mise.toml that this environment deliberately does not provide,
# each with the reason. Adding a name here is a decision; the reason is the
# record of it.
excused() {
  case "$1" in
    trufflehog)
      note "Go binary for the pre-push secret scan; not a build input."
      note "Installed on demand by scripts/install-git-hooks.sh."
      return 0
      ;;
    idris2)
      note "Bootstraps through Chez Scheme + GMP; provisioned on demand."
      note "See scripts/install-idris2.sh and \`just install-idris2\`."
      return 0
      ;;
    *) return 1 ;;
  esac
}

# --- extraction -------------------------------------------------------------
# mise.toml [tools] entries: `name = "pin"` or `"aqua:owner/name" = "pin"`.
# Names are normalised to their Guix package name where the two differ.
mise_tools() {
  awk '
    /^\[tools\]/ { intools = 1; next }
    /^\[/        { intools = 0 }
    intools && /=/ {
      key = $0; sub(/=.*/, "", key); gsub(/["[:space:]]/, "", key)
      if (key == "") next
      sub(/^aqua:[^/]+\//, "", key)   # aqua:nickel-lang/nickel -> nickel
      print key
    }
  ' mise.toml
}

# Package names offered by guix.scm, read out of the quoted list it passes to
# `pkg`.
guix_packages() {
  sed -n '/^(define idaptik-tools/,/^)))/p' guix.scm \
    | grep -o '"[a-z0-9][a-z0-9-]*"' \
    | tr -d '"' \
    | sort -u
}

rust_channel() {
  sed -n 's/^channel[[:space:]]*=[[:space:]]*"\(.*\)"/\1/p' rust-toolchain.toml
}

# --- checks -----------------------------------------------------------------
check_manifest_present() {
  local f
  for f in guix.scm mise.toml rust-toolchain.toml; do
    if [ -f "$f" ]; then pass "$f is present"; else fail "$f is missing"; fi
  done
}

check_guix_offers() {
  local name
  for name in rustup nickel just zig elixir erlang podman; do
    if guix_packages | grep -qx "$name"; then
      pass "guix.scm provides $name"
    else
      fail "guix.scm does not provide $name"
    fi
  done
}

check_rust_authority() {
  # rust-toolchain.toml must be the only place a Rust version is chosen. A bare
  # `rust` package in the environment would shadow it with Guix's compiler.
  local channel
  channel="$(rust_channel)"
  if [ -z "$channel" ]; then
    fail "rust-toolchain.toml has no channel pin"
  else
    pass "rust-toolchain.toml pins Rust $channel"
  fi
  if guix_packages | grep -qx rustup; then
    pass "guix.scm provides rustup (version authority stays in rust-toolchain.toml)"
  else
    fail "guix.scm does not provide rustup"
  fi
  if guix_packages | grep -qx rust; then
    fail "guix.scm provides a bare 'rust' package, which shadows rust-toolchain.toml"
    note "Remove it: rustup is the only correct provider here."
  else
    pass "guix.scm does not provide a bare 'rust' package"
  fi
}

check_every_pin_is_served() {
  local tool
  local missing=0
  for tool in $(mise_tools | sort -u); do
    if guix_packages | grep -qx "$tool"; then
      pass "mise pin '$tool' is provided by the environment"
      continue
    fi
    if excused "$tool"; then
      pass "mise pin '$tool' is excused (see note)"
      continue
    fi
    fail "mise pins '$tool' but guix.scm neither provides nor excuses it"
    missing=$((missing + 1))
  done
  if [ "$missing" -eq 0 ]; then
    pass "every mise pin is served or excused"
  fi
}

check_podman_supported() {
  if guix_packages | grep -qx podman; then
    pass "guix.scm provides podman (container runtime)"
  else
    fail "guix.scm does not provide podman"
  fi
  if [ -f .devcontainer/devcontainer.json ]; then
    pass ".devcontainer/devcontainer.json is present"
  else
    fail ".devcontainer/devcontainer.json is missing"
  fi
  if [ -f .devcontainer/Containerfile ]; then
    pass ".devcontainer/Containerfile is present"
  else
    fail ".devcontainer/Containerfile is missing"
  fi
  # A Containerfile without a FROM is not a container file. This branch used to
  # be pass-only, so an empty file was silently accepted — found by the mutant
  # below, which is the whole reason mutants are mandatory here.
  if grep -qs '^FROM' .devcontainer/Containerfile; then
    pass ".devcontainer/Containerfile has a FROM line"
  else
    fail ".devcontainer/Containerfile has no FROM line"
  fi
}

check_doctor_agrees() {
  # doctor.sh is where versions are actually asserted at runtime. If it stops
  # naming a pinned tool, the pin is decorative.
  local tool
  for tool in nickel just zig; do
    if grep -q "mise exec .*${tool}@" scripts/doctor.sh; then
      pass "scripts/doctor.sh asserts $tool's pinned version"
    else
      fail "scripts/doctor.sh does not assert $tool's pinned version"
    fi
  done
}

check_all() {
  check_manifest_present
  check_guix_offers
  check_rust_authority
  check_every_pin_is_served
  check_podman_supported
  check_doctor_agrees
}

# --- self-test --------------------------------------------------------------
# Every control is paired with a mutant: a tree in which it must fail. A check
# that cannot fail is not a check (AGENTS.md).
self_test() {
  local pristine mutants fails=0
  pristine="$(mktemp -d)"
  mutants="$(mktemp -d)"
  # shellcheck disable=SC2064
  trap "rm -rf '$pristine' '$mutants'" EXIT

  # A pristine tree: exactly the files the check reads, nothing else.
  local files=(guix.scm mise.toml rust-toolchain.toml scripts/doctor.sh
               .devcontainer/devcontainer.json .devcontainer/Containerfile)
  seed() { # seed <dir>
    local dir="$1" f
    mkdir -p "$dir/scripts" "$dir/.devcontainer"
    for f in "${files[@]}"; do cp "$f" "$dir/$f"; done
  }
  seed "$pristine"

  run_in() { ( PARITY_ROOT="$1" bash "$SCRIPT_ABS" --quiet ) >/dev/null 2>&1; }

  if run_in "$pristine"; then
    pass "self-test: the real tree passes"
  else
    fail "self-test: the real tree does not pass"
    fails=$((fails + 1))
  fi

  # Each mutant is a fresh copy with one deliberate defect. The check must fail.
  mutant() { # mutant <label> <file> <sed-expr>
    local label="$1" file="$2" expr="$3" dir
    dir="$(mktemp -d "$mutants/m.XXXXXX")"
    seed "$dir"
    sed -i "$expr" "$dir/$file"
    if run_in "$dir"; then
      fail "mutant survived: $label"
      fails=$((fails + 1))
    else
      pass "mutant killed: $label"
    fi
  }

  mutant "guix.scm stops providing nickel" guix.scm '/"nickel"/d'
  mutant "guix.scm stops providing rustup" guix.scm '/"rustup"/d'
  mutant "guix.scm provides a bare rust" guix.scm 's/^         "rustup"$/         "rustup"\n         "rust"/'
  mutant "guix.scm stops providing podman" guix.scm '/"podman"/d'
  mutant "mise.toml gains an unserved tool" mise.toml 's/^just = .*/just = "1.46.0"\nsled = "0.34.7"/'
  mutant "doctor stops asserting nickel" scripts/doctor.sh '/nickel/d'
  mutant "the Containerfile empties" .devcontainer/Containerfile '1,$d'
  mutant "rust-toolchain.toml loses its channel" rust-toolchain.toml '/^channel/d'

  printf 'toolchain-parity: %s controls, 8 mutants, %s failure(s)\n' \
    "$(declare -F | grep -c ' check_')" "$fails"
  if [ "$fails" -eq 0 ]; then
    echo "toolchain-parity: ok"
    return 0
  fi
  echo "toolchain-parity: FAILED"
  return 1
}

# --- main -------------------------------------------------------------------
quiet=0
case "${1:-}" in
  --self-test) self_test; exit $? ;;
  --quiet) quiet=1 ;;
  "") ;;
  *) echo "usage: $0 [--self-test|--quiet]" >&2; exit 2 ;;
esac

if [ "$quiet" -eq 0 ]; then
  echo "Toolchain parity: environment (guix.scm) vs versions (mise.toml, rust-toolchain.toml)"
fi

check_all

if [ "$failures" -eq 0 ]; then
  [ "$quiet" -eq 0 ] && echo "toolchain-parity: ok"
  exit 0
fi
printf 'toolchain-parity: %s problem(s)\n' "$failures" >&2
exit 1
