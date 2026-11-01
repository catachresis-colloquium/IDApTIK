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
# stub cargo: reports a version; "build" materializes the stub client at the
# path the caller asked for (debug for the launcher, release for the runtime,
# and a target triple directory for the Windows-native client).
if [ "${1:-}" = "--version" ]; then
  echo "cargo 1.97.1 (stub)"
  exit 0
fi
if [ "${1:-}" = "build" ]; then
  shift
  triple=""
  profile="debug"
  while [ $# -gt 0 ]; do
    case "$1" in
      --target) triple="$2"; shift 2 ;;
      --release) profile="release"; shift ;;
      *) shift ;;
    esac
  done
  out="target/$profile"
  [ -z "$triple" ] || out="target/$triple/$profile"
  bin="$out/idaptik-bevy"
  [ -z "$triple" ] || bin="$bin.exe"
  mkdir -p "$out"
  cat > "$bin" <<'SEAT'
#!/usr/bin/env bash
# fixture client stub: record the invocation, print the adapter line Bevy
# prints when it selects one, then stay alive until killed.
printf '%s %s\n' "$0" "$*" >> "${SEAT_ARGS_FILE:-/dev/null}"
printf '%s\n' "${IDAPTIK_FIXTURE_ADAPTER:-AdapterInfo { name: \"Fixture GPU (stub)\", vendor: 0, device: 0, device_type: DiscreteGpu, driver: \"stub\", driver_info: \"stub\", backend: Vulkan }}"
# Announce this client the way a real Windows process appears to tasklist.
# Interop detaches a Windows GUI process, so the multiplayer runtime expects
# this command to return while the client keeps running; emulate that.
if [ -n "${IDAPTIK_FIXTURE_WINDOWS_CLIENT_FLAG:-}" ]; then
  touch "$IDAPTIK_FIXTURE_WINDOWS_CLIENT_FLAG"
  if [ "${IDAPTIK_FIXTURE_WINDOWS_DETACH:-}" = "1" ]; then exit 0; fi
fi
sleep 600 &
seat_child=$!
trap 'kill "$seat_child" 2>/dev/null' TERM
wait "$seat_child"
SEAT
  chmod +x "$bin"
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

cat > "$STUBS/wslpath" <<'EOF'
#!/usr/bin/env bash
# stub wslpath: enough path translation for the Windows seat fixture.
case "${1:-}" in
  -w) shift; printf 'C:%s\n' "$1" ;;
  -u) shift; printf '%s\n' "$1" ;;
  *) printf '%s\n' "$*" ;;
esac
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
  note "ok (clean): the host flow reaches READY FOR JOINERS"
else
  broke "host flow never reached readiness:
$(tail -30 "$host_out")
--- game.log ---
$(tail -20 "$LOG_FILE" 2>/dev/null || true)"
fi
kill "$host_launcher" 2>/dev/null || true
wait "$host_launcher" 2>/dev/null || true

if grep -qF "joiner address: 192.0.2.42:4123" "$host_out"; then
  note "ok (clean): joiner address honours the LAN route and IDAPTIK_PORT"
else
  broke "joiner address line malformed"
fi
if grep -qF "./launcher.sh --join 192.0.2.42:4123 --role infiltrator" "$host_out"; then
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
if grep -qF 'host|192.0.2.42|4123|ghost-lobby|hacker' "$READY_FILE" 2>/dev/null; then
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

# --- relay failure fixtures ---------------------------------------------------

say 'relay failure fixtures:'

kill_stragglers

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

# --- solo client fixtures (renderer evidence) --------------------------------

say 'solo client fixtures (renderer evidence from the client log):'

rm -f "$READY_FILE" "$LAUNCHER_PID_FILE" "$RELAY_PID_FILE"
solo_out="$SANDBOX/solo.log"
run_env --bg "$LAUNCHER" --solo >"$solo_out" 2>&1 &
solo_launcher=$!
if wait_for "$solo_out" "game process: PID"; then
  note "ok (clean): the solo flow reaches READY"
else
  broke "solo flow never reached readiness:
$(tail -40 "$solo_out")
--- game.log ---
$(tail -20 "$LOG_FILE" 2>/dev/null || true)"
fi
if grep -qF 'renderer: Fixture GPU (stub) (Vulkan)' "$solo_out"; then
  note "ok (clean): the launcher reports the adapter Bevy selected"
else
  broke "renderer evidence missing:
$(cat "$solo_out" 2>/dev/null || true)"
fi
if grep -qF '● READY — OPENING SOLO GUI' "$solo_out"; then
  note "ok (clean): the solo readiness banner is shown"
else
  broke "solo readiness banner missing"
fi
kill "$solo_launcher" 2>/dev/null || true
wait "$solo_launcher" 2>/dev/null || true
run_env "$LAUNCHER" --stop >/dev/null 2>&1 || true

# A software adapter must be named as such, not reported like any other GPU.
rm -f "$READY_FILE"
software_out="$SANDBOX/solo-software.log"
run_env --bg \
  IDAPTIK_FIXTURE_ADAPTER='AdapterInfo { name: "llvmpipe (LLVM 19.1.7, 256 bits)", vendor: 0, device: 0, device_type: Cpu, driver: "llvmpipe", driver_info: "Mesa", backend: Vulkan }' \
  "$LAUNCHER" --solo >"$software_out" 2>&1 &
software_launcher=$!
if wait_for "$software_out" "game process: PID"; then
  note "ok (clean): a software-adapter solo run still reaches READY"
else
  broke "software-adapter solo run never reached readiness:
$(tail -40 "$software_out")"
fi
grep -qF 'renderer: llvmpipe (LLVM 19.1.7, 256 bits) (Vulkan)' "$software_out" &&
  note "ok (clean): the software adapter is named" ||
  broke "software adapter not named:
$(cat "$software_out" 2>/dev/null || true)"
grep -q 'SOFTWARE rendering' "$software_out" &&
  note "ok (fires): software rendering is warned about, not silently accepted" ||
  broke "software rendering was not warned about"
kill "$software_launcher" 2>/dev/null || true
wait "$software_launcher" 2>/dev/null || true
run_env "$LAUNCHER" --stop >/dev/null 2>&1 || true

status_after="$(run_env "$LAUNCHER" --status 2>&1)" || true
if grep -qF 'renderer: llvmpipe (LLVM 19.1.7, 256 bits) (Vulkan)' <<<"$status_after" &&
  grep -q 'SOFTWARE rendering' <<<"$status_after"; then
  note "ok (clean): --status reports the last run's adapter with no game running"
else
  broke "--status lost the last renderer:
$status_after"
fi

# --- WSL2 native-Windows-client fixtures -------------------------------------

say 'Windows-client fixtures (interop discovered without cmd.exe on PATH):'

WINSYS="$SANDBOX/winsys32"
mkdir -p "$WINSYS"
cat > "$WINSYS/cmd.exe" <<'CMD'
#!/usr/bin/env bash
# fixture cmd.exe: interop probes succeed. tasklist reports one Windows client
# (PID 4242) only while the stub client has marked itself running, so the
# runtime's "already running" guard and its tracking loop both behave.
flag="${IDAPTIK_FIXTURE_WINDOWS_CLIENT_FLAG:-}"
for arg in "$@"; do
  case "$arg" in
    tasklist)
      if [ -n "$flag" ] && [ -e "$flag" ]; then
        printf '"idaptik-bevy.exe","4242"\r\n'
      else
        printf 'INFO: No tasks are running which match the specified criteria.\r\n'
      fi
      exit 0
      ;;
    taskkill) [ -z "$flag" ] || rm -f "$flag"; exit 0 ;;
  esac
done
exit 0
CMD
chmod +x "$WINSYS/cmd.exe"
mkdir -p "$SANDBOX/wslg" "$SANDBOX/shared_memory"
printf '%s\n' 'rdp_allocate_shared_memory: Failed to open "/mnt/shared_memory/id" with error: Input/output error' \
  > "$SANDBOX/wslg/weston.log"

# Explicit request: the flag alone must select the Windows client.
dry="$(run_env \
  IDAPTIK_DIAG_IS_WSL=1 IDAPTIK_WINDOWS_SYSTEM32="$WINSYS" \
  IDAPTIK_LAUNCHER_DRY_RUN=1 "$LAUNCHER" --solo --windows-client 2>&1)" ||
  broke "--solo --windows-client dry run exited nonzero:
$dry"
grep -q 'IDAPTIK_CLIENT_PLATFORM=windows' <<<"$dry" &&
  note "ok (clean): --windows-client selects the Windows client explicitly" ||
  broke "--windows-client did not select the Windows client:
$dry"
grep -q -- 'launcher.sh __run-solo' <<<"$dry" &&
  note "ok (clean): the Windows request still launches the solo client" ||
  broke "--windows-client broke solo command formation:
$dry"

# The modifier can precede the mode or occur between its arguments. The
# alias must preserve the requested host/join command and its role.
dry="$(run_env IDAPTIK_DIAG_IS_WSL=1 IDAPTIK_WINDOWS_SYSTEM32="$WINSYS" \
  IDAPTIK_LAUNCHER_DRY_RUN=1 "$LAUNCHER" --wsl-windows --host --role hacker 2>&1)" ||
  broke "Windows host dry run exited nonzero:
$dry"
if grep -qF 'DRY RUN: IDAPTIK_CLIENT_PLATFORM=windows' <<<"$dry" &&
   grep -qF 'multiplayer-runtime.sh host --role hacker' <<<"$dry"; then
  note "ok (clean): the Windows alias composes with host and its role"
else
  broke "Windows host command malformed:
$dry"
fi
touch "$RELAY_UP_FLAG"
dry="$(run_env IDAPTIK_DIAG_IS_WSL=1 IDAPTIK_WINDOWS_SYSTEM32="$WINSYS" \
  IDAPTIK_LAUNCHER_DRY_RUN=1 "$LAUNCHER" --join 192.0.2.42:4123 --windows-client --role infiltrator 2>&1)" ||
  broke "Windows join dry run exited nonzero:
$dry"
if grep -qF 'DRY RUN: IDAPTIK_CLIENT_PLATFORM=windows' <<<"$dry" &&
   grep -qF 'multiplayer-runtime.sh join 192.0.2.42:4123 --role infiltrator' <<<"$dry"; then
  note "ok (clean): the Windows modifier composes with join and its role"
else
  broke "Windows join command malformed:
$dry"
fi
rm -f "$RELAY_UP_FLAG"

# Automatic path: broken WSLg Copy Mode plus discovered interop, with no
# cmd.exe anywhere on PATH.
dry="$(run_env \
  IDAPTIK_DIAG_IS_WSL=1 IDAPTIK_WINDOWS_SYSTEM32="$WINSYS" \
  IDAPTIK_DIAG_WSLG_DIR="$SANDBOX/wslg" \
  IDAPTIK_DIAG_SHARED_MEMORY="$SANDBOX/shared_memory" \
  IDAPTIK_DIAG_WESTON_LOG="$SANDBOX/wslg/weston.log" \
  IDAPTIK_LAUNCHER_DRY_RUN=1 "$LAUNCHER" --solo 2>&1)" ||
  broke "WSLg fallback dry run exited nonzero:
$dry"
grep -q 'WSLg Copy Mode is broken; using the native Windows Bevy client instead' <<<"$dry" &&
  note "ok (clean): broken WSLg selects the Windows client without cmd.exe on PATH" ||
  broke "the WSLg fallback no longer fires:
$dry"
grep -q 'IDAPTIK_CLIENT_PLATFORM=windows' <<<"$dry" &&
  note "ok (clean): the automatic fallback resolves to the Windows client" ||
  broke "the fallback did not select the Windows client"

# End to end: the requested client is built at the target-triple path and run.
rm -f "$READY_FILE" "$LAUNCHER_PID_FILE"
: > "$SEAT_ARGS"
windows_out="$SANDBOX/solo-windows.log"
run_env --bg IDAPTIK_DIAG_IS_WSL=1 IDAPTIK_WINDOWS_SYSTEM32="$WINSYS" \
  "$LAUNCHER" --solo --windows-client >"$windows_out" 2>&1 &
windows_launcher=$!
if wait_for "$windows_out" "game process: PID"; then
  note "ok (clean): the Windows-native solo client builds and starts"
else
  broke "Windows-native solo flow never reached readiness:
$(tail -40 "$windows_out")
--- game.log ---
$(tail -20 "$LOG_FILE" 2>/dev/null || true)"
fi
if grep -q 'x86_64-pc-windows-gnu' "$SEAT_ARGS"; then
  note "ok (clean): the target-triple binary is the one executed"
else
  broke "the Windows client was not the executed binary:
$(cat "$SEAT_ARGS" 2>/dev/null || true)"
fi
grep -qF 'renderer: Fixture GPU (stub) (Vulkan)' "$windows_out" &&
  note "ok (clean): the Windows client's adapter is reported too" ||
  broke "no renderer evidence for the Windows client:
$(cat "$windows_out" 2>/dev/null || true)"
kill "$windows_launcher" 2>/dev/null || true
wait "$windows_launcher" 2>/dev/null || true
run_env "$LAUNCHER" --stop >/dev/null 2>&1 || true

# Multiplayer Windows seat: the runtime tracks the client through tasklist, so
# a cmd.exe that is not on PATH must still be found and used.
rm -f "$SANDBOX/win-seat.log"
: > "$SEAT_ARGS"
run_env --bg \
  IDAPTIK_CLIENT_PLATFORM=windows IDAPTIK_INTERNAL_RUNTIME=1 IDAPTIK_PREFLIGHT_DONE=1 \
  IDAPTIK_READY_FILE="$SANDBOX/win-ready" \
  IDAPTIK_FIXTURE_WINDOWS_CLIENT_FLAG="$SANDBOX/windows-client.flag" \
  IDAPTIK_FIXTURE_WINDOWS_DETACH=1 \
  IDAPTIK_DIAG_IS_WSL=1 IDAPTIK_WINDOWS_SYSTEM32="$WINSYS" \
  "$RUNTIME" join 192.0.2.42 --role hacker >"$SANDBOX/win-seat.log" 2>&1 &
win_seat=$!
if wait_for "$SANDBOX/win-seat.log" "Windows client: PID 4242"; then
  note "ok (clean): the Windows multiplayer seat is tracked without cmd.exe on PATH"
else
  broke "the Windows seat never reported a tracked PID:
$(tail -30 "$SANDBOX/win-seat.log" 2>/dev/null || true)"
fi
if grep -q 'x86_64-pc-windows-gnu/release/idaptik-bevy.exe' "$SEAT_ARGS"; then
  note "ok (clean): the Windows seat runs the target-triple release binary"
else
  broke "the Windows seat did not run the expected binary:
$(cat "$SEAT_ARGS" 2>/dev/null || true)"
fi
kill "$win_seat" 2>/dev/null || true
wait "$win_seat" 2>/dev/null || true

expect_fail "--windows-client outside WSL2 is refused" "runs only under WSL2" \
  run_env IDAPTIK_DIAG_IS_WSL=0 IDAPTIK_LAUNCHER_DRY_RUN=1 "$LAUNCHER" --solo --windows-client

mkdir -p "$SANDBOX/winsys-empty"
expect_fail "a machine without interop gets the reason, not a broken build" \
  "no cmd.exe under /mnt/c/Windows/System32" \
  run_env IDAPTIK_DIAG_IS_WSL=1 IDAPTIK_WINDOWS_SYSTEM32="$SANDBOX/winsys-empty" \
  IDAPTIK_LAUNCHER_DRY_RUN=1 "$LAUNCHER" --solo --windows-client

# Interop disabled in /etc/wsl.conf leaves cmd.exe readable but unrunnable;
# existence alone must not count as working.
mkdir -p "$SANDBOX/winsys-broken"
printf '#!/usr/bin/env bash\nexit 1\n' > "$SANDBOX/winsys-broken/cmd.exe"
chmod +x "$SANDBOX/winsys-broken/cmd.exe"
expect_fail "an unrunnable interop surface is not counted as interop" \
  "cmd.exe exists but did not run" \
  run_env IDAPTIK_DIAG_IS_WSL=1 IDAPTIK_WINDOWS_SYSTEM32="$SANDBOX/winsys-broken" \
  IDAPTIK_LAUNCHER_DRY_RUN=1 "$LAUNCHER" --solo --windows-client

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
