#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Exercise the scan orchestration against an isolated Git inventory and a
# deterministic scanner double; the real jq processes the reports.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FIXTURE="$(mktemp -d)"
trap 'rm -rf "$FIXTURE"' EXIT
repo="$FIXTURE/project"
mkdir -p "$repo/scripts" "$repo/crates/app/src" "$repo/crates/app/tests" "$FIXTURE/bin"
cp "$REPO_DIR/scripts/pons_scan.sh" "$repo/scripts/pons_scan.sh"
printf 'fn implementation() {}\n' > "$repo/crates/app/src/lib.rs"
printf 'fn support() {}\n' > "$repo/crates/app/tests/test file.rs"
printf '# tier\tpath\tnote\nS\tcrates/app/src/lib.rs\timplementation\nT\tcrates/app/tests/test file.rs\ttest\n' > "$repo/crates/CREUSOT-CLASSIFICATION.tsv"
git -C "$repo" init -q
git -C "$repo" add crates
# Untracked Rust files must not silently enter the staged inventory.
printf 'fn untracked() {}\n' > "$repo/crates/app/src/untracked.rs"

cat > "$repo/scripts/creusot_ledger_check.sh" <<'LEDGER'
#!/usr/bin/env bash
set -eu
printf 'ledger %s\n' "$*" >> "$CALLS"
if [[ "${MODE:-}" == ledger_fail ]]; then
  echo 'fixture ledger failure' >&2
  exit 17
fi
[[ "$*" == all ]]
LEDGER
chmod +x "$repo/scripts/creusot_ledger_check.sh"

cat > "$FIXTURE/bin/pons" <<'PONS'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$CALLS"
mode="${MODE:-}"
if [[ "$1" == validate-report ]]; then
  [[ "$#" == 2 && -s "$2" ]]
  if [[ "$mode" == validate_fail && "$2" == */implementation.json ]]; then
    echo 'fixture validation failure' >&2
    exit 19
  fi
  jq -e . "$2" >/dev/null
  exit
fi
[[ "$1" == scan ]]
input="$2"
shift 2
case "$input" in
  */controls/positive.rs|*/controls/negative.rs)
    [[ "$*" == '--rule self-assignment --format json --require-language rust --fail-on-skips' ]]
    if [[ "$input" == */positive.rs ]]; then
      [[ "$(cat "$input")" == 'fn f() { x = x; }' ]]
      if [[ "$mode" == positive_missing ]]; then
        printf '{"findings":[]}\n'
      else
        printf '{"findings":[{"rule_id":"self-assignment"}]}\n'
      fi
    else
      [[ "$(cat "$input")" == 'fn f() { x = y; }' ]]
      if [[ "$mode" == negative_finding ]]; then
        printf '{"findings":[{"rule_id":"self-assignment"}]}\n'
      else
        printf '{"findings":[]}\n'
      fi
    fi
    ;;
  */implementation|*/test-support)
    format="$2"
    [[ "$*" == "--format $format --no-speculative --require-language rust --fail-on-skips" ]]
    [[ "$format" == json || "$format" == sarif ]]
    if [[ "$mode" == scan_fail && "$input" == */implementation && "$format" == json ]]; then
      echo 'fixture scan failure' >&2
      exit 23
    fi
    if [[ "$mode" == sarif_fail && "$input" == */implementation && "$format" == sarif ]]; then
      echo 'fixture SARIF failure' >&2
      exit 24
    fi
    if [[ "$format" == sarif ]]; then
      printf '{"version":"2.1.0","runs":[]}\n'
      exit
    fi
    count="$(find "$input" -type f -name '*.rs' | wc -l)"
    skipped='[]'
    coverage='[]'
    if [[ "$input" == */implementation ]]; then
      case "$mode" in
        wrong_count) count=$((count + 1));;
        skipped) skipped='["unparsed.rs"]';;
        coverage_failure) coverage='["rust"]';;
      esac
    fi
    if [[ "$input" == */test-support && "$mode" == test_support_skipped ]]; then
      skipped='["unparsed.rs"]'
    fi
    jq -n --argjson count "$count" --argjson skipped "$skipped" --argjson coverage "$coverage" \
      '{schema_version:"fixture",tool:"pons",execution:{},scanned:{files:$count,coverage_status:"complete",skipped:$skipped,coverage_failures:$coverage},counts:{},findings:[{"rule_id":"example"}]}'
    ;;
  *) echo "unexpected scanner input: $input" >&2; exit 1;;
esac
PONS
chmod +x "$FIXTURE/bin/pons"
export PATH="$FIXTURE/bin:$PATH"
export CALLS="$FIXTURE/calls"

run_scan() { # mode, output directory
  : > "$CALLS"
  (cd "$repo/crates/app" && MODE="$1" "$repo/scripts/pons_scan.sh" "$2") > "$FIXTURE/stdout" 2> "$FIXTURE/stderr"
}
fail() { printf 'FAIL: %s\n' "$*" >&2; cat "$FIXTURE/stderr" >&2; exit 1; }
assert_last_call() {
  local last
  last="$(tail -n 1 "$CALLS")"
  [[ "$last" == "$1" ]] || fail "expected last call '$1', got '$last'"
}
expect_failure() { # description, mode, expected final call
  local description="$1" mode="$2" last="$3" out="$FIXTURE/out-$2"
  if run_scan "$mode" "$out"; then fail "$description unexpectedly succeeded"; fi
  assert_last_call "$last"
  if [[ -f "$out/summary.txt" ]] && grep -Fq 'Heuristic scan evidence' "$out/summary.txt"; then
    fail "$description claimed complete evidence"
  fi
  printf '  ok: %s\n' "$description"
}

out="$FIXTURE/output with spaces"
run_scan normal "$out" || fail 'complete scan'
[[ "$(head -n 1 "$CALLS")" == 'ledger all' ]] || fail 'ledger was not checked first'
[[ "$(wc -l < "$CALLS")" -eq 11 ]] || fail 'unexpected scan or validation call count'
cmp "$repo/crates/app/src/lib.rs" "$out/implementation/crates/app/src/lib.rs" || fail 'implementation copy differs'
cmp "$repo/crates/app/tests/test file.rs" "$out/test-support/crates/app/tests/test file.rs" || fail 'test-support copy differs'
[[ ! -e "$out/implementation/crates/app/tests/test file.rs" && ! -e "$out/test-support/crates/app/src/lib.rs" ]] || fail 'partitions overlap'
[[ ! -e "$out/implementation/crates/app/src/untracked.rs" ]] || fail 'untracked file was staged'
(cd "$repo" && sha256sum -c "$out/source-sha256.txt") >/dev/null || fail 'source hashes do not match'
[[ "$(wc -l < "$out/source-sha256.txt")" -eq 2 ]] || fail 'wrong source hash count'
for partition in implementation test-support; do
  jq -e '.scanned.files == 1 and (.findings | length == 1)' "$out/$partition.json" >/dev/null || fail 'report missing findings or file count'
  jq -e '.version == "2.1.0"' "$out/$partition.sarif" >/dev/null || fail 'SARIF missing'
  grep -Fq "### $partition" "$out/summary.txt" || fail 'summary missing partition'
done
grep -Fq 'Heuristic scan evidence, not a correctness proof.' "$out/summary.txt" || fail 'summary missing evidence boundary'
cmp "$out/summary.txt" "$FIXTURE/stdout" || fail 'stdout differs from summary'
printf '  ok: complete inventory, reports and non-gating findings\n'

# Used output and paths inside the repository must be rejected before scanning.
if run_scan normal "$out"; then fail 'reusing output unexpectedly succeeded'; fi
grep -Fq 'Output directory already used' "$FIXTURE/stderr" || fail 'wrong reused-output error'
assert_last_call 'ledger all'
if run_scan normal "$repo/nested-output"; then fail 'repository output unexpectedly accepted'; fi
grep -Fq 'Output must be outside the repository' "$FIXTURE/stderr" || fail 'wrong output location error'
[[ ! -s "$CALLS" ]] || fail 'scanner ran for repository output'
if (cd "$repo" && "$repo/scripts/pons_scan.sh") > "$FIXTURE/stdout" 2> "$FIXTURE/stderr"; then fail 'missing argument unexpectedly accepted'; fi
grep -Fq 'usage: pons_scan.sh' "$FIXTURE/stderr" || fail 'missing usage error'

# Both directions of inventory mismatch are rejected before invoking pons.
cp "$repo/crates/CREUSOT-CLASSIFICATION.tsv" "$FIXTURE/classification"
sed -i '/test file.rs/d' "$repo/crates/CREUSOT-CLASSIFICATION.tsv"
expect_failure 'tracked file missing from ledger' missing_row 'ledger all'
cp "$FIXTURE/classification" "$repo/crates/CREUSOT-CLASSIFICATION.tsv"
printf 'S\tcrates/app/src/gone.rs\torphan\n' >> "$repo/crates/CREUSOT-CLASSIFICATION.tsv"
expect_failure 'ledger row without tracked file' extra_row 'ledger all'
cp "$FIXTURE/classification" "$repo/crates/CREUSOT-CLASSIFICATION.tsv"
expect_failure 'ledger failure stops the scan' ledger_fail 'ledger all'
expect_failure 'positive control must detect self-assignment' positive_missing "validate-report $FIXTURE/out-positive_missing/control-negative.json"
expect_failure 'negative control must have no findings' negative_finding "validate-report $FIXTURE/out-negative_finding/control-negative.json"
expect_failure 'scanner errors stop the scan' scan_fail "scan $FIXTURE/out-scan_fail/implementation --format json --no-speculative --require-language rust --fail-on-skips"
expect_failure 'invalid report stops the scan' validate_fail "validate-report $FIXTURE/out-validate_fail/implementation.json"
expect_failure 'SARIF failure stops the scan' sarif_fail "scan $FIXTURE/out-sarif_fail/implementation --format sarif --no-speculative --require-language rust --fail-on-skips"
for mode in wrong_count skipped coverage_failure; do
  expect_failure "incomplete scan rejected: $mode" "$mode" "validate-report $FIXTURE/out-$mode/implementation.json"
done
expect_failure 'incomplete second partition cannot claim completion' test_support_skipped "validate-report $FIXTURE/out-test_support_skipped/test-support.json"
grep -Fq '### implementation' "$FIXTURE/out-test_support_skipped/summary.txt" || fail 'first partition evidence missing from partial summary'

# An empty partition must not count as a successful scan even when the inventory
# itself matches the index. The other partition is scanned first.
git -C "$repo" rm -q --cached 'crates/app/tests/test file.rs'
sed -i '/test file.rs/d' "$repo/crates/CREUSOT-CLASSIFICATION.tsv"
expect_failure 'empty test-support partition is rejected' empty_partition "scan $FIXTURE/out-empty_partition/implementation --format sarif --no-speculative --require-language rust --fail-on-skips"
printf 'Pons scan fixtures: all checks passed\n'
