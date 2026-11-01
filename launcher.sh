#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
#
set -euo pipefail

APP_NAME="idaptik"
APP_DISPLAY="IDApTIK"
APP_DESC="Asymmetric two-player infiltration game"
APP_CATEGORIES="Game;Network;"
VERSION="0.5.0"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/$APP_NAME"
REPO_PATH_FILE="$CONFIG_DIR/repo-path"

# An integrated copy lives outside the repository. It finds the source tree
# through the path recorded by --integ; a source-tree invocation needs no
# configuration and always wins over a stale recorded path.
if [ -f "$SCRIPT_DIR/Cargo.toml" ] && [ -f "$SCRIPT_DIR/scripts/multiplayer-runtime.sh" ]; then
  REPO_DIR="$SCRIPT_DIR"
elif [ -n "${IDAPTIK_REPO_DIR:-}" ]; then
  REPO_DIR="$IDAPTIK_REPO_DIR"
elif [ -r "$REPO_PATH_FILE" ]; then
  IFS= read -r REPO_DIR < "$REPO_PATH_FILE"
else
  REPO_DIR="$SCRIPT_DIR"
fi

RUNTIME_DIR="${XDG_RUNTIME_DIR:-${TMPDIR:-/tmp}}"
STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/$APP_NAME"
PID_FILE="$RUNTIME_DIR/$APP_NAME-server.pid"
LOG_FILE="$STATE_DIR/game.log"
PROGRESS_FILE="$STATE_DIR/startup.progress"
READY_FILE="$STATE_DIR/startup.ready"
PORT="${IDAPTIK_PORT:-4000}"
MULTIPLAYER_RUNTIME="$REPO_DIR/scripts/multiplayer-runtime.sh"
RUNTIME_DOCTOR="$REPO_DIR/scripts/runtime-doctor.sh"
WINDOWS_INTEROP_LIB="$REPO_DIR/scripts/windows-interop.sh"

# Windows interop discovery is shared with the runtime doctor and the
# installer, so "can we run the Windows client?" has one answer. The library
# only defines functions, so a missing one is harmless until a launch mode
# needs it; require_repo refuses to launch without it.
if [ -r "$WINDOWS_INTEROP_LIB" ]; then
  # shellcheck source=scripts/windows-interop.sh
  . "$WINDOWS_INTEROP_LIB"
fi

say()  { printf '[%s] %s\n' "$APP_NAME" "$*"; }
warn() { printf '[%s] WARN: %s\n' "$APP_NAME" "$*" >&2; }
die()  {
  if [ -n "${IDAPTIK_PROGRESS_FILE:-}" ]; then
    printf 'FAIL|%s\n' "$*" >> "$IDAPTIK_PROGRESS_FILE"
  fi
  if declare -F hp_gui_error >/dev/null 2>&1; then
    hp_gui_error "$APP_DISPLAY launcher error" "$*"
  else
    printf '[%s] ERROR: %s\n' "$APP_NAME" "$*" >&2
  fi
  exit 1
}

platform() {
  local os arch
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  arch="$(uname -m)"
  case "$os" in
    mingw*|msys*|cygwin*) os="windows" ;;
    darwin*) os="macos" ;;
    linux*) os="linux" ;;
  esac
  printf '%s-%s' "$os" "$arch"
}

is_wsl() { hp_is_wsl; }

wslg_copy_mode_active() { hp_wslg_copy_mode_broken; }

windows_interop_available() { hp_windows_interop; }

select_client_platform() {
  # An explicit request wins, but is still validated: the Windows client is a
  # Windows executable, so it needs WSL2 and working interop no matter who
  # asked for it.
  if [ "${IDAPTIK_CLIENT_PLATFORM:-}" = "windows" ]; then
    is_wsl ||
      die "the native Windows client runs only under WSL2; this host is $(platform). Drop --windows-client."
    windows_interop_available ||
      die "the native Windows client needs Windows interop: $(hp_windows_interop_reason)"
    return 0
  fi
  [ -z "${IDAPTIK_CLIENT_PLATFORM:-}" ] || return 0
  if is_wsl && wslg_copy_mode_active && windows_interop_available; then
    export IDAPTIK_CLIENT_PLATFORM=windows
    warn "WSLg Copy Mode is broken; using the native Windows Bevy client instead"
  fi
}

build_sha() {
  git -C "$REPO_DIR" rev-parse --short HEAD 2>/dev/null || printf 'unknown'
}

# --- renderer evidence -------------------------------------------------------

# The launcher used to report success without ever saying which adapter the
# client got, so Vulkan-on-llvmpipe looked exactly like the discrete GPU until
# the frame rate gave it away. Bevy logs the AdapterInfo it selected at INFO;
# read that back rather than inferring anything from the build succeeding.
renderer_line() {
  local log="${1:-$LOG_FILE}"
  [ -r "$log" ] || return 1
  grep -a -m1 'AdapterInfo' "$log" 2>/dev/null || return 1
}

renderer_name() {
  sed -n 's/.*name: *"\([^"]*\)".*/\1/p' <<<"$1" | head -1
}

renderer_backend() {
  sed -n 's/.*backend: *\([A-Za-z0-9_-]*\).*/\1/p' <<<"$1" | head -1
}

print_renderer() { # $1 = AdapterInfo line
  local name backend
  name="$(renderer_name "$1")"
  backend="$(renderer_backend "$1")"
  case "$name" in
    *llvmpipe*|*softpipe*|*swiftshader*|*SwiftShader*|*WARP*|*"Basic Render"*)
      warn "renderer: ${name:-unknown}${backend:+ ($backend)} — SOFTWARE rendering; expect low frame rates" ;;
    '')
      say "renderer: ${1#*AdapterInfo}" ;;
    *)
      say "renderer: $name${backend:+ ($backend)}" ;;
  esac
}

# Blocking variant used once the solo client is up: wait briefly for the
# adapter line, and fail if the client dies before it gets that far (the WSLg
# presentation failure happens exactly here).
report_renderer() { # $1 = pid
  local pid="$1" waited=0 limit="${IDAPTIK_RENDERER_WAIT:-6}" line
  while [ "$waited" -lt "$limit" ]; do
    if line="$(renderer_line)"; then
      print_renderer "$line"
      return 0
    fi
    kill -0 "$pid" 2>/dev/null || return 1
    sleep 1
    waited=$((waited + 1))
  done
  say "renderer: not reported within ${limit}s (set IDAPTIK_RENDERER_WAIT to wait longer); see $LOG_FILE"
  return 0
}

show_last_renderer() {
  local line
  if line="$(renderer_line)"; then
    print_renderer "$line"
  else
    say "renderer: no adapter line in $LOG_FILE yet"
  fi
}

pid_is_game() {
  [ -f "$PID_FILE" ] || return 1
  local pid command
  IFS= read -r pid < "$PID_FILE" || return 1
  case "$pid" in *[!0-9]*|'') return 1 ;; esac
  kill -0 "$pid" 2>/dev/null || return 1
  command="$(ps -p "$pid" -o args= 2>/dev/null || true)"
  case "$command" in
    *idaptik-bevy*|*launcher.sh\ __run-*|*scripts/multiplayer-runtime.sh*) return 0 ;;
    *) return 1 ;;
  esac
}

require_repo() {
  [ -f "$REPO_DIR/Cargo.toml" ] || die "repository not found at $REPO_DIR (set IDAPTIK_REPO_DIR or rerun --integ from the repository)"
  [ -x "$MULTIPLAYER_RUNTIME" ] || die "internal multiplayer runtime missing or not executable: $MULTIPLAYER_RUNTIME"
  [ -x "$RUNTIME_DOCTOR" ] || die "runtime diagnostics missing or not executable: $RUNTIME_DOCTOR"
  [ -r "$WINDOWS_INTEROP_LIB" ] || die "shared launch library missing: $WINDOWS_INTEROP_LIB (incomplete checkout)"
}

runtime_preflight() {
  local profile="$1" target="${2:-}"
  require_repo
  select_client_platform
  if ! "$RUNTIME_DOCTOR" preflight "$profile" "$target"; then
    die "runtime preflight failed; run './launcher.sh --doctor' for the full report"
  fi
}

trust_mise() {
  if command -v mise >/dev/null 2>&1; then
    MISE_BIN="$(command -v mise)"
  elif [ -x "$HOME/.local/bin/mise" ]; then
    MISE_BIN="$HOME/.local/bin/mise"
  else
    die "mise is required; install it, then run 'mise install' in $REPO_DIR"
  fi
  (cd "$REPO_DIR" && "$MISE_BIN" trust -q)
}

run_solo() {
  require_repo
  trust_mise
  cd "$REPO_DIR"
  progress PASS "Pinned Rust dependencies are available"
  local bevy_bin="$REPO_DIR/target/debug/idaptik-bevy"
  progress STEP "Building the solo GUI (Cargo reuses completed work)"
  if [ "${IDAPTIK_CLIENT_PLATFORM:-linux}" = "windows" ]; then
    bevy_bin="$REPO_DIR/target/x86_64-pc-windows-gnu/debug/idaptik-bevy.exe"
    "$MISE_BIN" exec -- cargo build -q -p idaptik-bevy --target x86_64-pc-windows-gnu
    # Debug builds make wgpu label Vulkan render passes via VK_EXT_debug_utils.
    # Some Windows Vulkan drivers (seen: Intel HD 620) advertise the extension
    # but cannot load cmd_begin_debug_utils_label_ext, so the client panics a
    # second after opening its window. Disable the labels, and forward the
    # variable across the WSL->Windows boundary (plain name; the /u flag does not
    # forward it). An explicit user setting still wins.
    export WGPU_DEBUG="${WGPU_DEBUG:-0}"
    case ":${WSLENV:-}:" in *":WGPU_DEBUG:"*) ;; *) export WSLENV="${WSLENV:+$WSLENV:}WGPU_DEBUG" ;; esac
  else
    "$MISE_BIN" exec -- cargo build -q -p idaptik-bevy
  fi
  progress PASS "Solo GUI build is complete (${IDAPTIK_CLIENT_PLATFORM:-linux} client)"
  mark_ready "solo"
  exec "$bevy_bin" --local
}

progress() {
  [ -n "${IDAPTIK_PROGRESS_FILE:-}" ] || return 0
  printf '%s|%s\n' "$1" "$2" >> "$IDAPTIK_PROGRESS_FILE"
}

mark_ready() {
  [ -n "${IDAPTIK_READY_FILE:-}" ] || return 0
  printf '%s\n' "$1" > "$IDAPTIK_READY_FILE"
}

show_progress_line() {
  local kind="${1%%|*}" detail="${1#*|}"
  case "$kind" in
    PASS) printf '  ✓ %s\n' "$detail" ;;
    WARN) printf '  ! %s\n' "$detail" ;;
    FAIL) printf '  ✗ %s\n' "$detail" ;;
    STEP) printf '  … %s\n' "$detail" ;;
    *) printf '  · %s\n' "$detail" ;;
  esac
}

start_game() {
  if pid_is_game; then
    say "game already running (PID $(<"$PID_FILE")); stop it first with --stop"
    return 0
  fi

  local profile="$1" target="$2" label="$3"
  shift 3
  runtime_preflight "$profile" "$target"
  mkdir -p "$STATE_DIR"
  : > "$LOG_FILE"
  : > "$PROGRESS_FILE"
  rm -f "$READY_FILE"
  say "launching $APP_DISPLAY — $label (GUI)"
  say "startup log: $LOG_FILE"

  if [ "${IDAPTIK_LAUNCHER_DRY_RUN:-0}" = "1" ]; then
    # Name the client platform in the dry run: "solo" does not tell you
    # whether that is the Linux client or the Windows one across interop.
    printf '[%s] DRY RUN: IDAPTIK_CLIENT_PLATFORM=%s' \
      "$APP_NAME" "${IDAPTIK_CLIENT_PLATFORM:-linux}"
    printf ' %q' "$@"
    printf '\n'
    return 0
  fi

  nohup env \
    IDAPTIK_INTERNAL_RUNTIME=1 \
    IDAPTIK_PREFLIGHT_DONE=1 \
    IDAPTIK_PROGRESS_FILE="$PROGRESS_FILE" \
    IDAPTIK_READY_FILE="$READY_FILE" \
    "$@" >>"$LOG_FILE" 2>&1 &
  local pid=$!
  printf '%s\n' "$pid" > "$PID_FILE"

  printf '\nStartup checklist:\n'
  local waited=0 shown=0 lines line startup_timeout="${IDAPTIK_STARTUP_TIMEOUT:-1800}"
  while [ ! -f "$READY_FILE" ]; do
    lines="$(wc -l < "$PROGRESS_FILE" 2>/dev/null || printf 0)"
    if [ "$lines" -gt "$shown" ]; then
      while IFS= read -r line; do show_progress_line "$line"; done < <(sed -n "$((shown + 1)),${lines}p" "$PROGRESS_FILE")
      shown="$lines"
    fi
    if ! kill -0 "$pid" 2>/dev/null; then
      rm -f "$PID_FILE"
      printf '  ✗ Startup process exited before readiness.\n' >&2
      tail -n 30 "$LOG_FILE" >&2 || true
      die "nothing is ready; see $LOG_FILE or run './launcher.sh --flowdiags'"
    fi
    if [ "$waited" -ge "$startup_timeout" ]; then
      warn "startup is still incomplete after ${startup_timeout}s; joiners must wait"
      say "check progress with: tail -f $LOG_FILE"
      return 1
    fi
    sleep 1
    waited=$((waited + 1))
  done
  lines="$(wc -l < "$PROGRESS_FILE" 2>/dev/null || printf 0)"
  if [ "$lines" -gt "$shown" ]; then
    while IFS= read -r line; do show_progress_line "$line"; done < <(sed -n "$((shown + 1)),${lines}p" "$PROGRESS_FILE")
  fi
  sleep 1
  if ! kill -0 "$pid" 2>/dev/null; then
    rm -f "$PID_FILE" "$READY_FILE"
    printf '  ✗ Game process exited during the final readiness check.\n' >&2
    tail -n 30 "$LOG_FILE" >&2 || true
    client_failed_hint
    die "readiness was withdrawn; joiners must not start"
  fi
  local ready_kind ready_host ready_port _ready_session ready_role peer_role
  IFS='|' read -r ready_kind ready_host ready_port _ready_session ready_role < "$READY_FILE"
  [ "$ready_kind" = "$profile" ] || die "invalid readiness record for $profile: $ready_kind"
  case "$profile" in
    host)
      case "$ready_role" in
        infiltrator) peer_role="hacker" ;;
        hacker) peer_role="infiltrator" ;;
        *) die "invalid host readiness record: $ready_role" ;;
      esac
      printf '\n\033[1;32m● READY FOR JOINERS\033[0m\n'
      say "joiner address: $ready_host:$ready_port"
      say "give the other player: ./launcher.sh --join $ready_host:$ready_port --role $peer_role"
      ;;
    join) printf '\n\033[1;32m● READY — JOINING HOST NOW\033[0m\n' ;;
    solo) printf '\n\033[1;32m● READY — OPENING SOLO GUI\033[0m\n' ;;
  esac
  if [ "$profile" = "solo" ]; then
    # Readiness says the client started; it does not say what drew the frame.
    if ! report_renderer "$pid"; then
      rm -f "$PID_FILE" "$READY_FILE"
      printf '  ✗ The client exited before choosing a renderer.\n' >&2
      tail -n 30 "$LOG_FILE" >&2 || true
      client_failed_hint
      die "the solo client died during surface setup; nothing is running"
    fi
  fi
  say "game process: PID $pid"
}

# The known WSLg presentation failure (Fallback system failed to choose present
# mode; DRI2 driver load errors) kills the window within a second of opening.
# Naming the route that works beats repeating the same launch.
client_failed_hint() {
  if is_wsl && [ "${IDAPTIK_CLIENT_PLATFORM:-linux}" != "windows" ] &&
    windows_interop_available; then
    printf '  hint: a WSLg client that dies this early is usually the GL/Vulkan\n' >&2
    printf '        presentation failure, not a game bug. Bypass WSLg with the\n' >&2
    printf '        native Windows client:\n' >&2
    printf '          %s --solo --windows-client\n' "$0" >&2
  fi
}

start_solo() {
  start_game solo "" "solo (controls both roles)" "$REPO_DIR/launcher.sh" __run-solo
}

start_host() {
  local role="$1"
  start_game host "" "multiplayer host · $role" "$MULTIPLAYER_RUNTIME" host --role "$role"
}

start_join() {
  local host="$1" role="$2"
  start_game join "$host" "multiplayer join · $role · $host" "$MULTIPLAYER_RUNTIME" join "$host" --role "$role"
}

choose_role() {
  local choice
  printf '\nChoose your character / seat:\n' >&2
  printf '  1) Infiltrator — movement, stealth, physical interaction\n' >&2
  printf '  2) Hacker      — network access, doors, cameras, uplinks\n' >&2
  while true; do
    read -rp 'Character [1-2]: ' choice
    case "$choice" in
      1|i|I|infiltrator) printf 'infiltrator'; return 0 ;;
      2|h|H|hacker) printf 'hacker'; return 0 ;;
      *) warn "choose 1 (Infiltrator) or 2 (Hacker)" ;;
    esac
  done
}

menu() {
  local mode action role host
  [ -t 0 ] || die "interactive menu needs a terminal; use --solo, --host, or --join HOST"
  cat <<'MENU'

IDApTIK — launch menu

  1) Solo GUI         Control both sides of Ghost Lobby locally
  2) Multiplayer GUI  Host or join a two-player session
  3) Status
  4) Diagnostics and safe repair
  5) Quit
MENU
  while true; do
    read -rp 'Mode [1-5]: ' mode
    case "$mode" in
      1|s|S|solo) start_solo; return 0 ;;
      2|m|M|multiplayer)
        printf '\n  1) Host — start the relay and create a session\n' >&2
        printf '  2) Join — connect to another player\n' >&2
        while true; do
          read -rp 'Multiplayer [1-2]: ' action
          case "$action" in
            1|h|H|host)
              role="$(choose_role)"
              start_host "$role"
              return 0
              ;;
            2|j|J|join)
              read -rp 'Host address (name, IP, host:port, or ws:// URL): ' host
              [ -n "$host" ] || { warn "host address cannot be empty"; continue; }
              role="$(choose_role)"
              start_join "$host" "$role"
              return 0
              ;;
            *) warn "choose 1 (Host) or 2 (Join)" ;;
          esac
        done
        ;;
      3|status) mode_status; return $? ;;
      4|d|D|doctor|diagnostics)
        "$RUNTIME_DOCTOR" report all || true
        printf '\n  r) Attempt safe dependency/network repair\n' >&2
        printf '  f) Open the friendly troubleshooting flowchart\n' >&2
        printf '  Enter) Return to the shell\n' >&2
        read -rp 'Diagnostics action [r/f/Enter]: ' action
        case "$action" in
          r|R|repair) "$RUNTIME_DOCTOR" repair all ;;
          f|F|flow|flowdiags) mode_flowdiags ;;
        esac
        return 0
        ;;
      5|q|Q|quit|exit) return 0 ;;
      *) warn "choose 1, 2, 3, 4, or 5" ;;
    esac
  done
}

stop_game() {
  if pid_is_game; then
    local pid
    IFS= read -r pid < "$PID_FILE"
    say "stopping game (PID $pid)"
    kill "$pid"
    local waited=0
    while kill -0 "$pid" 2>/dev/null && [ "$waited" -lt 10 ]; do
      sleep 1
      waited=$((waited + 1))
    done
    if kill -0 "$pid" 2>/dev/null; then
      warn "game did not stop within 10 seconds (PID $pid)"
      return 1
    fi
  else
    say "game is not running"
  fi
  rm -f "$PID_FILE" "$READY_FILE" "$PROGRESS_FILE"
}

stop_all() {
  local status=0
  stop_game || status=1
  if [ -x "$MULTIPLAYER_RUNTIME" ]; then
    IDAPTIK_INTERNAL_RUNTIME=1 "$MULTIPLAYER_RUNTIME" __relay-stop || status=1
  fi
  return "$status"
}

mode_status() {
  local status=1
  if pid_is_game; then
    say "game: running (PID $(<"$PID_FILE"))"
    say "log: $LOG_FILE"
    if [ -r "$READY_FILE" ]; then
      say "readiness: $(<"$READY_FILE")"
    else
      say "readiness: startup incomplete — joiners must wait"
    fi
    status=0
  else
    say "game: stopped"
    [ ! -e "$READY_FILE" ] || say "readiness: stale record present (run --stop before relaunching)"
  fi
  # Report the adapter the last client actually chose, running or not: this is
  # the question "am I on the GPU or on llvmpipe?" that a build success cannot
  # answer.
  show_last_renderer
  if [ -x "$MULTIPLAYER_RUNTIME" ]; then
    IDAPTIK_INTERNAL_RUNTIME=1 "$MULTIPLAYER_RUNTIME" __relay-status && status=0 || true
  else
    say "relay: unavailable (internal multiplayer runtime not found)"
  fi
  return "$status"
}

mode_flowdiags() {
  local flow="$REPO_DIR/docs/player-launch-flow.html" explorer=""
  [ -r "$flow" ] || die "diagnostic flowchart is missing: $flow"
  if declare -F hp_windows_tool >/dev/null 2>&1; then
    explorer="$(hp_windows_tool explorer.exe 2>/dev/null || true)"
  fi
  # Under WSL the Windows browser is used because it keeps working when WSLg
  # windows are broken; resolving explorer.exe through interop rather than
  # PATH is what makes that true on a default WSL PATH.
  if is_wsl && [ -n "$explorer" ] && command -v wslpath >/dev/null 2>&1; then
    nohup "$explorer" "$(wslpath -w "$flow")" >/dev/null 2>&1 &
    say "opened the diagnostic flowchart in the Windows browser"
  elif command -v xdg-open >/dev/null 2>&1; then
    nohup xdg-open "$flow" >/dev/null 2>&1 &
    say "opened the diagnostic flowchart in the default browser"
  elif command -v open >/dev/null 2>&1; then
    open "$flow"
  else
    say "open this file in a browser: $flow"
  fi
}

mode_man() {
  local manual="$REPO_DIR/docs/man/idaptik-launcher.1"
  [ -r "$manual" ] || die "manual page is missing: $manual"
  if command -v man >/dev/null 2>&1 && [ -t 1 ]; then
    man "$manual"
  elif command -v groff >/dev/null 2>&1; then
    groff -man -Tascii "$manual"
  else
    say "manual source: $manual"
    sed -n '1,260p' "$manual"
  fi
}

desktop_tools_dir() {
  local candidate
  for candidate in \
    "${HP_DESKTOP_TOOLS:-}" \
    "${HP_ESTATE_ROOT:+$HP_ESTATE_ROOT/.desktop-tools}" \
    "${XDG_DATA_HOME:-$HOME/.local/share}/hyperpolymath/.desktop-tools" \
    "/var/mnt/eclipse/repos/.desktop-tools" \
    "$HOME/developer/repos/.desktop-tools" \
    "$HOME/dev/repos/.desktop-tools"
  do
    [ -n "$candidate" ] && [ -d "$candidate" ] && { printf '%s' "$candidate"; return 0; }
  done
  return 1
}

# The standard's shared helper makes failures visible when this script was
# started from a desktop icon. It always writes stderr too, and gracefully
# disappears when the estate desktop tools are not installed.
DESKTOP_TOOLS="$(desktop_tools_dir || true)"
if [ -n "$DESKTOP_TOOLS" ] && [ -r "$DESKTOP_TOOLS/gui-error.sh" ]; then
  # shellcheck disable=SC1091
  . "$DESKTOP_TOOLS/gui-error.sh"
fi

integration_paths() {
  case "$(platform)" in
    linux-*)
      INTEG_PLATFORM="linux"
      APPS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
      DESKTOP_DIR="$HOME/Desktop"
      BIN_DIR="$HOME/.local/bin"
      LAUNCHER_TARGET="$BIN_DIR/idaptik-launcher"
      APP_TARGET="$APPS_DIR/idaptik.desktop"
      DESKTOP_TARGET="$DESKTOP_DIR/idaptik.desktop"
      ;;
    macos-*)
      INTEG_PLATFORM="macos"
      APPS_DIR="$HOME/Applications"
      DESKTOP_DIR="$HOME/Desktop"
      BIN_DIR="$HOME/.local/bin"
      LAUNCHER_TARGET="$BIN_DIR/idaptik-launcher"
      APP_TARGET="$APPS_DIR/$APP_DISPLAY.app"
      DESKTOP_TARGET="$DESKTOP_DIR/$APP_DISPLAY.command"
      ;;
    windows-*)
      INTEG_PLATFORM="windows"
      APPS_DIR="${APPDATA:-$HOME/AppData/Roaming}/Microsoft/Windows/Start Menu/Programs"
      DESKTOP_DIR="$HOME/Desktop"
      BIN_DIR="$HOME/.local/bin"
      LAUNCHER_TARGET="$BIN_DIR/idaptik-launcher.sh"
      APP_TARGET="$APPS_DIR/$APP_DISPLAY.lnk"
      DESKTOP_TARGET="$DESKTOP_DIR/$APP_DISPLAY.lnk"
      ;;
    *) die "unsupported integration platform: $(platform)" ;;
  esac
}

write_linux_desktop() {
  local target="$1" keepopen="$2"
  local gui_cmd="$LAUNCHER_TARGET --auto"
  cat > "$target" <<EOF
[Desktop Entry]
Type=Application
Version=1.0
Name=$APP_DISPLAY
Comment=$APP_DESC
Exec=$keepopen "$APP_DISPLAY" "$REPO_DIR" "$gui_cmd" "" "$LOG_FILE"
Icon=applications-games
Terminal=true
Categories=$APP_CATEGORIES
StartupNotify=true
Actions=stop;status;

[Desktop Action stop]
Name=Stop IDApTIK
Exec=$LAUNCHER_TARGET --stop

[Desktop Action status]
Name=IDApTIK Status
Exec=$LAUNCHER_TARGET --status
EOF
  chmod 444 "$target"
}

mode_integ() {
  require_repo
  integration_paths
  local force="${1:-}" tools keepopen verifier
  tools="$(desktop_tools_dir)" || die "desktop tools not found; set HP_DESKTOP_TOOLS to the standards launcher helper directory"
  keepopen="$tools/keepopen.sh"
  [ -x "$keepopen" ] || die "required fallback wrapper is missing: $keepopen"

  if { [ -e "$APP_TARGET" ] || [ -e "$LAUNCHER_TARGET" ]; } && [ "$force" != "--force" ]; then
    if [ -t 0 ]; then
      local answer
      read -rp "$APP_DISPLAY is already integrated. Reinstall? [y/N] " answer
      case "$answer" in y|Y|yes|YES) ;; *) say "nothing changed"; return 0 ;; esac
    else
      die "already integrated; rerun with --integ --force"
    fi
  fi

  mkdir -p "$APPS_DIR" "$DESKTOP_DIR" "$BIN_DIR" "$CONFIG_DIR"
  printf '%s\n' "$REPO_DIR" > "$REPO_PATH_FILE"
  cp "$REPO_DIR/launcher.sh" "$LAUNCHER_TARGET"
  chmod 755 "$LAUNCHER_TARGET"

  case "$INTEG_PLATFORM" in
    linux)
      [ -e "$APP_TARGET" ] && chmod u+w "$APP_TARGET"
      [ -e "$DESKTOP_TARGET" ] && chmod u+w "$DESKTOP_TARGET"
      write_linux_desktop "$APP_TARGET" "$keepopen"
      write_linux_desktop "$DESKTOP_TARGET" "$keepopen"
      command -v gio >/dev/null 2>&1 && gio set "$DESKTOP_TARGET" metadata::trusted true >/dev/null 2>&1 || true
      command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APPS_DIR" >/dev/null 2>&1 || true
      ;;
    macos)
      mkdir -p "$APP_TARGET/Contents/MacOS"
      cat > "$APP_TARGET/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleName</key><string>$APP_DISPLAY</string>
<key>CFBundleIdentifier</key><string>org.hyperpolymath.idaptik</string>
<key>CFBundleExecutable</key><string>idaptik</string>
</dict></plist>
EOF
      cat > "$APP_TARGET/Contents/MacOS/idaptik" <<EOF
#!/usr/bin/env bash
exec "$keepopen" "$APP_DISPLAY" "$REPO_DIR" "$LAUNCHER_TARGET --auto" "" "$LOG_FILE"
EOF
      chmod 755 "$APP_TARGET/Contents/MacOS/idaptik"
      cp "$APP_TARGET/Contents/MacOS/idaptik" "$DESKTOP_TARGET"
      chmod 755 "$DESKTOP_TARGET"
      ;;
    windows)
      if command -v powershell.exe >/dev/null 2>&1; then
        powershell.exe -NoProfile -NonInteractive -Command \
          "\$w=New-Object -ComObject WScript.Shell; \$s=\$w.CreateShortcut('$APP_TARGET'); \$s.TargetPath='bash.exe'; \$s.Arguments='$keepopen \"$APP_DISPLAY\" \"$REPO_DIR\" \"$LAUNCHER_TARGET --auto\" \"\" \"$LOG_FILE\"'; \$s.Save(); \$d=\$w.CreateShortcut('$DESKTOP_TARGET'); \$d.TargetPath=\$s.TargetPath; \$d.Arguments=\$s.Arguments; \$d.Save()"
      else
        printf '@echo off\nbash.exe "%s" "%s" "%s" "%s --auto" "" "%s"\n' \
          "$keepopen" "$APP_DISPLAY" "$REPO_DIR" "$LAUNCHER_TARGET" "$LOG_FILE" \
          > "${APP_TARGET%.lnk}.bat"
        cp "${APP_TARGET%.lnk}.bat" "${DESKTOP_TARGET%.lnk}.bat"
      fi
      ;;
  esac

  verifier="$tools/verify-desktop-integrity.sh"
  if [ -x "$verifier" ]; then
    "$verifier" --generate >/dev/null 2>&1 || warn "desktop integrity verification failed"
  else
    warn "optional desktop integrity verifier not found at $verifier"
  fi
  say "integrated $APP_DISPLAY for $INTEG_PLATFORM; remove with $LAUNCHER_TARGET --disinteg"
}

mode_disinteg() {
  integration_paths
  stop_all || true
  local target
  for target in "$APP_TARGET" "$DESKTOP_TARGET" "${APP_TARGET%.lnk}.bat" "${DESKTOP_TARGET%.lnk}.bat"; do
    [ -e "$target" ] || [ -L "$target" ] || continue
    [ -f "$target" ] && chmod u+w "$target" 2>/dev/null || true
    if [ -d "$target" ] && [ ! -L "$target" ]; then
      rm -rf -- "$target"
    else
      rm -f -- "$target"
    fi
    say "removed $target"
  done
  rm -f -- "$LAUNCHER_TARGET" "$PID_FILE"
  say "integration removed; configuration in $CONFIG_DIR and logs in $STATE_DIR were preserved"
}

show_help() {
  cat <<EOF
$APP_DISPLAY launcher — GUI entry point for solo and multiplayer play

Usage: $0 [MODE] [OPTIONS]

Player modes:
  --auto, --browser, --web  Open the interactive launch menu (default)
  --solo                    Launch the solo Bevy GUI (controls both roles)
  --host [--role ROLE]      Start the relay and host in the Bevy GUI
  --join HOST [--role ROLE] Join HOST in the Bevy GUI
                            ROLE is infiltrator or hacker

Client selection (WSL2):
  --windows-client          Build and run the native Windows Bevy client
                            instead of the WSLg one. Combine with --solo,
                            --host, --join or --auto. This bypasses WSLg
                            entirely; the relay stays in WSL. Windows interop
                            is discovered at /mnt/c/Windows/System32, so
                            cmd.exe does not need to be on PATH.
                            Prerequisites: run 'just windows-client-deps'.
  (automatic)               WSLg Copy Mode failure already selects the
                            Windows client when interop is available.

Renderer evidence:
  After a solo launch the launcher prints the adapter Bevy selected
  (for example: renderer: NVIDIA GeForce RTX 4070 Laptop GPU (Vulkan)),
  and warns loudly when it is a software adapter (llvmpipe/softpipe/WARP).
  --status reports the same line from the last run's log.

Readiness choreography:
  A host may invite the other player only after ● READY FOR JOINERS.
  A joiner starts Bevy only after its build and exact relay probe pass.
  ✓ means passed, ! means non-blocking degradation, ✗ means stop/no launch.

Diagnostics and recovery:
  --doctor                  Machine/OS/WSL, coprocessor, dependency, audio,
                            Tailscale, route, and display health report
  --repair                  Install pinned dependencies; guide Tailscale/WSL repair
  --flowdiags               Open the friendly HTML troubleshooting flowchart
  --man                     Display the complete section-1 manual

Standard lifecycle modes:
  --start                   Launch the solo Bevy GUI without opening the menu
  --stop                    Stop the game and a relay started by this project
  --status                  Report game, relay, binary, and log status
  --integ [--force]         Install desktop/start-menu integration
  --disinteg                Remove installed integration; preserve config/logs
  --help, -h                Show this help and the files read/written
  --version, -V             Print launcher version/build/platform

Two-machine example:
  Host:    $0 --host --role infiltrator
           Wait for ● READY FOR JOINERS, then share its tailnet address.
  Joiner:  $0 --join HOST --role hacker
           Run only after the host is green; wait for ● READY — JOINING HOST NOW.

Recovery example:
  $0 --doctor
  $0 --repair
  $0 --flowdiags

Environment:
  IDAPTIK_PORT              Burble relay port (current: $PORT; default: 4000)
  IDAPTIK_BURBLE_DIR        Burble checkout (default: sibling ../burble)
  IDAPTIK_REPO_DIR          Override repository location for an installed copy
  IDAPTIK_LAUNCHER_DRY_RUN  Set to 1 to print the selected launch command
  IDAPTIK_STARTUP_TIMEOUT   Readiness wait in seconds (default: 1800)

Files:
  reads  $REPO_PATH_FILE
  writes $PID_FILE
         $LOG_FILE
         $PROGRESS_FILE
         $READY_FILE

Detected platform: $(platform)
Repository: $REPO_DIR
EOF
}

parse_role() {
  local role="${1:-infiltrator}"
  case "$role" in
    infiltrator|hacker) printf '%s' "$role" ;;
    *) die "role must be 'infiltrator' or 'hacker', got '$role'" ;;
  esac
}

# `--windows-client` is a modifier, not a mode: strip it wherever it appears so
# it composes with --solo, --host, --join and --auto. Kept as a flag rather than
# a documented environment variable because the working WSL2 route used to
# require exporting IDAPTIK_CLIENT_PLATFORM=windows by hand.
ARGS=()
for arg in "$@"; do
  case "$arg" in
    --windows-client|--wsl-windows) export IDAPTIK_CLIENT_PLATFORM=windows ;;
    *) ARGS+=("$arg") ;;
  esac
done
if [ "${#ARGS[@]}" -gt 0 ]; then set -- "${ARGS[@]}"; else set --; fi

MODE="${1:---auto}"
[ $# -gt 0 ] && shift || true

case "$MODE" in
  __run-solo) run_solo ;;
  --start|--solo) start_solo ;;
  --host)
    role="infiltrator"
    while [ $# -gt 0 ]; do
      case "$1" in
        --role) [ $# -ge 2 ] || die "--role needs a value"; role="$(parse_role "$2")"; shift 2 ;;
        *) die "unknown --host option: $1" ;;
      esac
    done
    start_host "$role"
    ;;
  --join)
    [ $# -ge 1 ] || die "--join needs HOST"
    host="$1"; shift
    role="hacker"
    while [ $# -gt 0 ]; do
      case "$1" in
        --role) [ $# -ge 2 ] || die "--role needs a value"; role="$(parse_role "$2")"; shift 2 ;;
        *) die "unknown --join option: $1" ;;
      esac
    done
    start_join "$host" "$role"
    ;;
  --auto|--browser|--web) menu ;;
  --stop) [ $# -eq 0 ] || die "--stop takes no arguments"; stop_all ;;
  --status) [ $# -eq 0 ] || die "--status takes no arguments"; mode_status ;;
  --doctor) [ $# -eq 0 ] || die "--doctor takes no arguments"; "$RUNTIME_DOCTOR" report all ;;
  --repair) [ $# -eq 0 ] || die "--repair takes no arguments"; "$RUNTIME_DOCTOR" repair all ;;
  --flowdiags) [ $# -eq 0 ] || die "--flowdiags takes no arguments"; mode_flowdiags ;;
  --man) [ $# -eq 0 ] || die "--man takes no arguments"; mode_man ;;
  --integ) [ $# -le 1 ] || die "usage: --integ [--force]"; mode_integ "${1:-}" ;;
  --disinteg) [ $# -eq 0 ] || die "--disinteg takes no arguments"; mode_disinteg ;;
  --version|-V) printf '%s %s (%s) [%s]\n' "$APP_NAME-launcher" "$VERSION" "$(build_sha)" "$(platform)" ;;
  --help|-h) show_help ;;
  *) die "unknown mode: $MODE (try --help)" ;;
esac
