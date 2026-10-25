#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Offline mutation tests for the repository-specific coordination.k9 guard.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CHECK="$ROOT/scripts/coordination_k9_check.sh"
SOURCE="$ROOT/coordination.k9"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
checks=0
failures=0

pass() { checks=$((checks + 1)); printf '  ok: %s\n' "$1"; }
fail() { printf '  FAIL: %s\n' "$1" >&2; failures=$((failures + 1)); }

if bash "$CHECK" "$SOURCE" >/dev/null; then
  pass 'accepts the current v1.2.0 coordination file'
else
  fail 'accepts the current v1.2.0 coordination file'
fi

expect_reject() {
  local name="$1" file="$2"
  if bash "$CHECK" "$file" >/dev/null 2>&1; then
    fail "$name (mutant was accepted)"
  else
    pass "$name"
  fi
}

cp "$SOURCE" "$TMP/bad-magic.k9"
sed -i '1s/^K9!$/ K9!/' "$TMP/bad-magic.k9"
expect_reject 'rejects leading whitespace before K9!' "$TMP/bad-magic.k9"

cp "$SOURCE" "$TMP/wrong-version.k9"
sed -i 's/^  schema_version: 1.2.0$/  schema_version: 1.1.0/' "$TMP/wrong-version.k9"
expect_reject 'rejects a non-current coordination schema version' "$TMP/wrong-version.k9"

cp "$SOURCE" "$TMP/bad-indent.k9"
sed -i 's/^  name: "IDApTIK"$/   name: "IDApTIK"/' "$TMP/bad-indent.k9"
expect_reject 'rejects indentation that is not a two-space multiple' "$TMP/bad-indent.k9"

cp "$SOURCE" "$TMP/no-deed-ref.k9"
sed -i '/^  deed_ref: /d' "$TMP/no-deed-ref.k9"
expect_reject 'rejects a missing project.deed_ref' "$TMP/no-deed-ref.k9"

cp "$SOURCE" "$TMP/repeated-section.k9"
printf '\nmetadata:\n  schema: k9-coordination\n' >> "$TMP/repeated-section.k9"
expect_reject 'rejects a repeated or out-of-order section' "$TMP/repeated-section.k9"

printf 'coordination-k9-test: %d checks, %d failure(s)\n' "$checks" "$failures"
[ "$failures" -eq 0 ]
