# Multiplayer troubleshooting

Known, reproduced failure modes of the multiplayer relay and launch path.
`launcher.sh` is the sole player-facing entry point and runs the Bevy GUI;
`idaptik-netplay` remains a direct TUI/verifier binary rather than the
player-facing launcher target. Add to this file when a new failure is
confirmed; don't speculate about ones that haven't been hit.

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

The launcher now detects this case (`is_wsl` / `wsl_natted` in its internal
multiplayer runtime) and prints a warning plus the two routes
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
local Windows client reaches that relay through `127.0.0.1`, while a remote
peer uses the displayed tailnet address.

If that fallback is unavailable, launch remains blocked. From Windows
PowerShell as Administrator run:

```powershell
wsl --update
wsl --shutdown
```

Reopen the distribution and rerun `./launcher.sh --doctor`. Restart Windows if
Copy Mode remains. Do not manually create `/mnt/shared_memory`: that can hide
the symptom without restoring WSLg's host-backed transport.

### The WSLg window opens and dies immediately

Reproduced on this machine: Bevy selected the NVIDIA adapter with the `gl`
backend and then failed during presentation with

```
Fallback system failed to choose present mode.
This is a bug. Mode: Fifo, Options: []
```

(or `EGL: DRI2: failed to load driver`), well before any game logic ran. This
is a WSLg surface/present-mode failure, not a game bug. Changing the present
mode does not fix an empty capability list, and the GL override environment
variables should not be treated as the fix. Hardware detection succeeding
(`glxinfo` reporting D3D12/NVIDIA) proves only the OpenGL path: wgpu may still
land on llvmpipe.

The route that works is the native Windows client, which bypasses WSLg
entirely. It was previously reachable only by exporting
`IDAPTIK_CLIENT_PLATFORM=windows` before running the launcher; it is now a
first-class flag:

```sh
./launcher.sh --solo --windows-client
```

The flag composes with `--host`, `--join` and `--auto`. Interop is discovered
at `/mnt/c/Windows/System32`, so `cmd.exe` does not need to be on `PATH`, and it
is proved by running `cmd.exe`, so a WSL config with interop disabled is not
mistaken for a working one. Provision the cross-toolchain once with:

```sh
just windows-client-deps
```

When the solo client dies during surface setup, the launcher now says so and
prints this exact command instead of repeating the same launch.

### Which renderer did I actually get?

Readiness is not evidence about the renderer. Since 0.5.0 the launcher prints
the `AdapterInfo` line Bevy logs when it chooses an adapter, for example:

```
renderer: NVIDIA GeForce RTX 4070 Laptop GPU (Vulkan)
```

and labels software adapters (`llvmpipe`, `softpipe`, `SwiftShader`, `WARP`) as
a warning. `./launcher.sh --status` prints the same line for the last run, and
`./launcher.sh --doctor` reports both the OpenGL renderer and the adapter Bevy
selected, which can disagree.

Missing audio and software rendering are reported separately as degradations;
they do not block play. `./launcher.sh --repair` installs pinned repository
toolchains and can guide Tailscale bootstrap/authentication, but intentionally
cannot restart WSL or alter Windows GPU drivers from inside the session.

## Host/join readiness choreography

The host must not invite the second player until the launcher prints the green
`READY FOR JOINERS` result. It means diagnostics passed, the Bevy build
completed, the relay answered its health probe, a peer-reachable address was
found, and the host seat is launching. Under WSL2 NAT, an authenticated
Tailscale route is required.

The joiner should start only after receiving that message. Their launcher
performs the same local checks and probes the exact host/port. It prints
`READY — JOINING HOST NOW` only after the local build is complete and the relay
answers. A cross is blocking; a warning is a disclosed degradation.

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
