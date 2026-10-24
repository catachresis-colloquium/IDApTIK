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
# The rule this file enforces is ONE PROVIDER PER TOOL:
#
#   guix.scm  provides mise, the C toolchain, system libraries, and podman
#   mise.toml is the only place a versioned tool gets on PATH
#   rustup    comes from scripts/install-rustup.sh, because Guix has no rustup
#
# The first version of this check said "guix.scm must provide every tool
# mise.toml pins", and it was wrong twice over: Guix has no `nickel` and no
# `rustup` package, so the environment it demanded could not be built. It went
# unnoticed because nothing here ever ran against a Guix daemon. The inverted
# check — guix.scm must NOT provide anything mise pins — is both truer and
# cheaper to satisfy, and it catches the shadowing defect directly.
#
# The check is structural, not version-exact: it asserts who provides what, not
# which version Guix's revision carries. That depends on the revision, and a
# check that guessed it would be wrong on most machines. A guess presented as a
# check is the defect docs/DOCUMENTATION-CLAIMS.md exists to prevent.
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

check_guix_provides_bridge() {
  # guix.scm's job is to provide the environment mise needs and then stop.
  # If mise is absent, the version table in mise.toml is decorative.
  if guix_packages | grep -qx mise; then
    pass "guix.scm provides mise (the single version-manager bridge)"
  else
    fail "guix.scm does not provide mise"
    note "Without it, mise.toml pins nothing that is actually installed."
  fi
}

check_single_provider() {
  # THE invariant of this file, and the one that was wrong before: a tool must
  # have ONE provider. If guix.scm also provides a tool that mise.toml pins,
  # there are two versions on PATH and no rule for which wins -- the same defect
  # as a bare `rust` shadowing rust-toolchain.toml, just quieter.
  local tool
  local doubled=0
  for tool in $(mise_tools | sort -u); do
    if guix_packages | grep -qx "$tool"; then
      fail "guix.scm provides '$tool' AND mise.toml pins it — two versions, no winner"
      note "Remove '$tool' from guix.scm; mise.toml is the version authority."
      doubled=$((doubled + 1))
      continue
    fi
    if excused "$tool"; then
      pass "mise pin '$tool' is excused (see note)"
      continue
    fi
    pass "mise pin '$tool' has exactly one provider (mise)"
  done
  [ "$doubled" -eq 0 ] || fail "$doubled tool(s) have two providers"
}

check_rust_authority() {
  # rust-toolchain.toml must be the only place a Rust version is chosen. Guix
  # provides no Rust at all here, so nothing in the environment can shadow the
  # pin; the check asserts that this stays true.
  local channel
  channel="$(rust_channel)"
  if [ -z "$channel" ]; then
    fail "rust-toolchain.toml has no channel pin"
  else
    pass "rust-toolchain.toml pins Rust $channel"
  fi
  if guix_packages | grep -qx rust; then
    fail "guix.scm provides a bare 'rust' package, which shadows rust-toolchain.toml"
    note "Remove it: rustup is the only correct provider here."
  else
    pass "guix.scm provides no bare 'rust' package"
  fi
}

check_bootstrap_scripts() {
  # A tool that neither Guix nor mise provides must have a script that does,
  # or it is a wish rather than a step. rustup is the newest member: it is not
  # a Guix package, so it cannot be an input to the environment that would
  # provide it.
  local f
  for f in scripts/install-rustup.sh scripts/install-idris2.sh; do
    if [ ! -s "$f" ]; then
      fail "$f is missing or empty"
      continue
    fi
    if head -1 "$f" | grep -q '^#!/usr/bin/env bash'; then
      pass "$f exists and is a bash script"
    else
      fail "$f has no bash shebang"
    fi
  done
  # The one flag that makes rustup's install reviewable: it must not edit the
  # user's rc files behind their back.
  if grep -qs -- '--no-modify-path' scripts/install-rustup.sh; then
    pass "scripts/install-rustup.sh installs with --no-modify-path"
  else
    fail "scripts/install-rustup.sh must pass --no-modify-path to rustup"
    note "An installer that edits rc files is a second, invisible authority."
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
  # doctor.sh is where versions are actually asserted at runtime. Two directions
  # matter: a tool doctor checks but mise does not pin is an unpinned tool, and
  # a tool doctor stops checking is a pin nobody verifies.
  local tool
  local pinned
  pinned="$(mise_tools | sort -u)"
  for tool in nickel just zig; do
    if grep -q "mise exec .*${tool}@" scripts/doctor.sh; then
      pass "scripts/doctor.sh asserts $tool's pinned version"
    else
      fail "scripts/doctor.sh does not assert $tool's pinned version"
    fi
    if printf '%s\n' "$pinned" | grep -qx "$tool"; then
      pass "mise.toml pins $tool, which doctor.sh checks"
    else
      fail "doctor.sh checks $tool but mise.toml does not pin it"
    fi
  done
}

check_all() {
  check_manifest_present
  check_guix_provides_bridge
  check_single_provider
  check_rust_authority
  check_bootstrap_scripts
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
               scripts/install-rustup.sh scripts/install-idris2.sh
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

  mutant "guix.scm stops providing mise"            guix.scm '/^         "mise"$/d'
  mutant "guix.scm also provides nickel"             guix.scm 's/^         "mise"$/         "mise"\n         "nickel"/'
  mutant "guix.scm provides a bare rust"             guix.scm 's/^         "mise"$/         "mise"\n         "rust"/'
  mutant "guix.scm stops providing podman"           guix.scm '/^         "podman"$/d'
  mutant "mise.toml stops pinning nickel"            mise.toml '/aqua:nickel-lang/d'
  mutant "doctor stops asserting nickel"             scripts/doctor.sh '/nickel/d'
  mutant "the Containerfile empties"                 .devcontainer/Containerfile '1,$d'
  mutant "rust-toolchain.toml loses its channel"     rust-toolchain.toml '/^channel/d'
  mutant "install-rustup.sh empties"                 scripts/install-rustup.sh '1,$d'
  mutant "install-rustup.sh may modify PATH files"   scripts/install-rustup.sh 's/--no-modify-path/--modify-path/g'

  printf 'toolchain-parity: %s controls, 10 mutants, %s failure(s)\n' \
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
