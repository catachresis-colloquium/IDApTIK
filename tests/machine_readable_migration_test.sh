#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Offline structural checks for the DEED/K9/coordination migration.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
failures=0
checks=0

check() { # $1 label; rest = test command
  local label="$1"
  shift
  checks=$((checks + 1))
  if "$@"; then
    printf '  ok: %s\n' "$label"
  else
    printf '  FAIL: %s\n' "$label" >&2
    failures=$((failures + 1))
  fi
}

check 'single project deed is present' test -r "$ROOT/IDApTIK_chora.deed"
check 'pointer-only startup deed is present' test -r "$ROOT/0-AI-MANIFEST.deed"
check 'root coordination protocol is present' test -r "$ROOT/coordination.k9"
check 'canonical contractile directory is present' test -d "$ROOT/.machine_readable/contractiles"
check 'legacy split descriptiles directory is absent' test ! -e "$ROOT/.machine_readable/descriptiles"
check 'legacy k9iser wrapper is absent' test ! -e "$ROOT/k9iser.toml"

for verb in adjust bust dust intend must trust; do
  check "$verb K9 component exists" test -r "$ROOT/.machine_readable/contractiles/$verb/$verb.k9.ncl"
  check "$verb Nickel runner exists" test -r "$ROOT/.machine_readable/contractiles/$verb/$verb.ncl"
done

legacy_files="$(find "$ROOT" -type f -name '*.a2ml' -not -path "$ROOT/.git/*" -print -quit)"
if [ -z "$legacy_files" ]; then
  checks=$((checks + 1))
  printf '  ok: no legacy A2ML files remain in the working tree\n'
else
  printf '  FAIL: legacy A2ML file remains: %s\n' "$legacy_files" >&2
  failures=$((failures + 1))
fi

for f in \
  "$ROOT/AGENTS.md" \
  "$ROOT/.well-known/ai.txt" \
  "$ROOT/justfile" \
  "$ROOT/.github/workflows/ci.yml" \
  "$ROOT/.machine_readable/contractiles/Justfile" \
  "$ROOT/launcher.sh" \
  "$ROOT/docs/PROJECT-GOVERNANCE-BINDING.adoc"; do
  if grep -qiE 'a2ml|k9iser|Mustfile\.a2ml' "$f"; then
    printf '  FAIL: active legacy-format reference remains in %s\n' "${f#$ROOT/}" >&2
    failures=$((failures + 1))
  else
    printf '  ok: no active legacy-format reference in %s\n' "${f#$ROOT/}"
  fi
  checks=$((checks + 1))
done

if grep -qiE 'Mustfile\.a2ml|k9iser[[:space:]]+(validate|generate|build)|validate-a2ml' "$ROOT/scripts/must_check.sh"; then
  printf '  FAIL: the Must runner still reads or invokes a legacy parser\n' >&2
  failures=$((failures + 1))
else
  printf '  ok: the Must runner executes K9 probe IDs through the host adapter\n'
fi
checks=$((checks + 1))


# Exercise the new K9 probe adapter against tiny isolated repository trees.
# No real Cargo, network, or application process is needed for these clauses.
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
MUST_ROOT="$TMP/must fixture"
mkdir -p "$MUST_ROOT/scripts" "$MUST_ROOT/.machine_readable/contractiles/must"
cp "$ROOT/scripts/must_check.sh" "$MUST_ROOT/scripts/"
COMPONENT="$MUST_ROOT/.machine_readable/contractiles/must/must.k9.ncl"

clause() {
  printf '      { id = "%s", severity = "%s", probe = "%s" },\n' "$1" "$2" "$3"
}

expect_must() {
  local label="$1" expected_status="$2" diagnostic="$3" out rc=0
  out="$(cd "$MUST_ROOT" && bash scripts/must_check.sh 2>&1)" || rc=$?
  check "$label: exit status" test "$rc" -eq "$expected_status"
  check "$label: diagnostic" grep -Fq -- "$diagnostic" <<< "$out"
}

expect_must 'missing component' 2 'is missing'
: > "$COMPONENT"
expect_must 'empty component' 2 'no K9 clauses were parsed'
printf 'K9!\n{ declaration = { clauses = [] } }\n' > "$COMPONENT"
expect_must 'no recognized clauses' 2 'no K9 clauses were parsed'
for malformed in \
  '{ id = "broken", severity = "critical" },' \
  '{ id = "broken", probe = "license-present" },' \
  '{ id = "", severity = "critical", probe = "license-present" },' \
  '{ id = "broken", severity = "critical", probe = "" },' \
  '{ id = "broken", severity = "critical", probe = "unterminated'; do
  printf '%s\n' "$malformed" > "$COMPONENT"
  expect_must 'malformed clause' 2 'malformed K9 clause at line 1'
done

clause 'license' critical license-present > "$COMPONENT"
expect_must 'missing required license' 1 '1 K9 clauses, 1 critical failure(s), 0 warning failure(s)'
touch "$MUST_ROOT/LICENSE"
expect_must 'registered probe succeeds' 0 '1 K9 clauses, 0 critical failure(s), 0 warning failure(s)'
clause 'readme' warning readme-present > "$COMPONENT"
expect_must 'warning is reported without blocking' 0 '1 K9 clauses, 0 critical failure(s), 1 warning failure(s)'
clause 'unknown' critical not-a-registered-probe > "$COMPONENT"
expect_must 'unknown probe fails closed' 1 'no host-side probe is registered for K9 clause not-a-registered-probe'

{
  clause 'same-id' critical license-present
  clause 'same-id' critical readme-present
} > "$COMPONENT"
expect_must 'duplicate clause IDs rejected before execution' 2 'duplicate K9 clause ID same-id'
{
  clause 'first-id' critical license-present
  clause 'second-id' critical license-present
} > "$COMPONENT"
expect_must 'distinct clause IDs may share a probe' 0 '2 K9 clauses, 0 critical failure(s), 0 warning failure(s)'
{
  clause 'license' critical license-present
  clause 'missing-readme' critical readme-present
  clause 'warning' warning adr-records
} > "$COMPONENT"
expect_must 'critical and warning results are aggregated' 1 '3 K9 clauses, 1 critical failure(s), 1 warning failure(s)'

# Both clause IDs and probe strings are data, never executable shell text.
# A future eval/bash -c regression would create the planted marker.
clause '$(touch injected-id)' critical '$(touch injected-probe)' > "$COMPONENT"
expect_must 'probe text cannot execute shell' 1 'no host-side probe is registered'
check 'no command substitution from clause ID' test ! -e "$MUST_ROOT/injected-id"
check 'no command substitution from probe value' test ! -e "$MUST_ROOT/injected-probe"


# The conformance wrapper must invoke each pinned validator and propagate
# failures. Stub only its external tools; execute the real orchestration script.
MR_ROOT="$TMP/conformance fixture"
STANDARDS="$TMP/standards fixture"
MR_BIN="$TMP/bin"
mkdir -p "$MR_ROOT/scripts" "$MR_ROOT/tests" "$MR_ROOT/.machine_readable/contractiles" \
  "$STANDARDS/1-formats/deed/tools" "$STANDARDS/1-formats/k9/tools" "$MR_BIN"
cp "$ROOT/scripts/machine_readable_check.sh" "$MR_ROOT/scripts/"
cp "$ROOT/IDApTIK_chora.deed" "$ROOT/0-AI-MANIFEST.deed" "$MR_ROOT/"
cp -r "$ROOT/.machine_readable/contractiles/"* "$MR_ROOT/.machine_readable/contractiles/"
: > "$STANDARDS/1-formats/deed/tools/deed_lint.js"
cat > "$MR_BIN/git" <<'STUB'
#!/usr/bin/env bash
[ "$*" = "-C $IDAPTIK_STANDARDS_DIR rev-parse HEAD" ] || exit 91
printf '%s\n' "${MR_STANDARDS_SHA:-8e48b1caade08a552646adeab66558ac43b4360d}"
STUB
cat > "$MR_BIN/node" <<'STUB'
#!/usr/bin/env bash
if [ "${2:-}" = --self-test ]; then step=deed-self-test; else step=deed-lint; fi
printf '%s\n' "$step" >> "$MR_LOG"
printf '%s\n' "$@" >> "$MR_ARGS"
[ "${MR_FAIL:-}" != "$step" ] || exit 17
STUB
cat > "$STANDARDS/1-formats/k9/tools/k9-validate.sh" <<'STUB'
#!/usr/bin/env bash
if [ "$1" = --self-test ]; then step=k9-self-test; else step=k9-L2; fi
printf '%s\n' "$step" >> "$MR_LOG"
printf '%s\n' "$@" >> "$MR_ARGS"
[ "${MR_FAIL:-}" != "$step" ] || exit 17
STUB
cat > "$MR_BIN/nickel" <<'STUB'
#!/usr/bin/env bash
if [ "$1" = --version ]; then printf 'Nickel %s\n' "${MR_NICKEL_VERSION:-1.18.0}"; exit; fi
[ "$1" = typecheck ] || exit 92
printf 'typecheck:%s\n' "$(basename "$(dirname "$2")")" >> "$MR_LOG"
printf '%s\n' "$@" >> "$MR_ARGS"
[ "${MR_FAIL:-}" != nickel-typecheck ] || exit 17
STUB
for entry in 'scripts/coordination_k9_check.sh' 'tests/machine_readable_migration_test.sh'; do
  cat > "$MR_ROOT/$entry" <<'STUB'
#!/usr/bin/env bash
step="$(basename "$0")"
printf '%s\n' "$step" >> "$MR_LOG"
[ "${MR_FAIL:-}" != "$step" ] || exit 17
STUB
done
# Unexpected tool downloads are a fixture failure, never a real request.
printf '#!/usr/bin/env bash\nexit 93\n' > "$MR_BIN/curl"
chmod +x "$MR_BIN/"*
MR_LOG="$TMP/conformance.log"
MR_ARGS="$TMP/conformance.args"

run_conformance() {
  : > "$MR_LOG"
  : > "$MR_ARGS"
  MR_OUTPUT=""
  MR_STATUS=0
  MR_OUTPUT="$(env PATH="$MR_BIN:$PATH" IDAPTIK_STANDARDS_DIR="$STANDARDS" \
    K9_NICKEL="$MR_BIN/nickel" MR_LOG="$MR_LOG" MR_ARGS="$MR_ARGS" \
    "$@" bash "$MR_ROOT/scripts/machine_readable_check.sh" 2>&1)" || MR_STATUS=$?
}

run_conformance
check 'conformance wrapper succeeds with all validators' test "$MR_STATUS" -eq 0
cat > "$TMP/expected-order" <<'EXPECTED'
deed-self-test
deed-lint
k9-self-test
k9-L2
typecheck:adjust
typecheck:bust
typecheck:dust
typecheck:intend
typecheck:must
typecheck:trust
coordination_k9_check.sh
machine_readable_migration_test.sh
EXPECTED
check 'all validators execute in dependency order' diff -u "$TMP/expected-order" "$MR_LOG"
check 'K9 validation requires L2' grep -Fxq L2 "$MR_ARGS"
check 'K9 validation requires strict mode' grep -Fxq -- --strict "$MR_ARGS"
for verb in adjust bust dust intend must trust; do
  check "$verb component passed to the validator" grep -Fxq \
    "$MR_ROOT/.machine_readable/contractiles/$verb/$verb.k9.ncl" "$MR_ARGS"
  check "$verb runner passed to Nickel" grep -Fxq \
    "$MR_ROOT/.machine_readable/contractiles/$verb/$verb.ncl" "$MR_ARGS"
done
for step in deed-self-test deed-lint k9-self-test k9-L2 nickel-typecheck \
  coordination_k9_check.sh machine_readable_migration_test.sh; do
  run_conformance "MR_FAIL=$step"
  check "propagates failure of $step" test "$MR_STATUS" -eq 17
  check "does not claim success after $step failure" test \
    "${MR_OUTPUT/DEED, K9 components, Nickel runners, and coordination checks passed/FOUND}" = "$MR_OUTPUT"
  if [ "$step" != machine_readable_migration_test.sh ]; then
    check "stops before final layout gate after $step failure" \
      test "$(tail -n 1 "$MR_LOG")" != machine_readable_migration_test.sh
  fi
done
run_conformance MR_STANDARDS_SHA=wrong-revision
check 'rejects a standards checkout at the wrong revision' test "$MR_STATUS" -eq 2
check 'wrong standards revision runs no validators' test ! -s "$MR_LOG"
run_conformance MR_NICKEL_VERSION=1.16.0
check 'rejects an incompatible Nickel version' test "$MR_STATUS" -eq 2
check 'wrong Nickel version reports required version' grep -Fq 'must be Nickel 1.18.0' <<< "$MR_OUTPUT"
run_conformance "K9_NICKEL=$TMP/not-executable"
check 'rejects a missing explicit Nickel executable' test "$MR_STATUS" -eq 2
check 'missing Nickel reports the configuration error' grep -Fq 'K9_NICKEL is not executable' <<< "$MR_OUTPUT"
for verb in adjust bust dust intend must trust; do
  for suffix in k9.ncl ncl; do
    missing="$MR_ROOT/.machine_readable/contractiles/$verb/$verb.$suffix"
    mv "$missing" "$TMP/held-component"
    run_conformance
    check "missing $verb.$suffix fails closed" test "$MR_STATUS" -eq 1
    check "missing $verb.$suffix names the input" grep -Fq "$missing" <<< "$MR_OUTPUT"
    mv "$TMP/held-component" "$missing"
  done
done


for validator in 1-formats/deed/tools/deed_lint.js 1-formats/k9/tools/k9-validate.sh; do
  mv "$STANDARDS/$validator" "$TMP/held-validator"
  run_conformance
  check "missing $validator is an infrastructure failure" test "$MR_STATUS" -eq 2
  check "missing $validator runs no validators" test ! -s "$MR_LOG"
  mv "$TMP/held-validator" "$STANDARDS/$validator"
done

# Do not let an ambient tool installation mask fail-closed prerequisite paths.
MIN_BIN="$TMP/minimal-bin"
mkdir -p "$MIN_BIN"
for tool in bash dirname mktemp rm; do
  ln -s "$(command -v "$tool")" "$MIN_BIN/$tool"
done
run_conformance "PATH=$MIN_BIN"
check 'missing Node fails closed' test "$MR_STATUS" -eq 2
check 'missing Node names the required tool' grep -Fq 'node is required' <<< "$MR_OUTPUT"
ln -s "$MR_BIN/node" "$MIN_BIN/node"
run_conformance "PATH=$MIN_BIN"
check 'missing Git fails closed' test "$MR_STATUS" -eq 2
check 'missing Git names the required tool' grep -Fq 'git is required' <<< "$MR_OUTPUT"

printf 'machine-readable-migration-test: %d checks, %d failure(s)\n' "$checks" "$failures"
[ "$failures" -eq 0 ]
