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


# A rejection must be the intended validation failure, not a crashed command.
expect_diagnostic() {
  local name="$1" file="$2" want="$3" out rc=0
  out="$(bash "$CHECK" "$file" 2>&1)" || rc=$?
  if [ "$rc" -eq 1 ] && [[ "$out" == *"$want"* ]]; then
    pass "$name"
  else
    fail "$name (status $rc; expected diagnostic: $want; got: $out)"
  fi
}

mutate() {
  local name="$1" expression="$2" diagnostic="$3"
  sed "$expression" "$SOURCE" > "$TMP/mutant.k9"
  expect_diagnostic "$name" "$TMP/mutant.k9" "$diagnostic"
}

expect_diagnostic 'missing input fails with a readable error' "$TMP/missing.k9" 'cannot read'
printf '\357\273\277' > "$TMP/bom.k9"
cat "$SOURCE" >> "$TMP/bom.k9"
expect_diagnostic 'rejects a UTF-8 BOM' "$TMP/bom.k9" 'line 1 must be exactly K9!'
: > "$TMP/empty.k9"
expect_diagnostic 'rejects an empty file' "$TMP/empty.k9" 'line 1 must be exactly K9!'
sed 's/$/\r/' "$SOURCE" > "$TMP/crlf.k9"
expect_diagnostic 'rejects CRLF after recognizing the magic' "$TMP/crlf.k9" 'requires LF line endings'
mutate 'rejects tabs' 's/^  schema:/\tschema:/' 'tabs are forbidden'
mutate 'rejects YAML document separators' '/^project:/i ---' 'YAML document separators'
mutate 'requires an immediately following SPDX header' '2s/^# SPDX-.*/# ordinary comment/' 'SPDX header must immediately follow'
# Delete both SPDX lines; deleting only one leaves another valid SPDX header.
sed '2,3d' "$SOURCE" > "$TMP/no-spdx.k9"
expect_diagnostic 'rejects a missing SPDX header' "$TMP/no-spdx.k9" 'SPDX header must immediately follow'
mutate 'rejects an unknown top-level section' '$a unknown_section:' 'unknown top-level section'
mutate 'rejects an unindented body field' 's/^  build_system:/build_system:/' 'not a section or an indented field'
mutate 'requires metadata section' '/^metadata:$/d' 'metadata, project, and invariants sections are required'
mutate 'requires project section' '/^project:$/d' 'metadata, project, and invariants sections are required'
mutate 'requires invariants section' '/^invariants:$/d' 'metadata, project, and invariants sections are required'
mutate 'requires current schema name' 's/schema: k9-coordination/schema: other/' 'missing required v1.2.0 field'
mutate 'requires manual generation marker' 's/generated_by: manual/generated_by: automatic/' 'missing required v1.2.0 field'
mutate 'requires project identity' 's/name: "IDApTIK"/name: "Other"/' 'missing required v1.2.0 field'
mutate 'rejects an alternate deed target' 's/deed_ref: "IDApTIK_chora.deed"/deed_ref: "Other.deed"/' 'missing required v1.2.0 field'
mutate 'requires last_updated' '/^  last_updated:/d' 'metadata.last_updated'
mutate 'rejects a non-ISO date shape' 's/last_updated: .*/last_updated: 25-10-2026/' 'metadata.last_updated'
mutate 'rejects an incomplete earlier invariant' '/^    rule: "Keep descriptive project facts/d' 'invariant descriptive-state-home lacks rule or reason'
mutate 'rejects an incomplete final invariant' '/^    reason: "A skipped check/d' 'missing or incomplete invariant'
mutate 'rejects unsupported severity' 's/^    severity: critical$/    severity: warning/' 'illegal invariant severity'

# Optional severities are legal, and all three normative severity values work.
for severity in critical high moderate absent; do
  if [ "$severity" = absent ]; then
    sed '/^    severity:/d' "$SOURCE" > "$TMP/valid.k9"
  else
    sed "s/^    severity: .*/    severity: $severity/" "$SOURCE" > "$TMP/valid.k9"
  fi
  if bash "$CHECK" "$TMP/valid.k9" >/dev/null; then
    pass "accepts invariant severity $severity"
  else
    fail "accepts invariant severity $severity"
  fi
done

# Relative input paths are resolved from the repository, even from another cwd.
if (cd "$TMP" && bash "$CHECK" coordination.k9 >/dev/null); then
  pass 'resolves relative paths from the repository root'
else
  fail 'resolves relative paths from the repository root'
fi

printf 'coordination-k9-test: %d checks, %d failure(s)\n' "$checks" "$failures"
[ "$failures" -eq 0 ]
