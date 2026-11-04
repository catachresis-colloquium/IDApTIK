# Bash source library; load with . scripts/local-session.sh.
# shellcheck shell=bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
# Shared boundary for the experimental, fixed-secret Burble test profile.
# This is not authenticated admission. Never accept a remote target here.

idaptik_local_port() {
  local port="$1"
  [[ "$port" =~ ^[0-9]{1,5}$ ]] || return 1
  port=$((10#$port))
  [ "$port" -ge 1 ] && [ "$port" -le 65535 ] || return 1
  printf '%s' "$port"
}

# Return a canonical numeric authority, never an unresolved hostname. Reject
# userinfo, queries, fragments, alternate paths, address lookalikes and TLS
# claims: this profile serves only ws://127.0.0.1:<port>/voice/websocket.
idaptik_local_target() {
  local target="$1" port
  if [[ "$target" =~ ^(ws://)?(127\.0\.0\.1|localhost)(:([0-9]{1,5}))?(/voice/websocket)?$ ]]; then
    port="${BASH_REMATCH[4]:-${IDAPTIK_PORT:-4000}}"
    port="$(idaptik_local_port "$port")" || {
      printf 'local-only session requires a port between 1 and 65535\n' >&2
      return 1
    }
    printf '127.0.0.1:%s' "$port"
  else
    printf 'local-only session requires 127.0.0.1 or localhost; authenticated remote hosting is not available\n' >&2
    return 1
  fi
}

# Standards launcher policy: predictable records inside a user-scoped
# directory, never directly in the shared /tmp namespace.
idaptik_runtime_root() {
  printf '%s/launch-scaffolder/idaptik' "${XDG_RUNTIME_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}}"
}

idaptik_private_dirs() {
  # umask also protects any newly created intermediate directories. Repair
  # the final directories' modes on upgrades from older launchers.
  (umask 077; mkdir -p -- "$@") || return 1
  chmod 0700 -- "$@"
}
