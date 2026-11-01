#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
#
# Internal multiplayer runtime for launcher.sh. This is deliberately not a
# launcher and has no lifecycle/UI aliases: players use ./launcher.sh only.
# It owns the Phoenix relay process, release build, host/join seat execution,
# readiness events, and peer-address discovery behind the canonical launcher.
#
# Environment: IDAPTIK_PORT (relay port, default 4000), IDAPTIK_BURBLE_DIR
# (Burble checkout, default sibling ../burble), IDAPTIK_CLIENT_PLATFORM
# (linux|windows), IDAPTIK_RELAY_STARTUP_TIMEOUT (relay readiness wait in
# seconds, default 60; primarily a fixture knob).

set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
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

PORT="${IDAPTIK_PORT:-4000}"
SESSION_DEFAULT="ghost-lobby"

PID_FILE="${XDG_RUNTIME_DIR:-${TMPDIR:-/tmp}}/idaptik-relay.pid"
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

# WSL2's virtual adapter is NAT'd behind Windows: an address from it is
# unreachable from any other machine, so the share banner must not offer it.
is_wsl() { grep -qi microsoft /proc/version 2>/dev/null; }

# True when running under WSL *and* holding an address in WSL's NAT block
# (172.16/12). WSL2 mirrored networking (Windows 11 22H2+) shares the host's
# real LAN address instead and is reachable, so it must not be warned about.
wsl_natted() {
  is_wsl || return 1
  case "$1" in
    172.1[6-9].*|172.2[0-9].*|172.3[01].*) return 0 ;;
    *) return 1 ;;
  esac
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

relay_up() { curl -fsS "http://127.0.0.1:${PORT}/" >/dev/null 2>&1; }

start_relay() {
  if relay_up; then
    say "relay already answering on :$PORT — reusing it."
    progress PASS "Relay is already answering on port $PORT"
    return 0
  fi
  mkdir -p "$LOG_DIR"
  say "starting the relay on :$PORT (log: $LOG_FILE)…"
  progress STEP "Starting the Phoenix/Bandit relay on port $PORT"
  # Burble's test environment is its offline-capable local profile. Set the
  # launcher-selected bind/port before delegating to Phoenix's normal server
  # task, preserving Burble's application startup order.
  burble_mix deps.get >/dev/null
  burble_mix compile >/dev/null
  # Backgrounded as an *exec chain* so $! is the relay process itself. A
  # backgrounded function would leave $! pointing at a wrapper subshell, and
  # stop_relay's kill/pkill -P pair can then orphan the real relay process
  # (found by tests/launcher_fixture_test.sh).
  ( exec "$MISE_BIN" exec -C "$REPO_DIR" -- bash -c \
      'cd "$1" && shift && exec "$@"' _ "$BURBLE_DIR/server" \
      env MIX_ENV=test PHX_SERVER=true IDAPTIK_PORT="$PORT" mix run --no-start -e \
      'port = System.fetch_env!("IDAPTIK_PORT") |> String.to_integer(); endpoint = Application.get_env(:burble, BurbleWeb.Endpoint, []); Application.put_env(:burble, BurbleWeb.Endpoint, Keyword.merge(endpoint, server: true, http: [ip: {0, 0, 0, 0}, port: port]), persistent: true); Mix.Tasks.Phx.Server.run([])' ) \
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
  say ""
  say "── tell the other player ──────────────────────────────────"
  local shared=0
  HOST_SHARE_ADDRESS=""

  if command -v tailscale >/dev/null 2>&1; then
    local ts
    ts="$(tailscale ip -4 2>/dev/null | head -1 || true)"
    if [ -n "$ts" ]; then
      say "  tailnet address for ./launcher.sh: $ts"
      HOST_SHARE_ADDRESS="$ts"
      shared=1
    fi
  fi

  local lan
  lan="$(ip -4 route get 1.1.1.1 2>/dev/null | grep -oP 'src \K[0-9.]+' | head -1 || true)"

  if [ -n "$lan" ] && wsl_natted "$lan"; then
    # WSL2's default (NAT) networking puts this machine behind an adapter only
    # Windows can see through, so the address is unreachable from anywhere else
    # — and forwarding the router port cannot help, because the router cannot
    # see past Windows either. WSL2 *mirrored* networking shares the host's real
    # LAN address and is fine, which is why this keys on the address, not on
    # merely running under WSL.
    say "  WARNING: WSL2 NAT detected. $lan is a virtual adapter behind Windows"
    say "  and is NOT reachable from another machine, even on the same network."
    if [ "$shared" -eq 0 ]; then
      say "    Fix (once, both machines): install Tailscale, then re-run host —"
      say "      curl -fsSL https://tailscale.com/install.sh | sh && sudo tailscale up"
      say "    Alternative: forward TCP $PORT on the router AND add a Windows"
      say "    portproxy to $lan (netsh interface portproxy)."
    fi
  elif [ -n "$lan" ]; then
    say "  LAN address for ./launcher.sh: $lan"
    HOST_SHARE_ADDRESS="$lan"
    shared=1
  fi

  # Always leave a route out: a host with no detectable address still needs to
  # know the fallback, and so does one whose LAN address works locally but not
  # from outside.
  if [ "$shared" -eq 0 ] || [ -n "$lan" ]; then
    say "  (internet without a tunnel: forward TCP $PORT, share your public IP)"
  fi
  say "───────────────────────────────────────────────────────────"
  say ""
}

seat_url() {
  # Accept a bare host, host:port, or a full ws:// url.
  case "$1" in
    ws://*|wss://*) printf '%s' "$1" ;;
    *:*)            printf 'ws://%s/voice/websocket' "$1" ;;
    *)              printf 'ws://%s:%s/voice/websocket' "$1" "$PORT" ;;
  esac
}

windows_client_pid() {
  (cd "$(windows_cwd)" && "$WINDOWS_CMD" /D /C tasklist \
    /FI "IMAGENAME eq idaptik-bevy.exe" /FO CSV /NH 2>/dev/null) |
    tr -d '\r' |
    sed -n 's/^"idaptik-bevy\.exe","\([0-9][0-9]*\)".*/\1/p' |
    head -1
}

windows_pid_is_running() {
  local pid="$1"
  (cd "$(windows_cwd)" && "$WINDOWS_CMD" /D /C tasklist \
    /FI "PID eq $pid" /FO CSV /NH 2>/dev/null) |
    tr -d '\r' |
    grep -q "^\"idaptik-bevy.exe\",\"$pid\""
}

run_windows_client() {
  local pid="" waited=0
  [ -z "$(windows_client_pid)" ] || die "a Windows IDApTIK client is already running; close it or use './launcher.sh --stop'"

  # WSL interop detaches a Windows GUI process when this runtime itself was
  # backgrounded by launcher.sh. Capture the resulting Windows PID and keep
  # this Linux owner alive so its Phoenix child relay is not reaped with it.
  "$@"
  while [ -z "$pid" ] && [ "$waited" -lt 10 ]; do
    pid="$(windows_client_pid)"
    [ -n "$pid" ] || sleep 1
    waited=$((waited + 1))
  done
  [ -n "$pid" ] || die "Windows reported no idaptik-bevy.exe after launch"
  say "Windows client: PID $pid"

  cleanup_windows_client() {
    if windows_pid_is_running "$pid"; then
      (cd "$(windows_cwd)" && "$WINDOWS_CMD" /D /C taskkill /PID "$pid" /T /F >/dev/null 2>&1) || true
    fi
  }
  trap cleanup_windows_client EXIT
  trap 'cleanup_windows_client; exit 143' HUP INT TERM
  while windows_pid_is_running "$pid"; do sleep 1; done
  trap - EXIT HUP INT TERM
}

run_seat() { # $1 host|join  $2 target  $3 url  $4 role  $5 session
  local mode="$1" target="$2" url="$3" role="$4" session="$5"
  local run_config="$RUN_CONFIG"
  if [ "$CLIENT_PLATFORM" = "windows" ]; then
    run_config="$(wslpath -w "$RUN_CONFIG")"
  fi
  say "seat: $role · session: $session · relay: $url · frontend: Bevy GUI ($CLIENT_PLATFORM)"
  say "keys: arrows/WASD move · E interact · Q throw · 1-4 uplinks · Tab changes view · Esc quits"
  case "$mode" in
    host)
      if [ "$CLIENT_PLATFORM" = "windows" ]; then
        run_windows_client "$BEVY_BIN" --host --url "$url" --session "$session" \
          --role "$role" --script "$run_config"
      else
        exec "$BEVY_BIN" --host --url "$url" --session "$session" \
          --role "$role" --script "$run_config"
      fi
      ;;
    join)
      if [ "$CLIENT_PLATFORM" = "windows" ]; then
        run_windows_client "$BEVY_BIN" --join "$target" --url "$url" --session "$session" \
          --role "$role" --script "$run_config"
      else
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
  progress PASS "Host machine, WSLg, dependencies, and tailnet route passed preflight"
  build_seat "${REBUILD:+--rebuild}"
  start_relay
  share_addresses
  [ -n "${HOST_SHARE_ADDRESS:-}" ] || die "no peer-reachable host address is available"
  progress PASS "Peer route is ready at $HOST_SHARE_ADDRESS:$PORT"
  say "waiting in the lobby for the other seat… (relay keeps running after you quit;"
  say "'./launcher.sh --stop' ends it)"
  progress PASS "Host seat is launching for session $session as $role"
  mark_ready "host|$HOST_SHARE_ADDRESS|$PORT|$session|$role"
  run_seat host "127.0.0.1" "$(seat_url "127.0.0.1:$PORT")" "$role" "$session"
}

mode_join() {
  [ $# -ge 1 ] || die "join needs the host's address: join <host|host:port|ws://…> [--role R] [--session S]"
  local target="$1"; shift
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
  progress PASS "Join machine, WSLg, dependencies, and host relay passed preflight"
  build_seat
  progress PASS "Host $target is reachable and the local seat is launching as $role"
  mark_ready "join|$target|$session|$role"
  run_seat join "$target" "$(seat_url "$target")" "$role" "$session"
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
