#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Exercise the real offline gate's replay-artifact requirement without Cargo.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FIXTURE="$(mktemp -d)"
trap 'rm -rf "$FIXTURE"' EXIT
mkdir -p "$FIXTURE/bin"

# The gate still runs normally; only Cargo is replaced so each case can
# control which replay files the corpus runner emits.
cat > "$FIXTURE/bin/cargo" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$1" >> "$EW_CALLS"
if [[ "$1" != run ]]; then
    exit 0
fi
while [[ $# -gt 0 && "$1" != --out-dir ]]; do shift; done
[[ $# -ge 2 ]] || exit 2
out="$2"
mkdir -p "$out"
printf '{}\n' > "$out/corpus-report.json"
for id in \
    EW-01-stale-duplicate-messages \
    EW-02-impossible-movement-invalid-targets \
    EW-03-conflicting-epochs \
    EW-04-reconnect-obsolete-messages \
    EW-05-delayed-withheld-observations \
    EW-06-infrastructure-orders-and-observations; do
    [[ "$id" == "$EW_OMIT" ]] && continue
    mkdir -p "$out/$id"
    printf '{}\n' > "$out/$id/result.json"
done
if [[ "$EW_WRONG_PATH" == yes ]]; then
    mkdir -p "$out/EW-06-infrastructure-orders-and-observations"
    printf '{}\n' > "$out/EW-06-infrastructure-orders-and-observations/other.json"
fi
STUB
chmod +x "$FIXTURE/bin/cargo"
export PATH="$FIXTURE/bin:$PATH"
export EW_CALLS="$FIXTURE/calls"
export EW_OMIT='' EW_WRONG_PATH=no

run_gate() {
    : > "$EW_CALLS"
    bash "$REPO_DIR/scripts/evil_weevil_check.sh" > "$FIXTURE/output" 2>&1
}

# A complete corpus makes it through the artifact check and both runs.
run_gate
grep -Fq 'PASS: Evil Weevil offline gate' "$FIXTURE/output"
[[ "$(grep -c '^run$' "$EW_CALLS")" -eq 2 ]]

# EW-01 through EW-05 alone must not pass.
export EW_OMIT=EW-06-infrastructure-orders-and-observations
if run_gate; then
    echo 'FAIL: missing EW-06 result.json passed the gate' >&2
    exit 1
fi
grep -Fq "FAIL: missing replay artifact for $EW_OMIT" "$FIXTURE/output"
[[ "$(grep -c '^run$' "$EW_CALLS")" -eq 1 ]]

# Neither the scenario directory nor a differently named JSON file is a
# substitute for its replay.
export EW_WRONG_PATH=yes
if run_gate; then
    echo 'FAIL: unrelated EW-06 JSON passed the gate' >&2
    exit 1
fi
grep -Fq "FAIL: missing replay artifact for $EW_OMIT" "$FIXTURE/output"
[[ "$(grep -c '^run$' "$EW_CALLS")" -eq 1 ]]

# An earlier missing artifact is still rejected even when EW-06 is present.
export EW_OMIT=EW-05-delayed-withheld-observations EW_WRONG_PATH=no
if run_gate; then
    echo 'FAIL: missing EW-05 result.json passed the gate' >&2
    exit 1
fi
grep -Fq "FAIL: missing replay artifact for $EW_OMIT" "$FIXTURE/output"
[[ "$(grep -c '^run$' "$EW_CALLS")" -eq 1 ]]

printf 'Evil Weevil artifact gate fixtures passed\n'
