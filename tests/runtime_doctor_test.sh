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

# Exercise the shared library directly in isolated shells. Paths with spaces
# also check that discovery and the execution probe preserve argument boundaries.
mkdir -p "$FIXTURE/system 32" "$FIXTURE/path-bin" "$FIXTURE/empty"
cat > "$FIXTURE/system 32/cmd.exe" <<'CMD'
#!/bin/sh
printf '%s\n' "$*" >> "$PROBE_LOG"
exit "${PROBE_EXIT:-0}"
CMD
chmod +x "$FIXTURE/system 32/cmd.exe"
(
  unset IDAPTIK_DIAG_WINDOWS_INTEROP IDAPTIK_DIAG_IS_WSL
  . "$REPO_DIR/scripts/windows-interop.sh"
  IDAPTIK_DIAG_PROC_VERSION="$FIXTURE/version"
  printf 'Linux Microsoft WSL2\n' > "$IDAPTIK_DIAG_PROC_VERSION"
  hp_is_wsl
  printf 'Linux native\n' > "$IDAPTIK_DIAG_PROC_VERSION"
  if hp_is_wsl; then echo 'native kernel detected as WSL' >&2; exit 1; fi
  IDAPTIK_DIAG_IS_WSL=1; hp_is_wsl
  IDAPTIK_DIAG_IS_WSL=0
  if hp_is_wsl; then echo 'WSL override ignored' >&2; exit 1; fi
)
(
  unset IDAPTIK_DIAG_WINDOWS_INTEROP
  . "$REPO_DIR/scripts/windows-interop.sh"
  export IDAPTIK_DIAG_IS_WSL=1 IDAPTIK_WINDOWS_SYSTEM32="$FIXTURE/system 32"
  export PROBE_LOG="$FIXTURE/probes" PROBE_EXIT=0
  export PATH="$FIXTURE/path-bin:/usr/bin:/bin"
  : > "$PROBE_LOG"
  hp_windows_system32 > "$FIXTURE/resolved"
  [ "$(cat "$FIXTURE/resolved")" = "$IDAPTIK_WINDOWS_SYSTEM32" ]
  # The successful directory is cached even if the pin subsequently changes.
  IDAPTIK_WINDOWS_SYSTEM32="$FIXTURE/empty"
  [ "$(hp_windows_system32)" = "$FIXTURE/system 32" ]
  hp_windows_interop
  PROBE_EXIT=1
  hp_windows_interop
  [ "$(wc -l < "$PROBE_LOG")" -eq 1 ]
  [ "$(cat "$PROBE_LOG")" = '/c exit 0' ]
  # An explicit verdict overrides even a cached result.
  IDAPTIK_DIAG_WINDOWS_INTEROP=0
  if hp_windows_interop; then echo 'interop override ignored' >&2; exit 1; fi
  IDAPTIK_DIAG_WINDOWS_INTEROP=1; hp_windows_interop
)
(
  unset IDAPTIK_DIAG_WINDOWS_INTEROP
  . "$REPO_DIR/scripts/windows-interop.sh"
  export IDAPTIK_DIAG_IS_WSL=1 IDAPTIK_WINDOWS_SYSTEM32="$FIXTURE/system 32"
  export PROBE_LOG="$FIXTURE/failed-probes" PROBE_EXIT=1
  export PATH="$FIXTURE/path-bin:/usr/bin:/bin"
  : > "$PROBE_LOG"
  if hp_windows_interop; then echo 'failed probe accepted' >&2; exit 1; fi
  PROBE_EXIT=0
  if hp_windows_interop; then echo 'failed probe was not cached' >&2; exit 1; fi
  [ "$(wc -l < "$PROBE_LOG")" -eq 1 ]
  [[ "$(hp_windows_interop_reason)" == *'cmd.exe exists but did not run'* ]]
)
(
  unset IDAPTIK_DIAG_WINDOWS_INTEROP
  . "$REPO_DIR/scripts/windows-interop.sh"
  export IDAPTIK_DIAG_IS_WSL=0 IDAPTIK_WINDOWS_SYSTEM32="$FIXTURE/system 32"
  export PROBE_LOG="$FIXTURE/non-wsl-probes" PROBE_EXIT=0
  export PATH="$FIXTURE/path-bin:/usr/bin:/bin"
  : > "$PROBE_LOG"
  if hp_windows_interop; then echo 'non-WSL interop accepted' >&2; exit 1; fi
  [ ! -s "$PROBE_LOG" ]
)
(
  unset IDAPTIK_DIAG_WINDOWS_INTEROP IDAPTIK_WINDOWS_SYSTEM32
  . "$REPO_DIR/scripts/windows-interop.sh"
  export PATH="$FIXTURE/path-bin:/usr/bin:/bin" WSLPATH_DIR="$FIXTURE/system 32"
  cat > "$FIXTURE/path-bin/wslpath" <<'WSLPATH'
#!/bin/sh
[ "$1" = '-u' ] && [ "$2" = 'C:\Windows\System32' ] || exit 1
printf '%s\n' "$WSLPATH_DIR"
WSLPATH
  chmod +x "$FIXTURE/path-bin/wslpath"
  if [ -f /mnt/c/Windows/System32/cmd.exe ]; then
    [ "$(hp_windows_system32)" = /mnt/c/Windows/System32 ]
  else
    [ "$(hp_windows_system32)" = "$WSLPATH_DIR" ]
    WSLPATH_DIR="$FIXTURE/empty"
    if hp_windows_system32; then echo 'translated directory without cmd.exe accepted' >&2; exit 1; fi
  fi
)
(
  . "$REPO_DIR/scripts/windows-interop.sh"
  export PATH="$FIXTURE/path-bin:/usr/bin:/bin"
  IDAPTIK_WINDOWS_SYSTEM32="$FIXTURE/empty"
  if hp_windows_system32; then echo 'missing pinned cmd.exe accepted' >&2; exit 1; fi
  IDAPTIK_DIAG_IS_WSL=1
  [[ "$(hp_windows_interop_reason)" == *'no cmd.exe'* ]]
  IDAPTIK_DIAG_IS_WSL=0
  [ "$(hp_windows_interop_reason)" = 'this is not a WSL2 session' ]
  cp "$FIXTURE/system 32/cmd.exe" "$FIXTURE/path-bin/cmd.exe"
  # PATH takes precedence for tools even if a system directory is pinned.
  IDAPTIK_WINDOWS_SYSTEM32="$FIXTURE/system 32"
  [ "$(hp_windows_tool cmd.exe)" = "$FIXTURE/path-bin/cmd.exe" ]
  if hp_windows_tool explorer.exe; then echo 'missing tool accepted' >&2; exit 1; fi
)
(
  . "$REPO_DIR/scripts/windows-interop.sh"
  IDAPTIK_DIAG_SHARED_MEMORY="$FIXTURE/shared_memory"
  IDAPTIK_DIAG_WESTON_LOG="$FIXTURE/wslg/weston.log"
  if hp_wslg_copy_mode_broken; then echo 'healthy WSLg reported broken' >&2; exit 1; fi
  printf '%s\n' 'rdp_allocate_shared_memory: Failed to open fixture' > "$IDAPTIK_DIAG_WESTON_LOG"
  hp_wslg_copy_mode_broken
  IDAPTIK_DIAG_SHARED_MEMORY="$FIXTURE/missing"
  IDAPTIK_DIAG_WESTON_LOG="$FIXTURE/missing.log"
  hp_wslg_copy_mode_broken
)
: > "$FIXTURE/wslg/weston.log"

# Exercise both branches of the probe's timeout handling without waiting ten
# seconds. Keep PATH isolated so the result cannot depend on the host tools.
mkdir -p "$FIXTURE/no-timeout" "$FIXTURE/timeout-bin"
(
  unset IDAPTIK_DIAG_WINDOWS_INTEROP
  . "$REPO_DIR/scripts/windows-interop.sh"
  export IDAPTIK_DIAG_IS_WSL=1 IDAPTIK_WINDOWS_SYSTEM32="$FIXTURE/system 32"
  export PROBE_LOG="$FIXTURE/no-timeout-probes" PROBE_EXIT=0
  export PATH="$FIXTURE/no-timeout"
  : > "$PROBE_LOG"
  hp_windows_interop
  [ "$(< "$PROBE_LOG")" = '/c exit 0' ]
)
cat > "$FIXTURE/timeout-bin/timeout" <<'TIMEOUT'
#!/bin/sh
printf '%s\n' "$*" > "$TIMEOUT_LOG"
exit 124
TIMEOUT
chmod +x "$FIXTURE/timeout-bin/timeout"
(
  unset IDAPTIK_DIAG_WINDOWS_INTEROP
  . "$REPO_DIR/scripts/windows-interop.sh"
  export IDAPTIK_DIAG_IS_WSL=1 IDAPTIK_WINDOWS_SYSTEM32="$FIXTURE/system 32"
  export PROBE_LOG="$FIXTURE/timed-out-probes" TIMEOUT_LOG="$FIXTURE/timeout-args"
  export PATH="$FIXTURE/timeout-bin"
  : > "$PROBE_LOG"
  if hp_windows_interop; then echo 'timed-out probe accepted' >&2; exit 1; fi
  [ "$(< "$TIMEOUT_LOG")" = "10 $FIXTURE/system 32/cmd.exe /c exit 0" ]
  [ ! -s "$PROBE_LOG" ]
  rm_cmd="$FIXTURE/timeout-bin/timeout"
  /bin/rm "$rm_cmd"
  if hp_windows_interop; then echo 'timed-out probe was not cached' >&2; exit 1; fi
  [ ! -s "$PROBE_LOG" ]
)

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

# Isolate PATH so rustup is available only through mise, as on the affected
# setup. The second run verifies that missing cross-build prerequisites fail.
mkdir -p "$FIXTURE/tools" "$FIXTURE/home"
for tool in bash dirname uname awk getconf head grep sed tr; do
  ln -s "$(command -v "$tool")" "$FIXTURE/tools/$tool"
done
cat > "$FIXTURE/tools/mise" <<'MISE'
#!/bin/sh
case "$*" in
  --version) printf 'mise fixture\n' ;;
  'exec -- cargo --version') exit 0 ;;
  'exec -- rustup target list --installed')
    [ "${FIXTURE_TARGET:-0}" != 1 ] || printf 'x86_64-pc-windows-gnu\n'
    ;;
  *) exit 1 ;;
esac
MISE
printf '#!/bin/sh\nexit 0\n' > "$FIXTURE/tools/x86_64-w64-mingw32-gcc"
chmod +x "$FIXTURE/tools/mise" "$FIXTURE/tools/x86_64-w64-mingw32-gcc"
tools_report="$(env HOME="$FIXTURE/home" PATH="$FIXTURE/tools" FIXTURE_TARGET=1 \
  IDAPTIK_CLIENT_PLATFORM=windows IDAPTIK_DIAG_IS_WSL=1 IDAPTIK_DIAG_WINDOWS_INTEROP=1 \
  "$DOCTOR" report solo 2>&1)"
grep -q '\[PASS\].*Windows target.*x86_64-pc-windows-gnu' <<<"$tools_report"
grep -q '\[PASS\].*Windows linker' <<<"$tools_report"
rm "$FIXTURE/tools/x86_64-w64-mingw32-gcc"
if tools_missing="$(env HOME="$FIXTURE/home" PATH="$FIXTURE/tools" FIXTURE_TARGET=0 \
  IDAPTIK_CLIENT_PLATFORM=windows IDAPTIK_DIAG_IS_WSL=1 IDAPTIK_DIAG_WINDOWS_INTEROP=1 \
  "$DOCTOR" report solo 2>&1)"; then
  printf 'expected missing Windows target and linker to block launch\n' >&2
  exit 1
fi
grep -q '\[FAIL\].*Windows target.*just windows-client-deps' <<<"$tools_missing"
grep -q '\[FAIL\].*Windows linker.*just windows-client-deps' <<<"$tools_missing"

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

printf 'client started without an adapter line\n' > "$FIXTURE/game.log"
no_adapter="$(env IDAPTIK_DIAG_IS_WSL=1 IDAPTIK_DIAG_SKIP_TOOLS=1 \
  IDAPTIK_DIAG_DISPLAY=:0 IDAPTIK_DIAG_WSLG_DIR="$FIXTURE/wslg" \
  IDAPTIK_DIAG_SHARED_MEMORY="$FIXTURE/shared_memory" \
  IDAPTIK_DIAG_GAME_LOG="$FIXTURE/game.log" "$DOCTOR" report solo 2>&1)"
grep -q '\[INFO\].*bevy renderer.*logged no adapter line' <<<"$no_adapter"

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
