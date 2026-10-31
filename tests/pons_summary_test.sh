#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Exercise the actual workflow summary step with and without scan evidence.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FIXTURE="$(mktemp -d)"
trap 'rm -rf "$FIXTURE"' EXIT

# Fail closed if the named step or its shell block cannot be located.
awk '
  /- name: Publish readable evidence summary/ { step = 1; next }
  step && /^        if: always\(\)$/ { always = 1; next }
  step && /^        run: \|$/ { block = 1; next }
  block && /^      - / { exit }
  block { sub(/^          /, ""); print; n++ }
  END { if (!always || !n) exit 1 }
' "$REPO_DIR/.github/workflows/pons-scan.yml" > "$FIXTURE/step.sh"

export RUNNER_TEMP="$FIXTURE/runner"
export GITHUB_STEP_SUMMARY="$FIXTURE/github-summary"
mkdir -p "$RUNNER_TEMP/pons-evidence"
printf 'prior summary\n' > "$GITHUB_STEP_SUMMARY"
bash "$FIXTURE/step.sh"
grep -Fq 'prior summary' "$GITHUB_STEP_SUMMARY"
grep -Fq 'Scan did not complete. Inspect failed steps; no clean result is claimed.' "$GITHUB_STEP_SUMMARY"
grep -Fq 'Heuristic evidence, not a correctness proof. Findings require review.' "$GITHUB_STEP_SUMMARY"

printf 'fixture scan report\n' > "$RUNNER_TEMP/pons-evidence/summary.txt"
: > "$GITHUB_STEP_SUMMARY"
bash "$FIXTURE/step.sh"
grep -Fxq 'fixture scan report' "$GITHUB_STEP_SUMMARY"
if grep -Fq 'Scan did not complete' "$GITHUB_STEP_SUMMARY"; then
  printf 'FAIL: completed evidence was reported as missing\n' >&2
  exit 1
fi
printf 'Pons workflow summary fixtures: complete and missing evidence passed\n'
