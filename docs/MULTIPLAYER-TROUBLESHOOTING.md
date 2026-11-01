# Multiplayer troubleshooting

Known, reproduced failure modes of the multiplayer relay and launch path.
`launcher.sh` is the sole player-facing entry point and runs the Bevy GUI;
`idaptik-netplay` remains a direct TUI/verifier binary rather than the
player-facing launcher target. Add to this file when a new failure is
confirmed; don't speculate about ones that haven't been hit.

## Current security restriction: test relay is local-only

The launcher uses Burble's offline-capable **test** environment, which carries
test-only secrets and legacy guest/role joins without secure exclusive-seat
admission. It must not be exposed on a LAN, tailnet, or the public internet.
The host relay therefore binds **127.0.0.1 only**, refuses to reuse an
unverified listener, and prints **READY FOR LOCAL JOINER ONLY**. A second
player must run on the same computer. Do **not** forward this port.

To restore network play, a separate properly authenticated production-capable
host profile is required, with exclusive role admission and negative security
tests. A VPN alone is not a substitute for session-level authorisation. See
IDApTIK issue #211 and Burble issue #254. The local-only restriction does not
alter the headless CI loopback gate.

## A peer cannot join even though the relay is up

### The shared address is unreachable (WSL2 NAT)

**Status:** fixed in the launcher's share banner as of PR #78 (commit
`e01e3f5`) — this section documents the underlying cause for anyone still
hitting it via an older checkout, a hand-typed address, or a similar NAT
setup outside WSL2.

The internal `share_addresses()` helper used to print
whatever `ip -4 route get 1.1.1.1 | grep -oP 'src \K[0-9.]+'` returned and
label it `LAN:`. Under WSL2's default (NAT) networking, that command reports
the **WSL2 virtual adapter's own address** (a `172.16.0.0/12` address — e.g.
`172.28.101.65`) — not the Windows host's real LAN address. That address is
NAT'd behind Windows and cannot be reached from any other machine, even one
on the same physical network. Forwarding the relay's TCP port on the home
router does not fix this either: the router can see the Windows host, but
not past it into WSL2.

A user hit this directly: shared the printed "LAN" address with a remote
player, who got `ended_no_peer` because the address was never reachable in
the first place — the relay itself was working fine.

Earlier remote-hosting launcher revisions detected this case (`is_wsl` /
`wsl_natted`) and printed a warning plus the two routes
that actually work — a Tailscale/WireGuard tunnel (recommended), or a
router port-forward *combined with* a Windows `netsh interface portproxy`
hop into WSL2. WSL2 **mirrored** networking (Windows 11 22H2+) is unaffected
since it shares the host's real LAN address.

## The GUI does not open

Run the player-facing diagnostic before retrying:

```sh
./launcher.sh --doctor
```

The interactive menu also exposes **Diagnostics and safe repair**. Fatal
display faults stop before either the relay or GUI starts, so repeated retries
cannot accumulate invisible windows or misleading multiplayer sessions. Open
the friendly decision flow with `./launcher.sh --flowdiags`; it uses the
Windows browser under WSL and does not depend on WSLg working.

### WSLg shows `[WARN: COPY MODE]`

This title prefix comes from WSLg, not the game. It means WSLg has fallen back
from its shared-memory VAIL window transport to RAIL pixel copying. A confirmed
failure can leave a penguin taskbar item that cannot be restored/maximised and
can force Bevy onto the slow llvmpipe renderer.

The launcher checks both `/mnt/shared_memory` and the current WSLg Weston boot
log. If the mount is missing or Weston reports `rdp_allocate_shared_memory`
with an input/output error, a WSL-hosted Bevy window is unsafe. When Windows
interop, the `x86_64-pc-windows-gnu` Rust target, and the MinGW linker are
available, the launcher automatically builds and starts a native Windows Bevy
client instead. The Phoenix relay and Tailscale endpoint remain in WSL; the
local Windows client reaches that relay through `127.0.0.1`. The current
test profile does **not** advertise a tailnet address for remote players.

If that fallback is unavailable, launch remains blocked. From Windows
PowerShell as Administrator run:

```powershell
wsl --update
wsl --shutdown
```

Reopen the distribution and rerun `./launcher.sh --doctor`. Restart Windows if
Copy Mode remains. Do not manually create `/mnt/shared_memory`: that can hide
the symptom without restoring WSLg's host-backed transport.

Missing audio and software rendering are reported separately as degradations;
they do not block play. `./launcher.sh --repair` installs pinned repository
toolchains and can guide Tailscale bootstrap/authentication, but intentionally
cannot restart WSL or alter Windows GPU drivers from inside the session.

## Host/join readiness choreography

The host must not start the second **local** player until the launcher prints
`READY FOR LOCAL JOINER ONLY`. This means startup diagnostics and the Bevy
build have completed, and the loopback test relay has answered. It does **not**
claim authenticated remote-seat admission, application delivery or a durable
journal. The other player may join on the same computer through
`127.0.0.1:PORT`. Remote addresses must not be shared or port-forwarded.

The local joiner performs its own checks and probes the exact loopback port.
It prints `READY — JOINING HOST NOW` only after the local build and relay
probe. A cross is blocking; a warning is a disclosed degradation.

The main launcher builds and starts Bevy in the background. It reports the
startup log location when launching; by default this is
`$XDG_STATE_HOME/idaptik/game.log` (or
`$HOME/.local/state/idaptik/game.log`). Check it with:

```sh
tail -50 "${XDG_STATE_HOME:-$HOME/.local/state}/idaptik/game.log"
```

Run `./launcher.sh --status` to distinguish a slow first build from a process
that has exited, and `./launcher.sh --stop` to stop both the GUI and a relay
started by the host path.

## Legacy direct TUI use

The `idaptik-netplay --interactive` binary still has a 15-second default join
timeout and uses terminal raw mode. Those behaviours apply only when invoking
that binary directly; the player launcher never selects it. If a direct TUI run
leaves the terminal in raw/alternate-screen state, type `reset` (even blind)
or open another terminal and stop `idaptik-netplay`.
