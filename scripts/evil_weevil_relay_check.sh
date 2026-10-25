#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# The Evil Weevil real-relay leg (ADR-0017): confirms the in-process relay
# model against a throwaway, local, isolated burble GameChannel. It uses
# the fixed burble test port (4002 — hard-coded in burble's config/test.exs,
# same convention as scripts/loopback_check.sh; hosted runners are isolated
# so the fixed port cannot collide across jobs, and the two gates are not
# run concurrently), guest dummy identities, a bounded handful of frames,
# and no remote address but loopback. There is no availability testing,
# scanning or traffic against any unrelated system.
#
# Requirements are hard: a missing mix/burble/cargo FAILS the gate rather
# than skipping. The offline corpus (scripts/evil_weevil_check.sh) needs no
# Elixir and is the primary gate; this one is the cross-repository check.
set -euo pipefail
cd "$(dirname "$0")/.."

PORT=4002
export IDAPTIK_RELAY_URL="ws://127.0.0.1:${PORT}/voice/websocket"
BURBLE_DIR="${IDAPTIK_BURBLE_DIR:-../burble}"

command -v mix >/dev/null 2>&1 || { echo "FAIL: mix (Elixir) is required — run 'just setup'" >&2; exit 1; }
command -v cargo >/dev/null 2>&1 || { echo "FAIL: cargo (Rust) is required" >&2; exit 1; }
[ -f "$BURBLE_DIR/server/mix.exs" ] || {
    echo "FAIL: burble not found at $BURBLE_DIR — clone it first (see scripts/loopback_check.sh)" >&2
    exit 1
}

echo "== build the socket test crate"
cargo build -q -p idaptik-net

echo "== throwaway burble relay on isolated port ${PORT}"
(cd "$BURBLE_DIR/server" && env MIX_ENV=test mix deps.get && env MIX_ENV=test mix compile)
(cd "$BURBLE_DIR/server" && MIX_ENV=test PHX_SERVER=true exec mix phx.server \
    >/tmp/idaptik_evil_weevil_relay.log 2>&1) &
RELAY_PID=$!
cleanup() {
    kill "$RELAY_PID" 2>/dev/null || true
    pkill -P "$RELAY_PID" 2>/dev/null || true
    wait "$RELAY_PID" 2>/dev/null || true
}
trap cleanup EXIT

for _ in $(seq 1 60); do
    if curl -fsS "http://127.0.0.1:${PORT}/" >/dev/null 2>&1; then
        break
    fi
    if ! kill -0 "$RELAY_PID" 2>/dev/null; then
        echo "FAIL: relay died during startup — /tmp/idaptik_evil_weevil_relay.log:" >&2
        tail -20 /tmp/idaptik_evil_weevil_relay.log >&2
        exit 1
    fi
    sleep 1
done
curl -fsS "http://127.0.0.1:${PORT}/" >/dev/null || {
    echo "FAIL: relay never answered on :${PORT}" >&2
    exit 1
}

echo "== Evil Weevil relay leg (ignored socket tests, loopback only)"
cargo test -p idaptik-net --test evil_weevil_relay -- --ignored --nocapture

echo "PASS: Evil Weevil real-relay leg — the model matches the pinned burble GameChannel"
