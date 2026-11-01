#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DOCTOR="$REPO_DIR/scripts/runtime-doctor.sh"
FIXTURE="$(mktemp -d)"
trap 'rm -rf "$FIXTURE"' EXIT

run_fixture() {
  env \
    IDAPTIK_DIAG_IS_WSL=1 \
    IDAPTIK_DIAG_SKIP_TOOLS=1 \
    IDAPTIK_DIAG_DISPLAY=:0 \
    IDAPTIK_DIAG_WAYLAND_DISPLAY=wayland-0 \
    IDAPTIK_DIAG_WSLG_DIR="$FIXTURE/wslg" \
    IDAPTIK_DIAG_SHARED_MEMORY="$FIXTURE/shared_memory" \
    IDAPTIK_DIAG_WESTON_LOG="$FIXTURE/wslg/weston.log" \
    IDAPTIK_DIAG_PULSE_SERVER=unix:"$FIXTURE/wslg/PulseServer" \
    "$DOCTOR" "$@"
}

mkdir -p "$FIXTURE/wslg" "$FIXTURE/shared_memory"
: > "$FIXTURE/wslg/weston.log"

healthy="$(run_fixture report solo 2>&1)"
grep -q '\[PASS\].*WSLg shared memory' <<<"$healthy"
grep -q 'READY TO LAUNCH' <<<"$healthy"

rmdir "$FIXTURE/shared_memory"
if missing="$(run_fixture preflight host 2>&1)"; then
  printf 'expected missing shared memory to block host preflight\n' >&2
  exit 1
fi
grep -q '\[FAIL\].*WSLg shared memory' <<<"$missing"
grep -q 'Nothing was launched' <<<"$missing"
grep -q 'wsl --shutdown' <<<"$missing"

mkdir -p "$FIXTURE/shared_memory"
printf '%s\n' 'rdp_allocate_shared_memory: Failed to open "/mnt/shared_memory/id" with error: Input/output error' > "$FIXTURE/wslg/weston.log"
if copy_mode="$(run_fixture preflight join 2>&1)"; then
  printf 'expected Weston EIO to block join preflight\n' >&2
  exit 1
fi
grep -q '\[FAIL\].*WSLg Copy Mode' <<<"$copy_mode"
grep -q 'restart Windows' <<<"$copy_mode"

windows_fallback="$(env \
  IDAPTIK_CLIENT_PLATFORM=windows \
  IDAPTIK_DIAG_WINDOWS_INTEROP=1 \
  IDAPTIK_DIAG_IS_WSL=1 \
  IDAPTIK_DIAG_SKIP_TOOLS=1 \
  IDAPTIK_DIAG_DISPLAY=:0 \
  IDAPTIK_DIAG_WSLG_DIR="$FIXTURE/wslg" \
  IDAPTIK_DIAG_SHARED_MEMORY="$FIXTURE/missing_shared_memory" \
  IDAPTIK_DIAG_WESTON_LOG="$FIXTURE/wslg/weston.log" \
  "$DOCTOR" report solo 2>&1 || true)"
grep -q '\[PASS\].*native Windows Bevy client selected' <<<"$windows_fallback"
grep -q 'WSLg is bypassed' <<<"$windows_fallback"
grep -q 'READY TO LAUNCH' <<<"$windows_fallback"

fallback_dry_run="$(env \
  IDAPTIK_DIAG_IS_WSL=1 \
  IDAPTIK_DIAG_WINDOWS_INTEROP=1 \
  IDAPTIK_DIAG_SKIP_TOOLS=1 \
  IDAPTIK_DIAG_DISPLAY=:0 \
  IDAPTIK_DIAG_WSLG_DIR="$FIXTURE/wslg" \
  IDAPTIK_DIAG_SHARED_MEMORY="$FIXTURE/missing_shared_memory" \
  IDAPTIK_DIAG_WESTON_LOG="$FIXTURE/wslg/weston.log" \
  IDAPTIK_LAUNCHER_DRY_RUN=1 \
  XDG_STATE_HOME="$FIXTURE/fallback-state" \
  XDG_RUNTIME_DIR="$FIXTURE/fallback-runtime" \
  "$REPO_DIR/launcher.sh" --solo 2>&1)"
grep -q 'using the native Windows Bevy client instead' <<<"$fallback_dry_run"
grep -q '\[PASS\].*native Windows Bevy client selected' <<<"$fallback_dry_run"
grep -q 'DRY RUN:.*launcher.sh __run-solo' <<<"$fallback_dry_run"

# Interop is discovered at the Windows system directory, not from PATH: this
# machine's cmd.exe is not on PATH and the launcher still must find it.
WINSYS="$FIXTURE/winsys32"
mkdir -p "$WINSYS"
printf '#!/usr/bin/env bash\nexit 0\n' > "$WINSYS/cmd.exe"
chmod +x "$WINSYS/cmd.exe"
interop_report="$(env \
  IDAPTIK_CLIENT_PLATFORM=windows \
  IDAPTIK_DIAG_WINDOWS_INTEROP= \
  IDAPTIK_WINDOWS_SYSTEM32="$WINSYS" \
  IDAPTIK_DIAG_IS_WSL=1 \
  IDAPTIK_DIAG_SKIP_TOOLS=1 \
  IDAPTIK_DIAG_DISPLAY=:0 \
  IDAPTIK_DIAG_WSLG_DIR="$FIXTURE/wslg" \
  IDAPTIK_DIAG_SHARED_MEMORY="$FIXTURE/shared_memory" \
  IDAPTIK_DIAG_WESTON_LOG="$FIXTURE/wslg/weston.log" \
  "$DOCTOR" report solo 2>&1 || true)"
grep -q '\[PASS\].*native Windows Bevy client selected' <<<"$interop_report" ||
  { printf 'doctor did not discover interop outside PATH:\n%s\n' "$interop_report" >&2; exit 1; }

# The adapter is reported from Bevy's own log, and software rendering is a
# warning rather than a silent success. Reset Weston's log first: the Copy Mode
# fixture above deliberately leaves a failure in it, and this run is about the
# renderer, not the transport.
: > "$FIXTURE/wslg/weston.log"
printf '%s\n' 'INFO bevy_render::renderer: AdapterInfo { name: "llvmpipe (LLVM 19.1.7, 256 bits)", vendor: 0, device: 0, device_type: Cpu, driver: "llvmpipe", driver_info: "Mesa", backend: Vulkan }' \
  > "$FIXTURE/game.log"
software_report="$(env \
  IDAPTIK_DIAG_IS_WSL=1 \
  IDAPTIK_DIAG_SKIP_TOOLS=1 \
  IDAPTIK_DIAG_DISPLAY=:0 \
  IDAPTIK_DIAG_WSLG_DIR="$FIXTURE/wslg" \
  IDAPTIK_DIAG_SHARED_MEMORY="$FIXTURE/shared_memory" \
  IDAPTIK_DIAG_WESTON_LOG="$FIXTURE/wslg/weston.log" \
  IDAPTIK_DIAG_GAME_LOG="$FIXTURE/game.log" \
  "$DOCTOR" report solo 2>&1 || true)"
grep -q '\[WARN\].*bevy renderer.*llvmpipe' <<<"$software_report" ||
  { printf 'doctor did not flag a software adapter:\n%s\n' "$software_report" >&2; exit 1; }

printf '%s\n' 'INFO bevy_render::renderer: AdapterInfo { name: "NVIDIA GeForce RTX 4070 Laptop GPU", vendor: 4318, device: 10191, device_type: DiscreteGpu, driver: "NVIDIA", driver_info: "555.42", backend: Vulkan }' \
  > "$FIXTURE/game.log"
gpu_report="$(env \
  IDAPTIK_DIAG_IS_WSL=1 \
  IDAPTIK_DIAG_SKIP_TOOLS=1 \
  IDAPTIK_DIAG_DISPLAY=:0 \
  IDAPTIK_DIAG_WSLG_DIR="$FIXTURE/wslg" \
  IDAPTIK_DIAG_SHARED_MEMORY="$FIXTURE/shared_memory" \
  IDAPTIK_DIAG_WESTON_LOG="$FIXTURE/wslg/weston.log" \
  IDAPTIK_DIAG_GAME_LOG="$FIXTURE/game.log" \
  "$DOCTOR" report solo 2>&1 || true)"
grep -q '\[PASS\].*bevy renderer.*NVIDIA GeForce RTX 4070 Laptop GPU (Vulkan)' <<<"$gpu_report" ||
  { printf 'doctor did not report the selected GPU:\n%s\n' "$gpu_report" >&2; exit 1; }

healthy_dry_run="$(env \
  IDAPTIK_DIAG_IS_WSL=0 \
  IDAPTIK_DIAG_SKIP_TOOLS=1 \
  IDAPTIK_DIAG_DISPLAY=:0 \
  IDAPTIK_DIAG_LAN_ADDRESS=192.0.2.10 \
  IDAPTIK_LAUNCHER_DRY_RUN=1 \
  XDG_STATE_HOME="$FIXTURE/state" \
  XDG_RUNTIME_DIR="$FIXTURE/runtime" \
  "$REPO_DIR/launcher.sh" --host --role infiltrator 2>&1)"
grep -q 'READY TO LAUNCH' <<<"$healthy_dry_run"
grep -q 'DRY RUN:.*scripts/multiplayer-runtime.sh host --role infiltrator' <<<"$healthy_dry_run"

[ ! -e "$REPO_DIR/idaptik-multiplayer-launcher.sh" ]
if internal_direct="$("$REPO_DIR/scripts/multiplayer-runtime.sh" host 2>&1)"; then
  printf 'expected the internal multiplayer runtime to reject direct use\n' >&2
  exit 1
fi
grep -q 'players must use.*/launcher.sh' <<<"$internal_direct"

if unreachable="$(env \
  IDAPTIK_DIAG_IS_WSL=0 \
  IDAPTIK_DIAG_SKIP_TOOLS=1 \
  IDAPTIK_DIAG_DISPLAY=:0 \
  "$DOCTOR" preflight join 127.0.0.1:1 2>&1)"; then
  printf 'expected an unreachable relay to block join preflight\n' >&2
  exit 1
fi
grep -q '\[FAIL\].*host relay' <<<"$unreachable"
grep -q 'wait for the host' <<<"$unreachable"

printf 'runtime doctor fixtures: PASS\n'
