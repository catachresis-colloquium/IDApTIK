#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# The Evil Weevil offline gate (ADR-0017): deterministic adversarial testing
# of the multiplayer acceptance rules and recovery behaviour, with no sockets
# and no remote systems. PASS requires:
#
#   1. every fixed corpus scenario passes against the real production
#      validators (idaptik-net lockstep/envelope + idaptik-core sim);
#   2. the host's invariants hold when an agent proposes prohibited actions
#      (the regression suite);
#   3. the defensive observer reports zero false positives on fault traces
#      and zero false negatives on violation traces;
#   4. every run stays inside the strict bounds in limits.rs (steps, queue
#      depth, wire messages, intent budget);
#   5. the corpus binary exits non-zero on any failure and writes a redacted
#      JSON report plus per-scenario replay artifacts.
#
# A missing Rust toolchain FAILS the gate (estate doctrine: a gate that
# skips is a gate that lies). The real-burble confirmation lives separately
# in scripts/evil_weevil_relay_check.sh and needs Elixir.
set -euo pipefail
cd "$(dirname "$0")/.."

command -v cargo >/dev/null 2>&1 || {
    echo "FAIL: cargo (Rust) is required — run 'just setup'" >&2
    exit 1
}

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "== unit + integration tests (policy, observer, redaction, regressions)"
cargo test -p idaptik-evil-weevil

echo "== clippy (warnings are errors)"
cargo clippy -p idaptik-evil-weevil --all-targets -- -D warnings

echo "== run the bounded adversarial corpus"
cargo run -q -p idaptik-evil-weevil -- --out-dir "$WORK/artifacts" --seed 777001

test -f "$WORK/artifacts/corpus-report.json"
for id in \
    EW-01-stale-duplicate-messages \
    EW-02-impossible-movement-invalid-targets \
    EW-03-conflicting-epochs \
    EW-04-reconnect-obsolete-messages \
    EW-05-delayed-withheld-observations; do
    test -f "$WORK/artifacts/$id/result.json" \
        || { echo "FAIL: missing replay artifact for $id" >&2; exit 1; }
done

# Minimal-evidence gate: no secret-shaped material in the written artifacts.
if grep -qiE 'bearer [a-z0-9._-]+|ghp_[a-z0-9]+|"password":|"api_key":' "$WORK/artifacts/corpus-report.json"; then
    echo "FAIL: the redacted corpus report contains secret-shaped material" >&2
    exit 1
fi

# Determinism gate: two runs produce byte-identical reports.
cargo run -q -p idaptik-evil-weevil -- --out-dir "$WORK/second" --seed 777001 >/dev/null
cmp -s "$WORK/artifacts/corpus-report.json" "$WORK/second/corpus-report.json" \
    || { echo "FAIL: corpus runs are not deterministic" >&2; exit 1; }

echo "PASS: Evil Weevil offline gate — artifacts in $WORK/artifacts"
