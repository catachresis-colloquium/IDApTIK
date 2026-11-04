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

# Exercise the shared local-only boundary directly, including values that
# would be ambiguous if ports were interpreted as octal or as URL authorities.
(
  . "$REPO_DIR/scripts/local-session.sh"
  unset IDAPTIK_PORT
  for pair in '1 1' '00001 1' '0080 80' '65535 65535'; do
    read -r input expected <<<"$pair"
    actual="$(idaptik_local_port "$input")"
    [ "$actual" = "$expected" ] || { printf 'port %s became %s\n' "$input" "$actual" >&2; exit 1; }
  done
  for input in '' 0 00000 65536 100000 -1 +1 ' 80' 80x; do
    if idaptik_local_port "$input" >/dev/null; then
      printf 'invalid port accepted: [%s]\n' "$input" >&2; exit 1
    fi
  done

  for pair in 'localhost 127.0.0.1:4000' \
    '127.0.0.1:00001 127.0.0.1:1' \
    'ws://localhost:65535/voice/websocket 127.0.0.1:65535' \
    'ws://127.0.0.1/voice/websocket 127.0.0.1:4000'; do
    read -r input expected <<<"$pair"
    actual="$(idaptik_local_target "$input")"
    [ "$actual" = "$expected" ] || { printf 'target %s became %s\n' "$input" "$actual" >&2; exit 1; }
  done
  IDAPTIK_PORT=0080
  [ "$(idaptik_local_target localhost)" = '127.0.0.1:80' ]
  IDAPTIK_PORT=65536
  if idaptik_local_target localhost >/dev/null 2>&1; then
    echo 'invalid default port accepted' >&2; exit 1
  fi
  [ "$(idaptik_local_target localhost:1)" = '127.0.0.1:1' ]
  for input in '' localhost: localhost:00000 localhost:100000 \
    localhost. LOCALHOST 127.0.0.2 127.000.0.1 'localhost#fragment' \
    'localhost?query=1' 'ws://user@localhost/voice/websocket' \
    'ws://localhost/voice/websocket/' 'http://localhost' 'localhost/path' \
    'localhost:80/voice/websocket?x=1'; do
    if idaptik_local_target "$input" >/dev/null 2>&1; then
      printf 'invalid local target accepted: [%s]\n' "$input" >&2; exit 1
    fi
  done
)

# Runtime paths use XDG precedence and create or repair private directories
# without changing the caller's umask, including paths containing spaces.
(
  . "$REPO_DIR/scripts/local-session.sh"
  export HOME="$FIXTURE/home path" XDG_STATE_HOME="$FIXTURE/state path" XDG_RUNTIME_DIR="$FIXTURE/runtime path"
  [ "$(idaptik_runtime_root)" = "$XDG_RUNTIME_DIR/launch-scaffolder/idaptik" ]
  unset XDG_RUNTIME_DIR
  [ "$(idaptik_runtime_root)" = "$XDG_STATE_HOME/launch-scaffolder/idaptik" ]
  unset XDG_STATE_HOME
  [ "$(idaptik_runtime_root)" = "$HOME/.local/state/launch-scaffolder/idaptik" ]
  mkdir -p "$FIXTURE/existing private"
  chmod 0755 "$FIXTURE/existing private"
  umask 022
  idaptik_private_dirs "$FIXTURE/existing private" "$FIXTURE/new parent/new child"
  [ "$(stat -c %a "$FIXTURE/existing private")" = 700 ]
  [ "$(stat -c %a "$FIXTURE/new parent")" = 700 ]
  [ "$(stat -c %a "$FIXTURE/new parent/new child")" = 700 ]
  [ "$(umask)" = 0022 ]
  : > "$FIXTURE/not-a-directory"
  if idaptik_private_dirs "$FIXTURE/not-a-directory/child" 2>/dev/null; then
    echo 'directory creation failure was ignored' >&2; exit 1
  fi
)

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
  export IDAPTIK_DIAG_WINDOWS_INTEROP=1; hp_windows_interop
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
  export IDAPTIK_DIAG_SHARED_MEMORY="$FIXTURE/missing"
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

# A local relay requires neither a usable external route nor authenticated
# Tailscale. These were incorrectly inherited from the retired remote flow.
for lan in '' 172.28.1.2; do
  offline="$(IDAPTIK_DIAG_LAN_ADDRESS="$lan" run_fixture preflight host 2>&1)"
  grep -q '\[PASS\].*local-only profile uses 127.0.0.1' <<<"$offline"
  ! grep -q '\[FAIL\].*tailscale\|\[FAIL\].*reachability' <<<"$offline"
done
for port in 0 65536 invalid; do
  if invalid_port="$(IDAPTIK_PORT="$port" run_fixture preflight host 2>&1)"; then
    printf 'invalid host port accepted: %s\n' "$port" >&2; exit 1
  fi
  grep -q '\[FAIL\].*relay port.*between 1 and 65535' <<<"$invalid_port"
done

# The diagnostic cannot be used as a network probe for an unsupported legacy
# session. An answering HTTP root must also not be called session admission.
mkdir -p "$FIXTURE/network-bin"
cat > "$FIXTURE/network-bin/curl" <<'CURL'
#!/bin/sh
printf '%s\n' "$*" >> "$IDAPTIK_FIXTURE_PROBE_LOG"
exit "${IDAPTIK_FIXTURE_PROBE_EXIT:-0}"
CURL
chmod +x "$FIXTURE/network-bin/curl"
probe_log="$FIXTURE/network-probes"
: > "$probe_log"
for target in example.test 100.64.0.1 ws://127.0.0.1@example.test/voice/websocket \
  127.0.0.1:0 127.0.0.1:65536 wss://127.0.0.1/voice/websocket; do
  if rejected="$(PATH="$FIXTURE/network-bin:$PATH" IDAPTIK_FIXTURE_PROBE_LOG="$probe_log" \
    run_fixture preflight join "$target" 2>&1)"; then
    printf 'unsupported target accepted: %s\n' "$target" >&2
    exit 1
  fi
  grep -q '\[FAIL\].*host target.*local-only' <<<"$rejected"
done
[ ! -s "$probe_log" ]
local_probe="$(PATH="$FIXTURE/network-bin:$PATH" IDAPTIK_FIXTURE_PROBE_LOG="$probe_log" \
  run_fixture preflight join ws://localhost:4123/voice/websocket 2>&1)"
grep -qF 'http://127.0.0.1:4123/' "$probe_log"
grep -qF -- '--noproxy *' "$probe_log"
grep -qF 'session admission and game readiness are not established' <<<"$local_probe"
! grep -qF 'the host has reached' <<<"$local_probe"
if failed_probe="$(PATH="$FIXTURE/network-bin:$PATH" IDAPTIK_FIXTURE_PROBE_LOG="$probe_log" \
  IDAPTIK_FIXTURE_PROBE_EXIT=7 run_fixture preflight join localhost:1 2>&1)"; then
  echo 'failed relay probe did not block join' >&2; exit 1
fi
grep -q '\[FAIL\].*host relay.*127.0.0.1:1 is not reachable' <<<"$failed_probe"
grep -qF 'http://127.0.0.1:1/' "$probe_log"

# PID paths are user-scoped even without XDG_RUNTIME_DIR. TMPDIR must not
# redirect stable ownership records into a shared directory.
runtime_root="$(unset XDG_RUNTIME_DIR; XDG_STATE_HOME="$FIXTURE/state" TMPDIR=/tmp \
  bash -c '. "$1/scripts/local-session.sh"; idaptik_runtime_root' _ "$REPO_DIR")"
[ "$runtime_root" = "$FIXTURE/state/launch-scaffolder/idaptik" ]

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
  'exec -- mix --version') exit 0 ;;
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

# Multiplayer needs the PowerShell owner even when the cross-build tools are
# otherwise present; solo does not. Use an isolated PATH for both controls.
printf '#!/bin/sh\nexit 0\n' > "$FIXTURE/tools/powershell.exe"
printf '#!/bin/sh\nexit 0\n' > "$FIXTURE/tools/curl"
chmod +x "$FIXTURE/tools/powershell.exe" "$FIXTURE/tools/curl"
for profile in host join; do
  windows_multiplayer="$(env HOME="$FIXTURE/home" PATH="$FIXTURE/tools" FIXTURE_TARGET=1 \
    IDAPTIK_WINDOWS_SYSTEM32="$FIXTURE/empty" IDAPTIK_CLIENT_PLATFORM=windows \
    IDAPTIK_DIAG_IS_WSL=1 IDAPTIK_DIAG_WINDOWS_INTEROP=1 \
    "$DOCTOR" report "$profile" localhost 2>&1)"
  grep -q '\[PASS\].*Windows seat owner' <<<"$windows_multiplayer"
done
rm "$FIXTURE/tools/powershell.exe"
for profile in host join; do
  if missing_owner="$(env HOME="$FIXTURE/home" PATH="$FIXTURE/tools" FIXTURE_TARGET=1 \
    IDAPTIK_WINDOWS_SYSTEM32="$FIXTURE/empty" IDAPTIK_CLIENT_PLATFORM=windows \
    IDAPTIK_DIAG_IS_WSL=1 IDAPTIK_DIAG_WINDOWS_INTEROP=1 \
    "$DOCTOR" report "$profile" localhost 2>&1)"; then
    printf 'Windows %s accepted without PowerShell\n' "$profile" >&2; exit 1
  fi
  grep -q '\[FAIL\].*Windows seat owner.*powershell.exe' <<<"$missing_owner"
done
! grep -q 'Windows seat owner' <<<"$tools_report"

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

# Without an override, read the solo seat's log rather than the old shared log.
mkdir -p "$FIXTURE/renderer-state/idaptik/seats/solo"
cp "$FIXTURE/game.log" "$FIXTURE/renderer-state/idaptik/seats/solo/game.log"
printf '%s\n' 'AdapterInfo { name: "llvmpipe", backend: Vulkan }' > "$FIXTURE/renderer-state/idaptik/game.log"
default_renderer="$(env -u IDAPTIK_DIAG_GAME_LOG XDG_STATE_HOME="$FIXTURE/renderer-state" \
  IDAPTIK_DIAG_IS_WSL=1 IDAPTIK_DIAG_SKIP_TOOLS=1 IDAPTIK_DIAG_DISPLAY=:0 \
  IDAPTIK_DIAG_WSLG_DIR="$FIXTURE/wslg" IDAPTIK_DIAG_SHARED_MEMORY="$FIXTURE/shared_memory" \
  IDAPTIK_DIAG_WESTON_LOG="$FIXTURE/wslg/weston.log" \
  "$DOCTOR" report solo 2>&1)"
grep -q '\[PASS\].*bevy renderer.*NVIDIA GeForce RTX 4070 Laptop GPU (Vulkan)' <<<"$default_renderer"
! grep -q '\[WARN\].*bevy renderer.*llvmpipe' <<<"$default_renderer"

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
