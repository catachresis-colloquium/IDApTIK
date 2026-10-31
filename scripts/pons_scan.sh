#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Run pinned pons against an exhaustive, staged first-party Rust inventory.
set -euo pipefail
root="$(git rev-parse --show-toplevel)"
cd "$root"
out="${1:?usage: pons_scan.sh OUTPUT_DIRECTORY (outside the repository)}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
case "$out/" in "$root/"*) echo 'Output must be outside the repository' >&2; exit 2;; esac
command -v pons >/dev/null
command -v jq >/dev/null
./scripts/creusot_ledger_check.sh all
# Do not accidentally reuse old staged files or reports.
[[ ! -e "$out/inventory.tsv" ]] || { echo 'Output directory already used' >&2; exit 2; }
awk -F '\t' '!/^#/ && NF >= 2 {print $1 "\t" $2}' \
  crates/CREUSOT-CLASSIFICATION.tsv > "$out/inventory.tsv"
git ls-files crates | grep '\.rs$' | LC_ALL=C sort > "$out/tracked.txt"
cut -f2 "$out/inventory.tsv" | LC_ALL=C sort > "$out/classified.txt"
cmp "$out/tracked.txt" "$out/classified.txt"
[[ -s "$out/tracked.txt" ]]
mkdir -p "$out/controls" "$out/implementation" "$out/test-support"
printf 'fn f() { x = x; }\n' > "$out/controls/positive.rs"
printf 'fn f() { x = y; }\n' > "$out/controls/negative.rs"
for control in positive negative; do
  pons scan "$out/controls/$control.rs" --rule self-assignment --format json \
    --require-language rust --fail-on-skips > "$out/control-$control.json"
  pons validate-report "$out/control-$control.json"
done
jq -e '.findings | any(.rule_id == "self-assignment")' "$out/control-positive.json" >/dev/null
jq -e '.findings | length == 0' "$out/control-negative.json" >/dev/null
while IFS=$'\t' read -r tier path; do
  partition=implementation
  [[ "$tier" != T ]] || partition=test-support
  mkdir -p "$out/$partition/$(dirname "$path")"
  cp -- "$path" "$out/$partition/$path"
  sha256sum "$path"
done < "$out/inventory.tsv" > "$out/source-sha256.txt"
for partition in implementation test-support; do
  expected="$(find "$out/$partition" -type f -name '*.rs' | wc -l)"
  [[ "$expected" -gt 0 ]]
  # Initial scan is evidence gathering, not a severity gate. Operational and
  # coverage failures still fail. Findings must be reviewed, never auto-fixed.
  pons scan "$out/$partition" --format json --no-speculative \
    --require-language rust --fail-on-skips > "$out/$partition.json"
  pons validate-report "$out/$partition.json"
  jq -e --argjson n "$expected" \
    '.scanned.files == $n and (.scanned.skipped | length == 0) and
     (.scanned.coverage_failures | length == 0)' "$out/$partition.json" >/dev/null
  pons scan "$out/$partition" --format sarif --no-speculative \
    --require-language rust --fail-on-skips > "$out/$partition.sarif"
  {
    printf '\n### %s\n\n' "$partition"
    jq '{schema_version, tool, execution, scanned: {files: .scanned.files,
      coverage_status: .scanned.coverage_status, skipped: .scanned.skipped}, counts}' \
      "$out/$partition.json"
  } >> "$out/summary.txt"
done
printf '\nHeuristic scan evidence, not a correctness proof. Inline tests remain in their source files.\n' >> "$out/summary.txt"
cat "$out/summary.txt"
