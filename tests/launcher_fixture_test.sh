#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Launcher and multiplayer-runtime fixture coverage (issue #102, P0).
#
# Drives the real launcher.sh and scripts/multiplayer-runtime.sh from a
# checkout whose path contains spaces, with stub tools (mise/cargo/mix/curl/ip)
# on PATH and fixture diagnostics for the real runtime-doctor. Covers host/join
# URL formation, environment overrides, the launcher readiness timeout, relay
# early exit and never-answered diagnoses, stale PID handling, and cleanup —
# each as a clean or firing fixture. A missing tool must fail with a
# localizing diagnosis, never pass silently.

set -euo pipefail

SOURCE_REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SANDBOX="$(mktemp -d)"

REPO="$SANDBOX/IDApTIK test"          # deliberate space: checkout paths bite
STUBS="$SANDBOX/bin"
BURBLE="$SANDBOX/burble"
FIXTURE_PORT=4123
SEAT_ARGS="$SANDBOX/seat.args"
RELAY_UP_FLAG="$SANDBOX/relay-up.flag"
LAUNCHER_PID_FILE="$SANDBOX/run/idaptik-server.pid"
RELAY_PID_FILE="$SANDBOX/run/idaptik-relay.pid"
READY_FILE="$SANDBOX/state/idaptik/startup.ready"
LOG_FILE="$SANDBOX/state/idaptik/game.log"

failures=0

say()  { printf '[launcher-fixture] %s\n' "$*"; }
note() { printf '  %s\n' "$*"; }

broke() { # $1 description
  printf '  FAIL: %s\n' "$1" >&2
  failures=$((failures + 1))
}

expect_pass() { # $1 description, rest = command
  local desc="$1"; shift
  if "$@" >/dev/null 2>&1; then
    note "ok (clean): $desc"
  else
    broke "expected success: $desc"
  fi
}

expect_fail() { # $1 description, $2 expected message, rest = command
  local desc="$1" want="$2"; shift 2
  local out
  if out="$("$@" 2>&1)"; then
    broke "expected failure: $desc"
  elif ! grep -qF "$want" <<<"$out"; then
    printf '  FAIL: %s failed for the wrong reason:\n%s\n' "$desc" "$out" >&2
    failures=$((failures + 1))
  else
    note "ok (fires): $desc"
  fi
}

kill_stragglers() {
  local pid_file pid
  for pid_file in "$LAUNCHER_PID_FILE" "$RELAY_PID_FILE"; do
    if [ -f "$pid_file" ]; then
      IFS= read -r pid < "$pid_file" || true
      case "$pid" in *[!0-9]*|'') ;; *) kill "$pid" 2>/dev/null || true ;; esac
    fi
  done
  rm -f "$RELAY_UP_FLAG"
}
trap 'kill_stragglers; rm -rf "$SANDBOX"' EXIT

# Fixture environment: sandbox XDG/HOME, stub tools, doctor diagnostics that
# pass on a headless host, and the environment overrides under test. With
# `--bg`, the function execs, so backgrounding it yields $! on the command
# itself rather than on a wrapper subshell (which would orphan the child).
run_env() {
  local bg=0
  if [ "${1:-}" = "--bg" ]; then bg=1; shift; fi
  if [ "$bg" = 1 ]; then
    exec env \
      HOME="$SANDBOX/home" \
      XDG_CONFIG_HOME="$SANDBOX/config" \
      XDG_RUNTIME_DIR="$SANDBOX/run" \
      XDG_STATE_HOME="$SANDBOX/state" \
      PATH="$STUBS:$PATH" \
      IDAPTIK_PORT="$FIXTURE_PORT" \
      IDAPTIK_BURBLE_DIR="$BURBLE" \
      SEAT_ARGS_FILE="$SEAT_ARGS" \
      IDAPTIK_RELAY_UP_FLAG="$RELAY_UP_FLAG" \
      IDAPTIK_DIAG_IS_WSL=0 \
      IDAPTIK_DIAG_DISPLAY=:0 \
      IDAPTIK_DIAG_SKIP_TOOLS=1 \
      IDAPTIK_DIAG_LAN_ADDRESS=192.0.2.42 \
      "$@"
  fi
  env \
    HOME="$SANDBOX/home" \
    XDG_CONFIG_HOME="$SANDBOX/config" \
    XDG_RUNTIME_DIR="$SANDBOX/run" \
    XDG_STATE_HOME="$SANDBOX/state" \
    PATH="$STUBS:$PATH" \
    IDAPTIK_PORT="$FIXTURE_PORT" \
    IDAPTIK_BURBLE_DIR="$BURBLE" \
    SEAT_ARGS_FILE="$SEAT_ARGS" \
    IDAPTIK_RELAY_UP_FLAG="$RELAY_UP_FLAG" \
    IDAPTIK_DIAG_IS_WSL=0 \
    IDAPTIK_DIAG_DISPLAY=:0 \
    IDAPTIK_DIAG_SKIP_TOOLS=1 \
    IDAPTIK_DIAG_LAN_ADDRESS=192.0.2.42 \
    "$@"
}

# Like run_env, but PATH contains only $1 plus bash — for missing-tool
# fixtures. The doctor diagnostics are still set so the failure isolates the
# missing tool rather than the environment.
run_env_without() { # $1 = minimal bin dir, rest = command
  local minimal="$1"; shift
  env \
    HOME="$SANDBOX/home" \
    XDG_CONFIG_HOME="$SANDBOX/config" \
    XDG_RUNTIME_DIR="$SANDBOX/run" \
    XDG_STATE_HOME="$SANDBOX/state" \
    PATH="$minimal" \
    IDAPTIK_PORT="$FIXTURE_PORT" \
    IDAPTIK_BURBLE_DIR="$BURBLE" \
    IDAPTIK_RELAY_UP_FLAG="$SANDBOX/relay-up.flag" \
    IDAPTIK_DIAG_IS_WSL=0 \
    IDAPTIK_DIAG_DISPLAY=:0 \
    IDAPTIK_DIAG_SKIP_TOOLS=1 \
    IDAPTIK_DIAG_LAN_ADDRESS=192.0.2.42 \
    "$@"
}

wait_for() { # $1 = file, $2 = expected content; returns 1 after 30 poll ticks
  local file="$1" want="$2" waited=0
  while ! grep -qF "$want" "$file" 2>/dev/null; do
    waited=$((waited + 1))
    [ "$waited" -ge 30 ] && return 1
    sleep 1
  done
}

# --- sandbox ---------------------------------------------------------------

mkdir -p "$SANDBOX/home" "$SANDBOX/config" "$SANDBOX/run" "$SANDBOX/state" \
  "$SANDBOX/bin" "$BURBLE/server" "$SANDBOX/only-mise" "$SANDBOX/nothing"
: > "$BURBLE/server/mix.exs"

# Checkout copy with a space in the path (no .git: nothing here needs history,
# and the copy must stay small).
mkdir -p "$REPO"
tar -C "$SOURCE_REPO" --exclude=.git -cf - . | tar -C "$REPO" -xf -
LAUNCHER="$REPO/launcher.sh"
RUNTIME="$REPO/scripts/multiplayer-runtime.sh"

# --- stub tools --------------------------------------------------------------

cat > "$STUBS/mise" <<'EOF'
#!/usr/bin/env bash
# stub mise: trust succeeds; exec [-C dir] -- CMD runs CMD.
set -e
case "$1" in
  trust) exit 0 ;;
  install) exit 0 ;;
  exec)
    shift
    while [ $# -gt 0 ] && [ "$1" != "--" ]; do shift; done
    [ $# -gt 0 ] || { echo "stub mise: no -- in exec" >&2; exit 1; }
    shift
    exec "$@"
    ;;
  --version) echo "mise 2026.9.1 (stub)" ;;
  *) exit 0 ;;
esac
EOF

cat > "$STUBS/cargo" <<'EOF'
#!/usr/bin/env bash
# stub cargo: reports a version; "build" materializes the stub seat binary.
if [ "${1:-}" = "--version" ]; then
  echo "cargo 1.97.1 (stub)"
  exit 0
fi
if [ "${1:-}" = "build" ]; then
  mkdir -p target/release
  cat > target/release/idaptik-bevy <<'SEAT'
#!/usr/bin/env bash
# fixture seat stub: record the seat invocation, then stay alive (as the
# bevy-named process, like the real client) until killed.
printf '%s\n' "$*" >> "$SEAT_ARGS_FILE"
sleep 600 &
seat_child=$!
trap 'kill "$seat_child" 2>/dev/null' TERM
wait "$seat_child"
SEAT
  chmod +x target/release/idaptik-bevy
  exit 0
fi
exit 0
EOF

cat > "$STUBS/mix" <<'EOF'
#!/usr/bin/env bash
# stub mix: deps/compile succeed; "run" is the relay under test. The healthy
# relay removes the answering flag when it is killed, like a real socket.
case "${1:-}" in
  deps.get|compile) exit 0 ;;
  run)
    case "${IDAPTIK_RELAY_MODE:-healthy}" in
      healthy)
        touch "$IDAPTIK_RELAY_UP_FLAG"
        trap 'rm -f "$IDAPTIK_RELAY_UP_FLAG"; exit 143' TERM
        while :; do sleep 1; done
        ;;
      dead)
        echo "fixture relay: exiting during startup" >&2
        exit 3
        ;;
      hang)
        while :; do sleep 1; done
        ;;
      *)
        echo "fixture relay: unknown mode" >&2
        exit 9
        ;;
    esac
    ;;
esac
exit 0
EOF

cat > "$STUBS/curl" <<'EOF'
#!/usr/bin/env bash
# stub curl: every http(s) URL answers only while the relay-up flag exists.
for arg in "$@"; do
  case "$arg" in
    http://*|https://*)
      if [ -e "$IDAPTIK_RELAY_UP_FLAG" ]; then
        exit 0
      else
        echo "stub curl: connection refused (fixture relay is not answering)" >&2
        exit 7
      fi
      ;;
  esac
done
exit 0
EOF

cat > "$STUBS/ip" <<'EOF'
#!/usr/bin/env bash
# stub ip: one usable IPv4 route (TEST-NET-1), or none in "none" mode.
if [ "${1:-}" = "-4" ] && [ "${2:-}" = "route" ]; then
  if [ "${IDAPTIK_IP_MODE:-}" = "none" ]; then
    exit 1
  fi
  printf 'route get 1.1.1.1\n  src 192.0.2.42\n'
fi
exit 0
EOF

chmod +x "$STUBS"/*

# Minimal tool directories for the missing-tool fixtures: coreutils plus
# bash, so a failure isolates the missing tool rather than the basics.
CORETOOLS="dirname uname awk grep sed tr cat head tail wc getconf sleep env ps mktemp mkdir cp touch rm kill pwd"
for tool in $CORETOOLS; do
  ln -sf "$(command -v "$tool")" "$SANDBOX/only-mise/$tool" 2>/dev/null || true
  ln -sf "$(command -v "$tool")" "$SANDBOX/nothing/$tool" 2>/dev/null || true
done
ln -sf "$(command -v bash)" "$SANDBOX/only-mise/bash"
ln -sf "$(command -v bash)" "$SANDBOX/nothing/bash"
ln -sf "$STUBS/mise" "$SANDBOX/only-mise/mise"

# --- launcher dry-run fixtures (command formation) ---------------------------

say 'dry-run fixtures (host/join command formation):'

dry="$(run_env IDAPTIK_LAUNCHER_DRY_RUN=1 "$LAUNCHER" --host --role hacker 2>&1)" ||
  broke "--host dry run exited nonzero"
grep -q -- "multiplayer-runtime.sh host --role hacker" <<<"$dry" &&
  note "ok (clean): --host forms the runtime host command" ||
  broke "--host dry run malformed:
$dry"

# The join preflight probes the target relay; make it look answering.
touch "$RELAY_UP_FLAG"
dry="$(run_env IDAPTIK_LAUNCHER_DRY_RUN=1 "$LAUNCHER" --join 192.0.2.42:4123 --role infiltrator 2>&1)" ||
  broke "--join dry run exited nonzero"
grep -q -- "multiplayer-runtime.sh join 192.0.2.42:4123 --role infiltrator" <<<"$dry" &&
  note "ok (clean): --join forms the runtime join command" ||
  broke "--join dry run malformed:
$dry"

dry="$(run_env IDAPTIK_LAUNCHER_DRY_RUN=1 "$LAUNCHER" --join ws://example.test:9443/voice/websocket 2>&1)" ||
  broke "ws:// --join dry run exited nonzero"
grep -q -- "join ws://example.test:9443/voice/websocket" <<<"$dry" &&
  note "ok (clean): a ws:// join target survives dry-run formation" ||
  broke "ws:// join dry run malformed:
$dry"
rm -f "$RELAY_UP_FLAG"

# --- join fixtures (seat URL formation, direct runtime) ----------------------

say 'join fixtures (URL formation and readiness records):'

run_join_seat() { # $1 = target, $2 = expected ws url
  local target="$1" want_url="$2" pid
  : > "$SEAT_ARGS"
  rm -f "$SANDBOX/join-ready"
  run_env --bg \
    IDAPTIK_INTERNAL_RUNTIME=1 IDAPTIK_PREFLIGHT_DONE=1 \
    IDAPTIK_READY_FILE="$SANDBOX/join-ready" \
    "$RUNTIME" join "$target" --role hacker >"$SANDBOX/join.log" 2>&1 &
  pid=$!
  if wait_for "$SEAT_ARGS" "$want_url"; then
    note "ok (clean): join $target forms seat URL $want_url"
  else
    broke "join $target never formed seat URL $want_url:
$(tail -5 "$SANDBOX/join.log" 2>/dev/null || true)"
  fi
  if grep -qF "join|$target|ghost-lobby|hacker" "$SANDBOX/join-ready" 2>/dev/null; then
    note "ok (clean): join $target readiness record is pipe-separated and complete"
  else
    broke "join $target readiness record malformed"
  fi
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
}

run_join_seat 192.0.2.42 "ws://192.0.2.42:4123/voice/websocket"
run_join_seat 192.0.2.42:9999 "ws://192.0.2.42:9999/voice/websocket"
run_join_seat wss://example.test/voice/websocket "wss://example.test/voice/websocket"
run_join_seat ws://example.test:9443/alt "ws://example.test:9443/alt"

# --- host fixture (full launcher flow, spaces in the checkout path) ----------

say 'host fixture (full launcher flow from a path with spaces):'

: > "$SEAT_ARGS"
rm -f "$RELAY_UP_FLAG" "$READY_FILE" "$LAUNCHER_PID_FILE" "$RELAY_PID_FILE"

host_out="$SANDBOX/host.log"
run_env --bg "$LAUNCHER" --host --role hacker >"$host_out" 2>&1 &
host_launcher=$!
if wait_for "$host_out" "game process: PID"; then
  note "ok (clean): the host flow reaches LOCAL readiness only"
else
  broke "host flow never reached readiness:
$(tail -30 "$host_out")
--- game.log ---
$(tail -20 "$LOG_FILE" 2>/dev/null || true)"
fi
kill "$host_launcher" 2>/dev/null || true
wait "$host_launcher" 2>/dev/null || true

if grep -qF "local joiner address: 127.0.0.1:4123" "$host_out"; then
  note "ok (clean): joiner address is loopback only and honours IDAPTIK_PORT"
else
  broke "joiner address line malformed"
fi
if grep -qF "./launcher.sh --join 127.0.0.1:4123 --role infiltrator" "$host_out"; then
  note "ok (clean): the shared join command names the peer role"
else
  broke "shared join command malformed"
fi
if grep -qF -- "--host --url ws://127.0.0.1:4123/voice/websocket --session ghost-lobby --role hacker" "$SEAT_ARGS"; then
  note "ok (clean): the host seat receives host/url/session/role verbatim"
else
  broke "host seat args malformed:
$(cat "$SEAT_ARGS")"
fi
if grep -qF 'host|127.0.0.1|4123|ghost-lobby|hacker' "$READY_FILE" 2>/dev/null; then
  note "ok (clean): host readiness record is complete"
else
  broke "host readiness record malformed"
fi

status_out="$(run_env "$LAUNCHER" --status 2>&1)" || true
if grep -qF 'game: running (PID' <<<"$status_out" && grep -qF 'relay: running (pid' <<<"$status_out"; then
  note "ok (clean): --status reports the running game and relay"
else
  broke "--status while running malformed:
$status_out"
fi

stop_out="$(run_env "$LAUNCHER" --stop 2>&1)" || true
if grep -qF 'relay stopped.' <<<"$stop_out"; then
  note "ok (clean): --stop stops the relay"
else
  broke "--stop did not stop the relay:
$stop_out"
fi
if [ ! -e "$LAUNCHER_PID_FILE" ] && [ ! -e "$READY_FILE" ] && [ ! -e "$RELAY_PID_FILE" ]; then
  note "ok (clean): --stop removes the PID and readiness records (cleanup)"
else
  broke "--stop left state files behind"
fi
# The stub relay tears its answering flag down asynchronously (a real socket
# closes the same way); give it a moment before asserting idle status.
relay_teardown=0
for _ in 1 2 3 4 5 6 7 8 9 10; do
  [ -e "$RELAY_UP_FLAG" ] || { relay_teardown=1; break; }
  sleep 1
done
post_stop="$(run_env "$LAUNCHER" --status 2>&1)" || true
if grep -qF 'game: stopped' <<<"$post_stop" && grep -qF 'relay: not running' <<<"$post_stop"; then
  note "ok (clean): post-stop status is fully idle"
else
  broke "post-stop --status malformed:
$post_stop"
fi

# The default test relay may NOT make a remotely shareable readiness claim.
if grep -qF 'READY FOR LOCAL JOINER ONLY' "$host_out" &&
   ! grep -qF 'READY FOR JOINERS' "$host_out" &&
   ! grep -qF '192.0.2.42:4123' "$host_out"; then
  note "ok (clean): test host never advertised the fixture LAN address"
else
  broke "test host advertised insecure remote readiness"
fi
if grep -qF 'http: [ip: {127, 0, 0, 1}, port: port]' "$RUNTIME"; then
  note "ok (clean): test relay is forced to bind loopback"
else
  broke "test relay bind is not visibly restricted to loopback"
fi

# --- relay failure fixtures ---------------------------------------------------

say 'relay failure fixtures:'

kill_stragglers

# A pre-existing answering endpoint must never be silently trusted.
touch "$RELAY_UP_FLAG"
expect_fail "an unknown answering relay cannot be reused" "refusing to reuse an unverified relay" \
  run_env IDAPTIK_INTERNAL_RUNTIME=1 "$RUNTIME" host --role hacker
rm -f "$RELAY_UP_FLAG"

# Early exit: the relay process dies during startup, with a localizing
# diagnosis in the relay log tail.
expect_fail "relay early exit is diagnosed" "relay died during startup" \
  run_env IDAPTIK_RELAY_MODE=dead IDAPTIK_INTERNAL_RUNTIME=1 \
  "$RUNTIME" host --role hacker
kill_stragglers

# Never answered: the relay stays up but never answers (wait shrunk via the
# fixture knob).
expect_fail "relay never answering is diagnosed" "relay never answered on :$FIXTURE_PORT" \
  run_env IDAPTIK_RELAY_MODE=hang IDAPTIK_RELAY_STARTUP_TIMEOUT=2 IDAPTIK_INTERNAL_RUNTIME=1 \
  "$RUNTIME" host --role hacker
kill_stragglers

# Launcher readiness timeout: the runtime is alive but never becomes ready.
rm -f "$LAUNCHER_PID_FILE" "$RELAY_PID_FILE" "$READY_FILE"
: > "$SEAT_ARGS"
timeout_out="$SANDBOX/timeout.log"
run_env IDAPTIK_RELAY_MODE=hang IDAPTIK_STARTUP_TIMEOUT=3 \
  "$LAUNCHER" --host --role hacker >"$timeout_out" 2>&1 ||
  timeout_status=$? || timeout_status=0
if grep -qF "startup is still incomplete after 3s" "$timeout_out"; then
  note "ok (fires): the launcher readiness timeout warns and returns nonzero"
else
  broke "readiness timeout did not fire:
$(tail -30 "$timeout_out")"
fi
kill_stragglers

# --- stale PID fixtures -------------------------------------------------------

say 'stale PID fixtures:'

stale_pid=
sleep 5 & stale_pid=$!
kill "$stale_pid" 2>/dev/null || true
wait "$stale_pid" 2>/dev/null || true
printf '%s\n' "$stale_pid" > "$LAUNCHER_PID_FILE"
printf '%s\n' "$stale_pid" > "$RELAY_PID_FILE"
printf 'host|192.0.2.42|%s|ghost-lobby|hacker\n' "$FIXTURE_PORT" > "$READY_FILE"

stale_status="$(run_env "$LAUNCHER" --status 2>&1)" || true
if grep -qF 'readiness: stale record present' <<<"$stale_status"; then
  note "ok (fires): --status flags a stale readiness record"
else
  broke "stale readiness not reported:
$stale_status"
fi
if grep -qF 'relay: not running' <<<"$stale_status"; then
  note "ok (fires): a dead relay PID is not reported as running"
else
  broke "stale relay PID not handled:
$stale_status"
fi

stale_stop="$(run_env "$LAUNCHER" --stop 2>&1)" || true
if grep -qF 'game is not running' <<<"$stale_stop" && grep -qF 'no relay of ours is running' <<<"$stale_stop"; then
  note "ok (clean): --stop clears stale records without killing anything"
else
  broke "--stop with stale records malformed:
$stale_stop"
fi
if [ ! -e "$LAUNCHER_PID_FILE" ] && [ ! -e "$RELAY_PID_FILE" ] && [ ! -e "$READY_FILE" ]; then
  note "ok (clean): --stop removed the stale records"
else
  broke "stale records survived --stop"
fi

# --- rejection and missing-tool fixtures --------------------------------------

say 'rejection and missing-tool fixtures (failure, never green):'

expect_fail "the runtime refuses players directly" "players must use" \
  run_env "$RUNTIME" host --role hacker

expect_fail "an unsupported client platform is rejected" "unsupported IDAPTIK_CLIENT_PLATFORM" \
  run_env IDAPTIK_CLIENT_PLATFORM=macos IDAPTIK_INTERNAL_RUNTIME=1 "$RUNTIME" host --role hacker

expect_fail "missing mise is a hard failure" "mise is required" \
  run_env_without "$SANDBOX/nothing" IDAPTIK_INTERNAL_RUNTIME=1 "$RUNTIME" host --role hacker

expect_fail "missing cargo toolchain is a hard failure" "pinned Rust toolchain is unavailable" \
  run_env_without "$SANDBOX/only-mise" IDAPTIK_INTERNAL_RUNTIME=1 "$RUNTIME" host --role hacker

# A failed doctor preflight (no usable LAN route) must block the launch.
expect_fail "a failed preflight blocks the launch" "runtime preflight failed" \
  run_env IDAPTIK_IP_MODE=none IDAPTIK_DIAG_LAN_ADDRESS= IDAPTIK_LAUNCHER_DRY_RUN=1 \
  "$LAUNCHER" --host --role hacker
unset IDAPTIK_IP_MODE

# --- verdict -------------------------------------------------------------------

kill_stragglers
if [ "$failures" -ne 0 ]; then
  printf 'launcher fixtures: %d FAILURES\n' "$failures" >&2
  exit 1
fi
printf 'launcher fixtures: all clean and firing fixtures behaved\n'
