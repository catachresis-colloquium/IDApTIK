#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
#
# Windows interop discovery for the WSL2 launch path.
#
# Sourced by launcher.sh, scripts/runtime-doctor.sh and install.sh so all three
# answer the same question the same way: *can this machine start the native
# Windows client?* Nothing here is executed on `source`.
#
# Why this file exists
# --------------------
# All three scripts used to test `command -v cmd.exe`. That is not a usable
# test for interop. WSL does not require C:\Windows\System32 on PATH, and the
# working route on the maintainer's machine needed
#
#   PATH="$PATH:/usr/bin:/mnt/c/Windows/System32" ./launcher-wsl.sh --solo
#
# with cmd.exe absent from PATH beforehand. A machine whose interop works was
# therefore told it had none, and the only known cure for the WSLg presentation
# failure (run the Windows client) was unreachable from the standard launcher.
#
# So: resolve the executables where WSL actually mounts them, then *prove*
# interop by running one. A file that exists but cannot execute (interop
# disabled in /etc/wsl.conf) must not count as working.
#
# Test hooks (used by tests/*.sh; no effect in normal use)
#   IDAPTIK_DIAG_IS_WSL           1|0  override WSL detection
#   IDAPTIK_DIAG_WINDOWS_INTEROP  1|0  override the whole interop verdict
#   IDAPTIK_WINDOWS_SYSTEM32      DIR  pin the Windows system directory
#   IDAPTIK_DIAG_WSLG_DIR         DIR  override /mnt/wslg
#   IDAPTIK_DIAG_SHARED_MEMORY    DIR  override /mnt/shared_memory
#   IDAPTIK_DIAG_WESTON_LOG       FILE override $wslg_dir/weston.log

HP_WINDOWS_SYSTEM32_CACHE=""
HP_WINDOWS_INTEROP_CACHE=""

# Detect WSL without trusting /proc/version when a caller overrides it.
hp_is_wsl() {
  if [ -n "${IDAPTIK_DIAG_IS_WSL:-}" ]; then
    [ "$IDAPTIK_DIAG_IS_WSL" = "1" ]
    return
  fi
  grep -qi microsoft "${IDAPTIK_DIAG_PROC_VERSION:-/proc/version}" 2>/dev/null
}

# Print the Windows system directory (the one holding cmd.exe), or fail.
# An explicit IDAPTIK_WINDOWS_SYSTEM32 never falls through to a guessed path:
# tests and users pin the answer, including the answer "none".
hp_windows_system32() {
  if [ -n "$HP_WINDOWS_SYSTEM32_CACHE" ]; then
    printf '%s' "$HP_WINDOWS_SYSTEM32_CACHE"
    return 0
  fi

  local dir=""
  if [ -n "${IDAPTIK_WINDOWS_SYSTEM32:-}" ]; then
    [ -f "$IDAPTIK_WINDOWS_SYSTEM32/cmd.exe" ] || return 1
    dir="$IDAPTIK_WINDOWS_SYSTEM32"
  elif [ -f /mnt/c/Windows/System32/cmd.exe ]; then
    dir="/mnt/c/Windows/System32"
  elif command -v wslpath >/dev/null 2>&1; then
    # Custom drvfs mount root (not every distribution mounts C: at /mnt/c).
    local translated
    translated="$(wslpath -u 'C:\Windows\System32' 2>/dev/null || true)"
    [ -n "$translated" ] && [ -f "$translated/cmd.exe" ] && dir="$translated"
  fi

  [ -n "$dir" ] || return 1
  HP_WINDOWS_SYSTEM32_CACHE="$dir"
  printf '%s' "$dir"
}

# Print the path of a Windows tool (cmd.exe, explorer.exe, powershell.exe),
# preferring PATH and falling back to the Windows system directory.
hp_windows_tool() {
  local name="$1" path dir
  if path="$(command -v "$name" 2>/dev/null)" && [ -n "$path" ]; then
    printf '%s' "$path"
    return 0
  fi
  if dir="$(hp_windows_system32)"; then
    if [ -f "$dir/$name" ]; then
      printf '%s/%s' "$dir" "$name"
      return 0
    fi
  fi
  return 1
}

# Return success when the WSL detection and cmd.exe execution probe succeed.
# IDAPTIK_DIAG_WINDOWS_INTEROP overrides the verdict. Otherwise the verdict is
# cached in the current shell; the probe times out after 10 seconds if timeout
# is available.
hp_windows_interop() {
  if [ -n "${IDAPTIK_DIAG_WINDOWS_INTEROP:-}" ]; then
    [ "$IDAPTIK_DIAG_WINDOWS_INTEROP" = "1" ]
    return
  fi
  case "$HP_WINDOWS_INTEROP_CACHE" in
    1) return 0 ;;
    0) return 1 ;;
  esac

  # Running a Windows program needs interop and a resolvable cmd.exe, not
  # wslpath: wslpath only translates paths (for --flowdiags). Requiring it here
  # would report "no interop" for a machine that can launch the client fine.
  local cmd="" probe_ok=0
  if hp_is_wsl; then
    cmd="$(hp_windows_tool cmd.exe 2>/dev/null || true)"
  fi

  # Prove it. /etc/wsl.conf can disable interop while the mount stays
  # readable, in which case cmd.exe exists but exec fails.
  if [ -n "$cmd" ]; then
    if command -v timeout >/dev/null 2>&1; then
      if timeout 10 "$cmd" /c exit 0 >/dev/null 2>&1; then probe_ok=1; fi
    else
      if "$cmd" /c exit 0 >/dev/null 2>&1; then probe_ok=1; fi
    fi
  fi

  if [ "$probe_ok" -eq 1 ]; then
    HP_WINDOWS_INTEROP_CACHE=1
    return 0
  fi
  HP_WINDOWS_INTEROP_CACHE=0
  return 1
}

# Print a likely reason interop is unavailable, based on WSL detection and
# cmd.exe discovery. This function does not run the interop probe itself.
hp_windows_interop_reason() {
  if ! hp_is_wsl; then
    printf 'this is not a WSL2 session'
  elif ! hp_windows_tool cmd.exe >/dev/null 2>&1; then
    printf 'no cmd.exe under /mnt/c/Windows/System32 or PATH; mount C: or enable interop in /etc/wsl.conf'
  else
    printf 'cmd.exe exists but did not run; interop may be disabled in /etc/wsl.conf (set [interop] enabled=true)'
  fi
}

# Return success if the shared-memory directory is missing or the readable
# Weston log reports a shared-memory open failure; return failure otherwise.
hp_wslg_copy_mode_broken() {
  local wslg_dir="${IDAPTIK_DIAG_WSLG_DIR:-/mnt/wslg}"
  local shared_memory="${IDAPTIK_DIAG_SHARED_MEMORY:-/mnt/shared_memory}"
  local weston_log="${IDAPTIK_DIAG_WESTON_LOG:-$wslg_dir/weston.log}"
  [ ! -d "$shared_memory" ] || {
    [ -r "$weston_log" ] &&
      grep -qE 'rdp_allocate_shared_memory: Failed to open' "$weston_log"
  }
}
