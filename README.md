<div align="center">

# IDApTIK

## *If you have to ask, you're already compromised.*

**Invisible Door: Action Point Trauma Inc. Kickers**

An asymmetric two-player infiltration game. One of you goes in. The other keeps you alive from a screen. Neither of you is fully in control, and that is the point.

</div>

---

## What this is

IDApTIK is a game about infiltration, deception, and trust, usually misplaced.

The repository's machine-readable sources have distinct roles: `IDApTIK_chora.deed` records descriptive project state, the six K9 components under `.machine_readable/contractiles/` hold policy, and root `coordination.k9` is the agent-instructions protocol. Run `just machine-readable-check` to validate them against the pinned standards.

One player is the **infiltrator**: moving through the world, dodging obstacles, bypassing security, staying unseen. The other is the **hacker**: watching remotely, opening doors, timing elevators, rewiring electronics, cutting the lights, and generally trying to keep the infiltrator breathing.

Control is not shared. It is asymmetric. The infiltrator moves; the hacker watches. If they do not work together, someone gets caught. The recurring problem is that the hacker's competence and the infiltrator's survival are loosely correlated at best.

> *"Breaking in is easy. Getting back out is harder. Especially when your escape route depends on a hacker who swears he knows what he's doing, right up until he accidentally reboots the entire security grid."*

## How it plays

Underneath the platforming sits a grounded network simulation, so the hacker is solving a real, legible network rather than pressing a "hack" button.

- **Grounded access.** The hacker starts from a specific device and sees only what is reachable from where they are.
- **Real topology.** Star networks, subnets, zones (LAN / VLAN / External), routing, DNS resolution, traceroute hop paths.
- **Movement through the network.** SSH between devices, session chaining, and pivoting so you attack from a compromised machine's perspective.
- **Pressure.** Active trace timers during intrusions, action-based alerts (failed logins, port scans), and passive logging for post-incident investigation. Bounce through intermediate machines to slow the trace.
- **The physical layer bites back.** Power grids, signal strength, cut cables, and physical location all affect what the hacker can do, and the infiltrator can open doors the hacker cannot reach remotely.

The two roles share one world. The hacker has a physical presence and real discovery risk; this is not a disembodied support role.

## Roadmap

Development runs in vehicle-named phases, smallest viable thing first:

**Envelope** &rarr; **Wheel** &rarr; **Skateboard** &rarr; **Bicycle** &rarr; **Chassis** &rarr; **Motorcycle** &rarr; **Computer Game**

- **Envelope (v0.1)** — minimum viable game: core movement, basic hacker interactions (doors, elevators, overrides), first art, documented mechanics.
- **Wheel** — refined movement and interaction; stealth responsiveness; expanded hacker capabilities (security loop manipulation, timed overrides, sabotage).
- **Skateboard** — deeper asymmetric multiplayer; multi-phase infiltration; infiltrator/hacker communication.
- **Bicycle** — strategic depth; AI countermeasures that react to interference.
- **Chassis** — richer environmental mechanics (terrain, sound sensitivity, misdirection); first non-linear missions.
- **Motorcycle** — the full experience: adaptive security AI, emergent tension, the complete multiplayer framework.
- **Computer Game** — beyond proof of concept.

## Tech

The Rust era's stack is being pinned as decisions are made, and each decision is recorded as an ADR under [`docs/adr/`](docs/adr/) rather than asserted here and quietly contradicted later.

- **Gameplay truth:** Rust/Crusoe — an engine-agnostic core, with **Bevy** as the
  selected graphical frontend over it
  ([ADR-0008](docs/adr/0008-select-bevy-and-retire-fyrox.md)).
- **Current relay/session coordination:** Elixir/OTP in Burble — **Bandit** +
  **Phoenix Channels**, not LiveView ([ADR-0002](docs/adr/0002-multiplayer-transport.md));
  simulation authority is a separate deployment choice
  ([ADR-0015](docs/adr/0015-deployment-topologies-and-state-ownership.md)).
- **FFI / ABI policy:** **Zig unified-api-adapter APIs/FFIs** for the adapter and **Idris2** for ABI
  contracts. The C ABI is still exported directly from Rust/Crusoe, the Idris2 ABI
  model exists (`crates/idaptik-ffi/abi`, gated by `just abi-model-check`), and the Zig
  adapter + UnifiedApiAdapter/serde-wire conformance harness now exist
  (`crates/idaptik-ffi/zig-adapter`, gated by `.github/workflows/abi-conformance.yml`:
  surface lockstep, fixture conformance incl. a planted failing fixture, and
  authority-digest drift rejection per `docs/abi/ABI-AUTHORITY.md`) — remaining
  work continues in [#103](https://github.com/metadatastician/IDApTIK/issues/103)
  ([ADR-0001](docs/adr/0001-toolchain-and-runtime-management.md)).
- **Config:** **Nickel** (typed configuration).
- **Toolchains:** pinned in `mise.toml` + `rust-toolchain.toml`; `just` runs tasks. On a fresh Debian/Ubuntu/WSL2 clone run `./install.sh` (also `just install`): it installs rustup, the pinned Rust, the mise tools and Bevy's system libraries (asks for `sudo`), builds the Bevy frontend, and ends with the runtime doctor. Otherwise provision with `just setup` (or `just bootstrap` for a fast, prebuilt-only bring-up), and check with `just doctor`.

The deployment contract is recorded in
[ADR-0015](docs/adr/0015-deployment-topologies-and-state-ownership.md). Direct
peer transport, a server-authoritative hosted runtime, and federated session
routing are not implemented yet. Still open: long-term player-save
compatibility/retention and the Rust↔Elixir wire encoding.

Run `./launcher.sh` for the player-facing menu: choose solo or multiplayer,
then choose the Infiltrator or Hacker seat for multiplayer. Both paths launch
the Bevy GUI. It is the sole player-facing entry point; relay and seat-process
machinery lives under `scripts/` as a guarded internal component. Connection
trouble is covered by
[`docs/MULTIPLAYER-TROUBLESHOOTING.md`](docs/MULTIPLAYER-TROUBLESHOOTING.md).
Before launch, the menu and `./launcher.sh --doctor` review the player machine,
OS/kernel/WSL transport, graphics coprocessor, audio, pinned dependencies,
Tailscale and reachability. `./launcher.sh --repair` performs safe dependency
and guided network remediation; `./launcher.sh --flowdiags` opens a friendly
HTML decision flow, and `./launcher.sh --man` displays the complete manual.
Hosts invite peers only after the green `READY FOR JOINERS` result; joiners get
their own build/reachability checklist and green joining result.

## Layout

```
crates/idaptik-core     engine-agnostic gameplay truth — the network sim + Ghost Lobby scenario, no rendering
crates/idaptik-ffi      Rust-exported C ABI + Idris2 ABI model (abi/) + Zig adapter (zig-adapter/, gated) (#103)
crates/idaptik-tui      ratatui/crossterm evaluation frontend + --headless/--replay/--export verifier over core (ADR-0004)
crates/idaptik-bevy     selected Bevy rendering frontend (ADR-0008)
crates/idaptik-net      Phoenix Channels client over burble game-session fabric (ADR-0006)
crates/idaptik-evil-weevil  sans-IO deterministic adversarial testing adapter over the multiplayer acceptance rules (ADR-0013)
config/                    Nickel — typed, schema-checked game/network config
docs/adr/                  architecture decision records
```

`just` runs the common tasks (`just doctor`, `just test`, `just run-bevy`,
`just loopback-check`, `just config-check`); toolchains are pinned in
`mise.toml` + `rust-toolchain.toml`. The loopback gate starts a throwaway
server from the sibling `metadatastician/burble` checkout; IDApTIK no longer
contains its own relay server. `just evil-weevil-check` runs the offline
adversarial corpus (no sockets, no Elixir); `just evil-weevil-relay-check`
adds the optional real-relay leg against that same throwaway local burble —
see `docs/security/evil-weevil-corpus.md`.

## UMS-authored package proof

The game owns the versioned package and gameplay contracts in
`contracts/idaptik/v1`; Universal Modding Studio consumes them without the game
depending on the UMS UI. From the sibling UMS repository, run:

```sh
just roundtrip-idaptik
```

That command compiles the UMS Ghost Lobby profile source, hands the exact
temporary artifact to IDApTIK's real package loader, executes a grounded camera
uplink, snapshots, restores, replays, and fails unless events and final state
match.

**Status:** implemented and CI-gated in Universal Modding Studio. The UMS
profile compiler consumes this repository's `contracts/idaptik/v1`, then the
real IDApTIK loader executes, snapshots, restores and replay-checks the emitted
package. UMS remains absent from the shipped game runtime.

The current Enaction evidence covers extracted interpolation parity; IDApTIK
still uses Bevy's fixed-step accumulator, so no broader timing adoption is
claimed.

## Licensing

IDApTIK is layered, and each layer is licensed for what it is. The whole project is open and is built to stay open.

| Layer | License |
|-------|---------|
| **Engine & code** | [AGPL-3.0-or-later](https://www.gnu.org/licenses/agpl-3.0.html) |
| **Game content** (art, levels, narrative, character designs, audio) | [CC-BY-SA-4.0](https://creativecommons.org/licenses/by-sa/4.0/) |
| **Names & marks** (IDApTIK, IDApTIK, Moletaire) | Trademark, all rights reserved |

The engine is strong copyleft: anyone who builds on it, including as a hosted service, must share their entire derivative source. The content is free culture: you may use, modify, and even sell derivatives, provided you attribute and keep them under the same share-alike terms.

## Contributing

Contributions come in under the Developer Certificate of Origin (DCO 1.1); sign your commits with `git commit -s`. See `CONTRIBUTING.md`.

## Status

Envelope-stage vertical slice. The deterministic Ghost Lobby simulation, TUI,
delay-lockstep client netplay, Bevy renderer, and UMS v1 package boundary are
implemented. Current netplay is relay-only: both seats simulate locally; direct
peer transport, hosted server authority, and multi-server failure recovery are
not shipped. IDApTIK's Rust suites, Bevy frontend, Nickel contracts, the
Burble-backed two-seat session path, and cross-repository UMS round trip are
CI-gated. The next milestone is a coherent player-facing Bevy shell with first
art, real-window validation, and documented mechanics.

---

<div align="center">

*The infiltrator moves. The hacker watches. Try to leave together.*

</div>
