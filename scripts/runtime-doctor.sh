#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
#
# Player-facing runtime diagnostics. Unlike scripts/doctor.sh, which audits
# every development toolchain, this checks only what a requested game mode
# needs and gives actionable remediation before any game/relay process starts.

set -uo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Windows interop discovery is shared with the launcher and the installer.
# Sourcing it is what stops the doctor from claiming a working WSL2 machine has
# no interop merely because cmd.exe is not on PATH.
if [ -r "$REPO_DIR/scripts/windows-interop.sh" ]; then
  # shellcheck source=scripts/windows-interop.sh
  . "$REPO_DIR/scripts/windows-interop.sh"
fi

MODE="${1:-report}"
[ $# -gt 0 ] && shift || true
PROFILE="${1:-all}"
[ $# -gt 0 ] && shift || true
TARGET="${1:-}"
[ $# -gt 0 ] && shift || true

FAILURES=0
WARNINGS=0
QUIET=0
[ "$MODE" = "preflight-quiet" ] && { MODE="preflight"; QUIET=1; }

pass() { [ "$QUIET" -eq 1 ] || printf '  ✓ [PASS] %-20s %s\n' "$1" "$2"; }
info() { [ "$QUIET" -eq 1 ] || printf '  · [INFO] %-20s %s\n' "$1" "$2"; }
warn() { WARNINGS=$((WARNINGS + 1)); [ "$QUIET" -eq 1 ] || printf '  ! [WARN] %-20s %s\n' "$1" "$2"; }
fail() { FAILURES=$((FAILURES + 1)); printf '  ✗ [FAIL] %-20s %s\n' "$1" "$2" >&2; }

is_wsl() { hp_is_wsl; }

windows_interop_available() { hp_windows_interop; }

# The adapter Bevy chose on the last run, read from its own log. glxinfo below
# reports the *OpenGL* renderer, which is not necessarily what wgpu picked —
# the observed failure this machine hit was WSLg Vulkan on llvmpipe while the
# GL probe reported D3D12/NVIDIA. Report both rather than conflating them.
game_log="${IDAPTIK_DIAG_GAME_LOG:-${XDG_STATE_HOME:-$HOME/.local/state}/idaptik/game.log}"

check_bevy_renderer() {
  local line name backend
  if [ ! -r "$game_log" ]; then
    info "bevy renderer" "no client log yet at $game_log"
    return
  fi
  line="$(grep -a -m1 'AdapterInfo' "$game_log" 2>/dev/null || true)"
  if [ -z "$line" ]; then
    info "bevy renderer" "the last client run logged no adapter line (see $game_log)"
    return
  fi
  name="$(sed -n 's/.*name: *"\([^"]*\)".*/\1/p' <<<"$line" | head -1)"
  backend="$(sed -n 's/.*backend: *\([A-Za-z0-9_-]*\).*/\1/p' <<<"$line" | head -1)"
  case "$name" in
    *llvmpipe*|*softpipe*|*swiftshader*|*SwiftShader*|*WARP*|*"Basic Render"*)
      warn "bevy renderer" "${name}${backend:+ ($backend)} — software rendering; the GPU was not selected" ;;
    '') info "bevy renderer" "$line" ;;
    *) pass "bevy renderer" "$name${backend:+ ($backend)}" ;;
  esac
}

display_value="${IDAPTIK_DIAG_DISPLAY-${DISPLAY:-}}"
wayland_value="${IDAPTIK_DIAG_WAYLAND_DISPLAY-${WAYLAND_DISPLAY:-}}"
wslg_dir="${IDAPTIK_DIAG_WSLG_DIR:-/mnt/wslg}"
shared_memory="${IDAPTIK_DIAG_SHARED_MEMORY:-/mnt/shared_memory}"
weston_log="${IDAPTIK_DIAG_WESTON_LOG:-$wslg_dir/weston.log}"
pulse_server="${IDAPTIK_DIAG_PULSE_SERVER-${PULSE_SERVER:-}}"
client_platform="${IDAPTIK_CLIENT_PLATFORM:-linux}"

check_repository() {
  if [ -f "$REPO_DIR/Cargo.toml" ] && [ -f "$REPO_DIR/mise.toml" ]; then
    pass repository "$REPO_DIR"
  else
    fail repository "IDApTIK source/configuration is incomplete at $REPO_DIR; pull a complete checkout"
  fi
}

check_machine() {
  local os kernel arch cpu cores memory
  if [ -r /etc/os-release ]; then
    # shellcheck disable=SC1091
    . /etc/os-release
    os="${PRETTY_NAME:-Linux}"
  else
    os="$(uname -s)"
  fi
  kernel="$(uname -r 2>/dev/null || printf unknown)"
  arch="$(uname -m 2>/dev/null || printf unknown)"
  cpu="$(awk -F: '/model name/{sub(/^[[:space:]]+/, "", $2); print $2; exit}' /proc/cpuinfo 2>/dev/null || true)"
  cores="$(getconf _NPROCESSORS_ONLN 2>/dev/null || printf unknown)"
  memory="$(awk '/MemTotal:/{printf "%.1f GiB", $2/1048576}' /proc/meminfo 2>/dev/null || true)"
  info operating-system "$os"
  info kernel "$kernel"
  info architecture "$arch"
  info processor "${cpu:-unknown} · $cores logical cores · ${memory:-memory unknown}"
  if is_wsl; then
    info virtualization "WSL2 detected"
    if [ -r "$wslg_dir/versions.txt" ]; then
      info wslg-version "$(tr '\n' ' ' < "$wslg_dir/versions.txt" | sed 's/[[:space:]]\+/ /g')"
    fi
  else
    info virtualization "native/non-WSL environment"
  fi
}

check_tools() {
  [ "${IDAPTIK_DIAG_SKIP_TOOLS:-0}" = "1" ] && { info dependencies "fixture check skipped"; return; }

  if ! command -v mise >/dev/null 2>&1 && [ ! -x "$HOME/.local/bin/mise" ]; then
    fail mise "missing; install mise, then run './launcher.sh --repair'"
    return
  fi

  local mise_bin
  mise_bin="$(command -v mise 2>/dev/null || printf '%s' "$HOME/.local/bin/mise")"
  pass mise "$($mise_bin --version 2>/dev/null | head -1 || printf 'installed')"

  if (cd "$REPO_DIR" && "$mise_bin" exec -- cargo --version >/dev/null 2>&1); then
    pass rust "pinned Cargo toolchain resolves"
  else
    fail rust "pinned toolchain is missing; run './launcher.sh --repair'"
  fi

  if [ "$client_platform" = "windows" ]; then
    # rustup usually resolves on PATH, but under mise the pinned toolchain may
    # only be reachable through `mise exec`; check both before declaring a
    # 6-minute cross-build impossible.
    local installed=""
    if command -v rustup >/dev/null 2>&1; then
      installed="$(rustup target list --installed 2>/dev/null || true)"
    else
      installed="$(cd "$REPO_DIR" && "$mise_bin" exec -- rustup target list --installed 2>/dev/null || true)"
    fi
    if grep -qx 'x86_64-pc-windows-gnu' <<<"$installed"; then
      pass "Windows target" "x86_64-pc-windows-gnu is installed"
    else
      fail "Windows target" "missing; run 'mise exec -- rustup target add x86_64-pc-windows-gnu' (or: just windows-client-deps)"
    fi
    if command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
      pass "Windows linker" "MinGW cross-linker resolves"
    else
      fail "Windows linker" "x86_64-w64-mingw32-gcc is missing; run 'just windows-client-deps' (installs gcc-mingw-w64-x86-64)"
    fi
  fi

  if [ "$PROFILE" = "host" ] || [ "$PROFILE" = "all" ]; then
    if (cd "$REPO_DIR" && "$mise_bin" exec -- mix --version >/dev/null 2>&1); then
      pass elixir "pinned Mix/OTP toolchain resolves"
    else
      fail elixir "host relay toolchain is missing; run './launcher.sh --repair'"
    fi
    if command -v curl >/dev/null 2>&1; then
      pass curl "relay readiness probe available"
    else
      fail curl "required for hosting; install curl with the operating-system package manager"
    fi
  fi
}

check_display() {
  case "$(uname -s 2>/dev/null || printf unknown)" in
    Linux*) ;;
    *) info display "native window diagnostics are currently Linux/WSLg-specific"; return ;;
  esac

  if [ "$client_platform" = "windows" ] && is_wsl; then
    if windows_interop_available; then
      pass display "native Windows Bevy client selected; WSLg is bypassed"
      info wslg "Copy Mode does not affect the Windows-native client"
    else
      fail "Windows interop" "cannot start the Windows client: $(hp_windows_interop_reason)"
    fi
    check_bevy_renderer
    return
  fi

  if [ -n "$display_value" ] || [ -n "$wayland_value" ]; then
    pass display "GUI endpoint advertised (${wayland_value:-$display_value})"
  else
    fail display "neither DISPLAY nor WAYLAND_DISPLAY is set; no graphical window can open"
  fi

  if ! is_wsl; then
    info wslg "not running under WSL; Copy Mode check does not apply"
    return
  fi

  if [ -d "$wslg_dir" ]; then
    pass wslg "WSLg integration directory is present"
  else
    fail wslg "integration directory $wslg_dir is missing"
    return
  fi

  if [ -r "$weston_log" ] && grep -qE 'rdp_allocate_shared_memory: Failed to open .*error: (Input/output error|No such file)' "$weston_log"; then
    fail "WSLg Copy Mode" "shared-memory initialization failed; Windows may show an unusable penguin taskbar item"
    printf '%s\n' \
      "         Remediation (Windows PowerShell as Administrator):" \
      "           wsl --update" \
      "           wsl --shutdown" \
      "         Reopen the distribution; if Copy Mode remains, restart Windows." >&2
  elif [ ! -d "$shared_memory" ]; then
    fail "WSLg shared memory" "$shared_memory is missing; refusing to create a misleading blank/Copy-Mode game window"
    printf '%s\n' \
      "         Remediation (Windows PowerShell as Administrator):" \
      "           wsl --update" \
      "           wsl --shutdown" \
      "         Do not manually create the directory; WSLg must mount it." >&2
  else
    pass "WSLg shared memory" "$shared_memory is mounted"
  fi

  if [ -r "$wslg_dir/stderr.log" ] && grep -q 'falling back to sw' "$wslg_dir/stderr.log"; then
    warn graphics "WSLg fell back to software rendering; update the Windows GPU driver if performance is poor"
  else
    pass graphics "WSLg reports no software-renderer fallback"
  fi

  if [ -e /dev/dxg ]; then
    pass "graphics bridge" "WSL DirectX GPU bridge /dev/dxg is present"
  else
    warn "graphics bridge" "/dev/dxg is not visible; Bevy may use CPU rendering"
  fi

  if command -v glxinfo >/dev/null 2>&1; then
    local renderer
    renderer="$(glxinfo -B 2>/dev/null | sed -n 's/^[[:space:]]*OpenGL renderer string:[[:space:]]*//p' | head -1 || true)"
    case "$renderer" in
      *llvmpipe*|*softpipe*) warn "graphics coprocessor" "${renderer:-software renderer} (OpenGL); playable but potentially slow" ;;
      '') warn "graphics coprocessor" "OpenGL renderer probe failed" ;;
      *) pass "graphics coprocessor" "$renderer (OpenGL)" ;;
    esac
  else
    info "graphics coprocessor" "glxinfo unavailable; Bevy will perform its own adapter selection"
  fi
  check_bevy_renderer

  case "$pulse_server" in
    unix:*)
      local pulse_path="${pulse_server#unix:}"
      if [ -S "$pulse_path" ] && { [ -e /dev/snd ] || [ -r /etc/asound.conf ] || [ -r "$HOME/.asoundrc" ]; }; then
        pass audio "WSLg endpoint and an ALSA route are present"
      elif [ -S "$pulse_path" ]; then
        warn audio "WSLg PulseAudio exists but no ALSA device/route is configured; Bevy will run without sound"
      else
        warn audio "PulseAudio endpoint $pulse_path is unavailable; the game will run without sound"
      fi
      ;;
    '') warn audio "PULSE_SERVER is unset; the game may run without sound" ;;
    *) info audio "audio endpoint is configured" ;;
  esac

  info "extended coprocessors" "QPU/APU/IOPU/vector/physics/math/tensor/neural/crypto suite is not required by this release"
}

check_network() {
  [ "$PROFILE" = "host" ] || [ "$PROFILE" = "join" ] || [ "$PROFILE" = "all" ] || return

  if [ "$PROFILE" = "host" ] || [ "$PROFILE" = "all" ]; then
    local lan=""
    lan="${IDAPTIK_DIAG_LAN_ADDRESS:-$(ip -4 route get 1.1.1.1 2>/dev/null | grep -oP 'src \K[0-9.]+' | head -1 || true)}"
    if [ -z "$lan" ]; then
      fail reachability "no usable IPv4 route was found; connect the machine to a network before hosting"
    elif is_wsl; then
      case "$lan" in
        172.1[6-9].*|172.2[0-9].*|172.3[01].*)
          if ! command -v tailscale >/dev/null 2>&1; then
            fail tailscale "required behind WSL2 NAT but not installed; select Diagnostics → safe repair"
          elif [ -z "$(tailscale ip -4 2>/dev/null | head -1 || true)" ]; then
            fail tailscale "installed but not connected/authenticated; run './launcher.sh --repair'"
          else
            pass tailscale "connected as $(tailscale ip -4 2>/dev/null | head -1)"
            pass reachability "WSL2 NAT is bypassed by the active tailnet route"
          fi
          ;;
        *) pass reachability "host address $lan is not a WSL2 NAT address" ;;
      esac
    else
      pass reachability "host has an IPv4 route via $lan"
    fi
  fi

  if [ "$PROFILE" = "join" ]; then
    if [ -z "$TARGET" ]; then
      fail "host target" "no host address was supplied"
      return
    fi
    local authority host port scheme
    authority="${TARGET#ws://}"
    authority="${authority#wss://}"
    authority="${authority%%/*}"
    host="${authority%%:*}"
    if [ "$authority" = "$host" ]; then port="${IDAPTIK_PORT:-4000}"; else port="${authority##*:}"; fi
    case "$TARGET" in wss://*) scheme=https ;; *) scheme=http ;; esac
    if command -v curl >/dev/null 2>&1 && curl -fsS --connect-timeout 4 --max-time 6 "$scheme://$host:$port/" >/dev/null 2>&1; then
      pass "host relay" "$host:$port is answering; the host has reached READY FOR JOINERS"
    else
      fail "host relay" "$host:$port is not reachable; wait for the host's green READY FOR JOINERS message"
    fi
  fi
}

run_checks() {
  [ "$QUIET" -eq 1 ] || printf 'IDApTIK runtime diagnostics (%s):\n' "$PROFILE"
  check_machine
  check_repository
  check_tools
  check_display
  check_network

  if [ "$FAILURES" -gt 0 ]; then
    printf '\n%d blocking fault(s), %d warning(s). Nothing was launched.\n' "$FAILURES" "$WARNINGS" >&2
    return 1
  fi
  [ "$QUIET" -eq 1 ] || printf '\n\033[1;32m● READY TO LAUNCH\033[0m with %d non-blocking warning(s).\n' "$WARNINGS"
}

repair() {
  printf 'IDApTIK safe repair:\n'
  local mise_bin
  if command -v mise >/dev/null 2>&1; then
    mise_bin="$(command -v mise)"
  elif [ -x "$HOME/.local/bin/mise" ]; then
    mise_bin="$HOME/.local/bin/mise"
  else
    fail mise "cannot self-install without downloading executable code; install from https://mise.jdx.dev"
    return 1
  fi

  if (cd "$REPO_DIR" && "$mise_bin" trust -q && "$mise_bin" install); then
    pass dependencies "pinned repository toolchains installed"
  else
    fail dependencies "mise could not install all pinned toolchains; inspect its output above"
  fi

  if is_wsl && { [ ! -d "$shared_memory" ] || { [ -r "$weston_log" ] && grep -qE 'rdp_allocate_shared_memory: Failed to open' "$weston_log"; }; }; then
    warn "WSLg Copy Mode" "host-level recovery cannot be applied from inside WSL without terminating this process"
    printf '%s\n' \
      "  Run in Windows PowerShell as Administrator:" \
      "    wsl --update" \
      "    wsl --shutdown" \
      "  Then reopen Debian and run './launcher.sh --doctor'."
  fi

  if is_wsl; then
    local lan=""
    lan="${IDAPTIK_DIAG_LAN_ADDRESS:-$(ip -4 route get 1.1.1.1 2>/dev/null | grep -oP 'src \K[0-9.]+' | head -1 || true)}"
    case "$lan" in
      172.1[6-9].*|172.2[0-9].*|172.3[01].*)
        if ! command -v tailscale >/dev/null 2>&1; then
          if [ -t 0 ]; then
            local answer installer
            read -rp 'Tailscale is required to host through WSL2 NAT. Install it from tailscale.com now? [y/N]: ' answer
            case "$answer" in
              y|Y|yes|YES)
                installer="$(mktemp)"
                if curl -fsSL https://tailscale.com/install.sh -o "$installer" && sudo sh "$installer"; then
                  pass tailscale "installed from the official Tailscale installer"
                else
                  fail tailscale "installation failed; see https://tailscale.com/download/linux"
                fi
                rm -f -- "$installer"
                ;;
              *) warn tailscale "not installed; hosting remains unavailable behind WSL2 NAT" ;;
            esac
          else
            warn tailscale "not installed; rerun './launcher.sh --repair' interactively to bootstrap it"
          fi
        fi
        if command -v tailscale >/dev/null 2>&1 && [ -z "$(tailscale ip -4 2>/dev/null | head -1 || true)" ]; then
          if [ -t 0 ]; then
            info tailscale "starting authentication; follow the URL Tailscale prints"
            if sudo tailscale up; then
              pass tailscale "connected as $(tailscale ip -4 2>/dev/null | head -1)"
            else
              fail tailscale "could not authenticate/start the tailnet connection"
            fi
          else
            warn tailscale "installed but disconnected; run 'sudo tailscale up'"
          fi
        fi
        ;;
    esac
  fi

  [ "$FAILURES" -eq 0 ]
}

case "$MODE" in
  report|preflight) run_checks ;;
  repair) repair ;;
  *) printf 'usage: %s [report|preflight|preflight-quiet|repair] [solo|host|join|all]\n' "$0" >&2; exit 2 ;;
esac
