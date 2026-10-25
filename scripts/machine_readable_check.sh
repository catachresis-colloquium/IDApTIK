#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Validate IDApTIK's current DEED, K9 component, Nickel runner, and coordination
# files against pinned normative tools. No moving standards branch is trusted.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STANDARDS_COMMIT="8e48b1caade08a552646adeab66558ac43b4360d"
NICKEL_VERSION="1.18.0"
NICKEL_SHA256="9cba4dd65ae9915ec61f73033aafcff307a377665a83fd8f530df086763318cb"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

if ! command -v node >/dev/null 2>&1; then
  echo "machine-readable: node is required for the pinned DEED linter" >&2
  exit 2
fi
if ! command -v git >/dev/null 2>&1; then
  echo "machine-readable: git is required to fetch the pinned standards tools" >&2
  exit 2
fi

if [ -n "${IDAPTIK_STANDARDS_DIR:-}" ]; then
  STANDARDS_DIR="$IDAPTIK_STANDARDS_DIR"
  actual="$(git -C "$STANDARDS_DIR" rev-parse HEAD)"
  [ "$actual" = "$STANDARDS_COMMIT" ] || {
    printf 'machine-readable: standards checkout is %s, expected %s\n' "$actual" "$STANDARDS_COMMIT" >&2
    exit 2
  }
else
  STANDARDS_DIR="$TMP/standards"
  git init --quiet "$STANDARDS_DIR"
  git -C "$STANDARDS_DIR" remote add origin https://github.com/hyperpolymath/standards.git
  git -C "$STANDARDS_DIR" fetch --quiet --depth=1 origin "$STANDARDS_COMMIT"
  git -C "$STANDARDS_DIR" checkout --quiet --detach FETCH_HEAD
fi

DEED_LINTER="$STANDARDS_DIR/1-formats/deed/tools/deed_lint.js"
K9_VALIDATOR="$STANDARDS_DIR/1-formats/k9/tools/k9-validate.sh"
[ -r "$DEED_LINTER" ] || { echo "machine-readable: pinned DEED linter is missing" >&2; exit 2; }
[ -r "$K9_VALIDATOR" ] || { echo "machine-readable: pinned K9 validator is missing" >&2; exit 2; }

node "$DEED_LINTER" --self-test
node "$DEED_LINTER" "$REPO_DIR/IDApTIK_chora.deed" "$REPO_DIR/0-AI-MANIFEST.deed"

NICKEL=""
if [ -n "${K9_NICKEL:-}" ]; then
  NICKEL="$K9_NICKEL"
  [ -x "$NICKEL" ] || { echo "machine-readable: K9_NICKEL is not executable: $NICKEL" >&2; exit 2; }
  version="$("$NICKEL" --version 2>&1)"
  case "$version" in *"$NICKEL_VERSION"*) ;; *) printf 'machine-readable: K9_NICKEL must be Nickel %s, got %s\n' "$NICKEL_VERSION" "$version" >&2; exit 2 ;; esac
elif command -v nickel >/dev/null 2>&1 && nickel --version 2>&1 | grep -q "$NICKEL_VERSION"; then
  NICKEL="$(command -v nickel)"
else
  NICKEL="$TMP/nickel"
  url="https://github.com/tweag/nickel/releases/download/${NICKEL_VERSION}/nickel-x86_64-linux"
  curl -sSfL --proto '=https' --proto-redir '=https' --tlsv1.2 --retry 3 -o "$NICKEL" "$url"
  echo "$NICKEL_SHA256  $NICKEL" | sha256sum -c -
  chmod +x "$NICKEL"
fi

bash "$K9_VALIDATOR" --self-test --nickel "$NICKEL"

components=()
runners=()
for verb in adjust bust dust intend must trust; do
  component="$REPO_DIR/.machine_readable/contractiles/$verb/$verb.k9.ncl"
  runner="$REPO_DIR/.machine_readable/contractiles/$verb/$verb.ncl"
  [ -r "$component" ] || { echo "machine-readable: missing K9 component $component" >&2; exit 1; }
  [ -r "$runner" ] || { echo "machine-readable: missing Nickel runner $runner" >&2; exit 1; }
  components+=("$component")
  runners+=("$runner")
done

bash "$K9_VALIDATOR" --layer L2 --strict --nickel "$NICKEL" "${components[@]}"
for runner in "${runners[@]}"; do
  "$NICKEL" typecheck "$runner"
done

bash "$REPO_DIR/scripts/coordination_k9_check.sh"
bash "$REPO_DIR/tests/machine_readable_migration_test.sh"
printf 'machine-readable: DEED, K9 components, Nickel runners, and coordination checks passed\n'
