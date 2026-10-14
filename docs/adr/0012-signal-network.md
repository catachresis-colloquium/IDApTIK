# ADR-0012: A signal network for re-wiring the building lives in `idaptik-core`

- Status: Proposed
- Date: 2026-10-12
- Relates to: ADR-0004 (deterministic event-sourced sim), ADR-0007 (UMS package
  boundary), `docs/level-design.md`, `docs/dev-notes/roadmap-2026-07.md` §D

## Context

The hacker's verbs today are *pick a node, it does its one thing*:
`netsim::Actuation` maps each node to a fixed effect (HoldDoor, LoopCamera,
CutPower …), and cutting a feed cascades along `deps`. That is a good model of
**reach** — who can get to what — and a thin model of **consequence**. Nothing
is conditional and nothing chains: a door cannot be made to open *when* a
terminal is compromised, a camera cannot be told to switch off *while* Billy is
in the hall.

## Decision

Add a signal network to `crates/idaptik-core/src/scenario/signal/` as a
deterministic, definition-as-data runtime, following the building runtime's
shape:

| Piece | Module | Contract |
|---|---|---|
| Definition + validator | `signal/mod.rs` | `SignalNetworkDef`, format `idaptik-signal/1`, `load_signal_network` (the UMS seam) |
| Runtime | `signal/sim.rs` | `SignalSim::apply(cmd, access)` / `step(readout)`; snapshot `idaptik-signal-snapshot/1` |
| Fixture | `signal/ghost_lobby_wiring.rs` + golden JSON | the Ghost Lobby floor's own devices, derived from `ghost_lobby()` |

Devices are the physical-function devices the grounded network already has —
doors, cameras, lights, sensors, the vacuum, Billy, terminals — plus a
free-standing **relay** (AND/OR/XOR/NAND/NOR/LATCH). Each exposes output ports
(a door is open, a camera has tripped, a terminal is compromised) and input
ports (open, lock, disable, pulse). Wires carry booleans output → input,
optionally inverted. The installer's wires are hardwired; players lay their
own, up to a shared budget. Signal ids equal grounded-network node ids wherever
a device has one, so the two networks line up without a mapping table.

### Both seats wire; they differ in access and skill, not role

- **Access.** The infiltrator touches a device by standing at it; the hacker by
  reaching it (pivot depth, or a foothold the infiltrator jacked in). The host
  computes this per tick from vantage, room and pivot, and hands it to every
  command as an `Access`. A wire is configured at its **receiving end**, so a
  seat must be able to touch the device it is wiring *into*.
- **Skill.** A `SeatProfile` per seat prices the work — the hacker in
  bandwidth, the infiltrator in busy ticks at the panel — and gates the skilled
  verbs: re-programming relays and wiring into `hacker_only` devices are
  hacker-only by default. Events carry the cost; the host charges it.

### What the host owns

Outputs that are facts about the world — a camera sees someone, Billy is
pursuing, a terminal has been compromised, the vacuum is running — are driven
by the host each tick through a `Readout`. In particular, *how* a terminal gets
compromised is not this module's business: today that is the existing
`NetHack` path, or the infiltrator jacking in. What a wire *asks* the world to
do that the network cannot do itself (route the vacuum) comes back as an event.

### Tick semantics (fixed, tested)

Outputs are read from state at the start of the tick; inputs then change state.
A signal crosses exactly one wire per tick, which makes relay feedback loops
well-defined. Edge inputs fire once on the rising edge, however long the wire is
held; relay inputs are level. A door `pulse` holds for `ROUTE_DURATION`, the
same as a routed hold.

## Hosted in Ghost Lobby

The scenario definition carries an optional `signal` network and the runtime
state an optional `signal` state; both are skipped in serde when absent, so the
canonical floor, its golden and every recorded replay are byte-identical.
`ghost_lobby_wired()` is the canonical floor with the fixture network attached.
`Command::Signal { op }` carries an index-addressed `SignalOp` (so `Command`
stays `Copy`) and is routed to whichever seat `op` names. Each tick
(`sim/signal.rs`, right after the timers) the sim reads the world into a
`Readout`, steps the network, and applies it: a locked door overrides routes
and badges, a held door stays open, a switched-off camera sees nothing, a dark
room hides the infiltrator from cameras and Billy, and a wire into the vacuum
routes it. Hacker wiring is charged in bandwidth; infiltrator wiring holds them
still for the busy time.

The Bevy frontend draws the network over the cross-section and wires it with
the mouse (`crates/idaptik-bevy/src/wiring.rs`; `C` overlay, `V` seat).

## Not decided here (next steps)

1. **Netplay.** burble's relay routes by `cmd` tag; its role table needs
   `Signal` (seat taken from `op.by`) before wiring works over the network.
2. **Jacking in.** An infiltrator at a terminal adds its segment as a hacker
   vantage — a netsim change, not a signal one.
3. **Nickel schema** (`config/signal-schema.ncl`) and `bad_*.ncl` fixtures for
   `just config-scenario-check`.
4. **TUI view** of ports and wires.
5. **Real wiring for real floors** — the fixture's single interlock is
   illustrative, not level design.

## Consequences

- The hacker gains conditional, chainable verbs and the infiltrator gains a
  hands-on version of them; neither gains omniscience.
- The two networks stay distinct: `netsim` decides reach, `signal` decides
  consequence.
