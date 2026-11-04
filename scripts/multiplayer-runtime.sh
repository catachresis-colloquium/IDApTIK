#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
#
# Internal multiplayer runtime for launcher.sh. This is deliberately not a
# launcher and has no lifecycle/UI aliases: players use ./launcher.sh only.
# It owns the Phoenix relay process, release build, host/join seat execution,
# readiness events, and the local-only boundary behind the canonical launcher.
#
# Environment: IDAPTIK_PORT (relay port, default 4000), IDAPTIK_BURBLE_DIR
# (Burble checkout, default sibling ../burble), IDAPTIK_CLIENT_PLATFORM
# (linux|windows), IDAPTIK_RELAY_STARTUP_TIMEOUT (relay readiness wait in
# seconds, default 60; primarily a fixture knob).

set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [ -r "$REPO_DIR/scripts/local-session.sh" ]; then
  # shellcheck source=scripts/local-session.sh
  . "$REPO_DIR/scripts/local-session.sh"
else
  printf 'error: shared local-session boundary missing\n' >&2
  exit 1
fi
BURBLE_DIR="${IDAPTIK_BURBLE_DIR:-$REPO_DIR/../burble}"
RUNTIME_DOCTOR="$REPO_DIR/scripts/runtime-doctor.sh"
RUN_CONFIG="$REPO_DIR/fixtures/session_relay/versus_script.json"
CLIENT_PLATFORM="${IDAPTIK_CLIENT_PLATFORM:-linux}"
case "$CLIENT_PLATFORM" in
  linux) BEVY_BIN="$REPO_DIR/target/release/idaptik-bevy" ;;
  windows) BEVY_BIN="$REPO_DIR/target/x86_64-pc-windows-gnu/release/idaptik-bevy.exe" ;;
  *) printf 'error: unsupported IDAPTIK_CLIENT_PLATFORM %q\n' "$CLIENT_PLATFORM" >&2; exit 1 ;;
esac

# Windows interop discovery is shared with the launcher, doctor and installer.
# The Windows seat is tracked through tasklist/taskkill, so this runtime needs a
# runnable cmd.exe even when C:\Windows\System32 is absent from PATH.
if [ -r "$REPO_DIR/scripts/windows-interop.sh" ]; then
  # shellcheck source=scripts/windows-interop.sh
  . "$REPO_DIR/scripts/windows-interop.sh"
fi
WINDOWS_CMD="cmd.exe"
if [ "$CLIENT_PLATFORM" = "windows" ] && declare -F hp_windows_tool >/dev/null 2>&1; then
  WINDOWS_CMD="$(hp_windows_tool cmd.exe 2>/dev/null || printf 'cmd.exe')"
fi

# cmd.exe dislikes a UNC working directory. /mnt/c/Windows is the usual answer,
# but a custom mount root or a stripped container may not have it; run from /
# rather than failing the whole seat over a diagnostics working directory.
windows_cwd() {
  if [ -d /mnt/c/Windows ]; then printf '/mnt/c/Windows'; else printf '/'; fi
}

PORT="$(idaptik_local_port "${IDAPTIK_PORT:-4000}")" || {
  printf 'error: IDAPTIK_PORT must be between 1 and 65535\n' >&2
  exit 1
}
SESSION_DEFAULT="ghost-lobby"

RUNTIME_DIR="$(idaptik_runtime_root)"
PID_FILE="$RUNTIME_DIR/relay.pid"
LOG_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/idaptik-multiplayer"
LOG_FILE="$LOG_DIR/relay.log"

# ----------------------------------------------------------------------------
# helpers
# ----------------------------------------------------------------------------

say()  { printf '%s\n' "$*"; }
die()  {
  if [ -n "${IDAPTIK_PROGRESS_FILE:-}" ]; then
    printf 'FAIL|%s\n' "$*" >> "$IDAPTIK_PROGRESS_FILE"
  fi
  printf 'error: %s\n' "$*" >&2
  exit 1
}
progress() {
  [ -n "${IDAPTIK_PROGRESS_FILE:-}" ] || return 0
  printf '%s|%s\n' "$1" "$2" >> "$IDAPTIK_PROGRESS_FILE"
}
mark_ready() {
  [ -n "${IDAPTIK_READY_FILE:-}" ] || die "internal readiness channel is missing"
  printf '%s\n' "$1" > "$IDAPTIK_READY_FILE"
}

need() {
  command -v "$1" >/dev/null 2>&1 || die "$1 is required $2 — $3"
}

burble_mix() {
  # Resolve the pinned BEAM from IDApTIK's trusted mise config while executing
  # Mix inside Burble's project directory.
  # $1/$@ are intentionally expanded by the child bash.
  # shellcheck disable=SC2016
  "$MISE_BIN" exec -C "$REPO_DIR" -- bash -c \
    'cd "$1" && shift && exec "$@"' _ "$BURBLE_DIR/server" \
    env MIX_ENV=test PHX_SERVER=true IDAPTIK_PORT="$PORT" mix "$@"
}

preflight_common() {
  local profile="$1" target="${2:-}"
  if [ "${IDAPTIK_PREFLIGHT_DONE:-0}" != "1" ]; then
    [ -x "$RUNTIME_DOCTOR" ] || die "runtime diagnostics missing: $RUNTIME_DOCTOR"
    "$RUNTIME_DOCTOR" preflight "$profile" "$target" || die "runtime preflight failed; run './launcher.sh --doctor'"
  fi
  [ -f "$RUN_CONFIG" ] || die "run config missing: $RUN_CONFIG (pull the repo?)"
  # An untrusted mise config is SILENTLY ignored and the global toolchain
  # resolves instead — trust it before anything builds (see AGENTS.md).
  if command -v mise >/dev/null 2>&1; then
    MISE_BIN="$(command -v mise)"
  elif [ -x "$HOME/.local/bin/mise" ]; then
    MISE_BIN="$HOME/.local/bin/mise"
  else
    die "mise is required — install it, then run 'mise install' in $REPO_DIR"
  fi
  (cd "$REPO_DIR" && "$MISE_BIN" trust -q)
  (cd "$REPO_DIR" && "$MISE_BIN" exec -- cargo --version >/dev/null) || \
    die "the pinned Rust toolchain is unavailable — run 'mise install'"
}

preflight_host() {
  preflight_common host
  [ -f "$BURBLE_DIR/server/mix.exs" ] || \
    die "Burble is required at $BURBLE_DIR — clone metadatastician/burble beside IDApTIK or set IDAPTIK_BURBLE_DIR"
  (cd "$REPO_DIR" && "$MISE_BIN" exec -- mix --version >/dev/null) || \
    die "the pinned Elixir/OTP toolchain is unavailable — run 'mise install'"
  need curl "to poll relay readiness (host only)" "install curl from your package manager"
}

build_seat() {
  # Always ask Cargo to build. Its fingerprinting makes an unchanged build
  # effectively free, while a freshly pulled source fix can never be hidden by
  # an older executable that happens to exist in target/release.
  say "checking the Bevy netplay client (a first build takes a few minutes)…"
  progress STEP "Building the Bevy GUI; joiners must wait until this completes"
  if [ "$CLIENT_PLATFORM" = "windows" ]; then
    (cd "$REPO_DIR" && "$MISE_BIN" exec -- cargo build --release --locked -q \
      -p idaptik-bevy --target x86_64-pc-windows-gnu)
  else
    (cd "$REPO_DIR" && "$MISE_BIN" exec -- cargo build --release --locked -q -p idaptik-bevy)
  fi
  [ -x "$BEVY_BIN" ] || die "build produced no $BEVY_BIN"
  progress PASS "Bevy GUI build is complete ($CLIENT_PLATFORM client)"
}

relay_pid() {
  [ -f "$PID_FILE" ] || return 1
  local pid
  pid="$(cat "$PID_FILE" 2>/dev/null)" || return 1
  kill -0 "$pid" 2>/dev/null || return 1
  printf '%s' "$pid"
}

# Probe the local HTTP root without a proxy. Success indicates reachability,
# not session admission or game readiness.
relay_up() { curl --noproxy '*' -fsS --connect-timeout 2 --max-time 3 "http://127.0.0.1:${PORT}/" >/dev/null 2>&1; }

start_relay() {
  if relay_up; then
    die "port $PORT is already answering; refusing to reuse an unverified relay (stop it explicitly)"
  fi
  idaptik_private_dirs "$RUNTIME_DIR" "$LOG_DIR"
  say "starting the relay on :$PORT (log: $LOG_FILE)…"
  progress STEP "Starting the Phoenix/Bandit relay on port $PORT"
  # This is Burble's test-only, fixed-secret profile. Bind LOOPBACK only:
  # its guest/role join is not secure remote-session admission. Never re-expose
  # it on LAN, tailnet or a public interface. Secure remote hosting needs a
  # separate authorised service contract, not an environment-variable opt-out.
  burble_mix deps.get >/dev/null
  burble_mix compile >/dev/null
  # Backgrounded as an *exec chain* so $! is the relay process itself. A
  # backgrounded function would leave $! pointing at a wrapper subshell, and
  # stop_relay's kill/pkill -P pair can then orphan the real relay process
  # (found by tests/launcher_fixture_test.sh).
  ( exec "$MISE_BIN" exec -C "$REPO_DIR" -- bash -c \
      'cd "$1" && shift && exec "$@"' _ "$BURBLE_DIR/server" \
      env MIX_ENV=test PHX_SERVER=true IDAPTIK_PORT="$PORT" mix run --no-start -e \
      'port = System.fetch_env!("IDAPTIK_PORT") |> String.to_integer(); endpoint = Application.get_env(:burble, BurbleWeb.Endpoint, []); Application.put_env(:burble, BurbleWeb.Endpoint, Keyword.merge(endpoint, server: true, http: [ip: {127, 0, 0, 1}, port: port]), persistent: true); Mix.Tasks.Phx.Server.run([])' ) \
    >>"$LOG_FILE" 2>&1 &
  echo $! > "$PID_FILE"
  # Seconds to wait for the relay to answer before diagnosing (default 60).
  # Also a fixture knob: tests/launcher_fixture_test.sh shrinks it so the
  # never-answered diagnosis is observable without a minute of wall time.
  local relay_timeout="${IDAPTIK_RELAY_STARTUP_TIMEOUT:-60}"
  local waited=0
  until relay_up; do
    waited=$((waited + 1))
    [ "$waited" -ge "$relay_timeout" ] && { tail -20 "$LOG_FILE" >&2 || true; die "relay never answered on :$PORT (see $LOG_FILE)"; }
    kill -0 "$(cat "$PID_FILE")" 2>/dev/null || { tail -20 "$LOG_FILE" >&2 || true; die "relay died during startup (see $LOG_FILE)"; }
    sleep 1
  done
  say "relay is up."
  progress PASS "Relay health check passed on port $PORT"
}

stop_relay() {
  local pid
  if pid="$(relay_pid)"; then
    kill "$pid" 2>/dev/null || true
    pkill -P "$pid" 2>/dev/null || true
    rm -f "$PID_FILE"
    say "relay stopped."
  else
    rm -f "$PID_FILE"
    say "no relay of ours is running."
  fi
}

share_addresses() {
  # Legacy test relay: local two-client development only. No public/tailnet
  # invitation and no router/portproxy instructions until authenticated host
  # admission and exclusive seat ownership are implemented and tested.
  HOST_SHARE_ADDRESS="127.0.0.1"
  say ""
  say "── local multiplayer only ─────────────────────────────────"
  say "  This test-profile Burble relay is bound to 127.0.0.1."
  say "  A second client may join from THIS computer only."
  say "  Remote LAN, Tailscale and public hosting are not supported by this profile."
  say "───────────────────────────────────────────────────────────"
  say ""
}

# Build the WebSocket URL from an already validated host:port authority.
seat_url() { printf 'ws://%s/voice/websocket' "$1"; }

# Return success if tasklist identifies $1 as an IDApTIK client PID. A
# tasklist failure also returns nonzero.
windows_pid_is_running() {
  local pid="$1"
  (cd "$(windows_cwd)" && "$WINDOWS_CMD" /D /C tasklist \
    /FI "PID eq $pid" /FO CSV /NH 2>/dev/null) |
    tr -d '\r' |
    grep -q "^\"idaptik-bevy.exe\",\"$pid\""
}

# Launch and track one Windows client using its exact PID. Write the supplied
# readiness record after verifying that PID, and attempt to stop that client
# on cancellation. Launch or PID verification failures exit with an error.
run_windows_client() { # mode target url role session run_config readiness
  local mode="$1" target="$2" url="$3" role="$4" session="$5" run_config="$6" readiness="$7"
  local pid pid_record powershell wrapper
  powershell="$(hp_windows_tool powershell.exe)" || die "native Windows multiplayer requires powershell.exe for per-seat process ownership"
  wrapper="$REPO_DIR/scripts/start-windows-client.ps1"
  [ -r "$wrapper" ] || die "Windows client helper is missing: $wrapper"
  hp_windows_script_preflight "$wrapper" || die "Windows client helper did not pass its policy preflight; no client was started"
  idaptik_private_dirs "$RUNTIME_DIR"
  pid_record="$(mktemp "$RUNTIME_DIR/windows-$role.XXXXXX")"
  # Start-Process returns the new client's PID. An image-name search would
  # select the host when launching the second local seat, or kill the wrong
  # window on cancellation. No game-specific authority is granted by this PID.
  if ! (cd "$(windows_cwd)" && "$powershell" -NoProfile -NonInteractive \
      -File "$(wslpath -w "$wrapper")" -Executable "$(wslpath -w "$BEVY_BIN")" \
      -Mode "$mode" -Target "$target" -Url "$url" -Session "$session" \
      -Role "$role" -RunConfig "$run_config" -PidFile "$(wslpath -w "$pid_record")"); then
    rm -f "$pid_record"
    die "Windows client launch failed before process ownership was recorded; inspect the PowerShell diagnostic and the machine's approved script/signing policy"
  fi
  pid="$(tr -d '\r\n' < "$pid_record")"
  rm -f "$pid_record"
  case "$pid" in *[!0-9]*|'') die "Windows returned an invalid client PID" ;; esac
  [ "$pid" -gt 0 ] || die "Windows returned an invalid client PID"
  windows_pid_is_running "$pid" || die "the Windows client exited before readiness"
  say "Windows client: PID $pid"

  cleanup_windows_client() {
    if windows_pid_is_running "$pid"; then
      (cd "$(windows_cwd)" && "$WINDOWS_CMD" /D /C taskkill /PID "$pid" /T /F >/dev/null 2>&1) || true
    fi
  }
  trap cleanup_windows_client EXIT
  trap 'cleanup_windows_client; exit 143' HUP INT TERM
  mark_ready "$readiness"
  while windows_pid_is_running "$pid"; do sleep 1; done
  trap - EXIT HUP INT TERM
}

run_seat() { # mode target url role session readiness
  local mode="$1" target="$2" url="$3" role="$4" session="$5" readiness="$6"
  local run_config="$RUN_CONFIG"
  if [ "$CLIENT_PLATFORM" = "windows" ]; then
    run_config="$(wslpath -w "$RUN_CONFIG")"
  fi
  say "seat: $role · session: $session · relay: $url · frontend: Bevy GUI ($CLIENT_PLATFORM)"
  say "keys: arrows/WASD move · E interact · Q throw · 1-4 uplinks · Tab changes view · Esc quits"
  case "$mode" in
    host)
      if [ "$CLIENT_PLATFORM" = "windows" ]; then
        run_windows_client host "$target" "$url" "$role" "$session" "$run_config" "$readiness"
      else
        mark_ready "$readiness"
        exec "$BEVY_BIN" --host --url "$url" --session "$session" \
          --role "$role" --script "$run_config"
      fi
      ;;
    join)
      if [ "$CLIENT_PLATFORM" = "windows" ]; then
        run_windows_client join "$target" "$url" "$role" "$session" "$run_config" "$readiness"
      else
        mark_ready "$readiness"
        exec "$BEVY_BIN" --join "$target" --url "$url" --session "$session" \
          --role "$role" --script "$run_config"
      fi
      ;;
    *) die "internal error: unknown seat mode $mode" ;;
  esac
}

# ----------------------------------------------------------------------------
# modes
# ----------------------------------------------------------------------------

mode_host() {
  local role="infiltrator" session="$SESSION_DEFAULT"
  while [ $# -gt 0 ]; do
    case "$1" in
      --role) role="$2"; shift 2 ;;
      --session) session="$2"; shift 2 ;;
      --rebuild) REBUILD=1; shift ;;
      *) die "host: unknown argument $1" ;;
    esac
  done
  case "$role" in infiltrator|hacker) ;; *) die "role must be infiltrator or hacker" ;; esac
  preflight_host
  progress PASS "Host machine, display, dependencies, and local-only profile passed preflight"
  build_seat "${REBUILD:+--rebuild}"
  start_relay
  share_addresses
  [ "$HOST_SHARE_ADDRESS" = "127.0.0.1" ] || die "local-only relay address is invalid"
  progress PASS "Local-only test relay is ready at $HOST_SHARE_ADDRESS:$PORT"
  say "waiting in the lobby for the other seat… (relay keeps running after you quit;"
  say "'./launcher.sh --stop' ends it)"
  progress PASS "Host seat is launching for session $session as $role"
  run_seat host "127.0.0.1" "$(seat_url "127.0.0.1:$PORT")" "$role" "$session" \
    "host|$HOST_SHARE_ADDRESS|$PORT|$session|$role"
}

mode_join() {
  [ $# -ge 1 ] || die "join needs a local relay address: join 127.0.0.1[:port] [--role R] [--session S]"
  local target="$1"; shift
  # Always enforce this, including when the launcher has already completed
  # preflight. A direct internal invocation must not turn into a remote client.
  target="$(idaptik_local_target "$target")" || die "join refused: this test relay is local-only"
  local role="hacker" session="$SESSION_DEFAULT"
  while [ $# -gt 0 ]; do
    case "$1" in
      --role) role="$2"; shift 2 ;;
      --session) session="$2"; shift 2 ;;
      *) die "join: unknown argument $1" ;;
    esac
  done
  case "$role" in infiltrator|hacker) ;; *) die "role must be infiltrator or hacker" ;; esac
  preflight_common join "$target"
  progress PASS "Join machine, display, dependencies, and local relay passed preflight"
  build_seat
  progress PASS "Host $target is reachable and the local seat is launching as $role"
  run_seat join "$target" "$(seat_url "$target")" "$role" "$session" \
    "join|$target|$session|$role"
}

mode_status() {
  local pid
  if pid="$(relay_pid)"; then
    say "relay: running (pid $pid, port $PORT, log $LOG_FILE)"
  elif relay_up; then
    say "relay: answering on :$PORT (not started by this runtime)"
  else
    say "relay: not running"
  fi
  if [ -x "$BEVY_BIN" ]; then
    say "GUI binary: built ($BEVY_BIN)"
  else
    say "GUI binary: not built yet"
  fi
}

# ----------------------------------------------------------------------------
# internal dispatch — launcher.sh is the only player-facing entry point
# ----------------------------------------------------------------------------

MODE="${1:-}"
[ $# -gt 0 ] && shift || true

[ "${IDAPTIK_INTERNAL_RUNTIME:-0}" = "1" ] || \
  die "this is an internal component; players must use $REPO_DIR/launcher.sh"

case "$MODE" in
  host)          mode_host "$@" ;;
  join)          mode_join "$@" ;;
  __relay-stop)  stop_relay ;;
  __relay-status) mode_status ;;
  *) die "internal multiplayer runtime; players must use $REPO_DIR/launcher.sh" ;;
esac
