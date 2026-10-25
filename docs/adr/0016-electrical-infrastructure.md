# ADR-0016: Electrical infrastructure in `idaptik-core`

- Status: Proposed
- Date: 2026-10-24
- Relates to: ADR-0004 (deterministic event-sourced sim), ADR-0012 (signal
  network), `docs/level-design.md`, `scenario/building/mod.rs` (`CircuitDef`,
  `ZoneDef`), `netsim::effect` (the power cascade)

## Context

Power is already load-bearing in three places, and none of them agree:

1. **`netsim::apply_actuation`** cuts a node and walks `deps` in reverse, emitting
   `Effect::DevicePowerLost` for everything downstream. That models **reach and
   cascade** — who is fed from whom — and nothing else. There is no rating, no
   protection, no backup, and one command kills a whole chain with no way back.
2. **`building::CircuitDef`** is a power feed keyed to a zone: an unpowered portal
   cannot be traversed, and lifts must declare a circuit. It is one boolean per
   circuit and one node per feed.
3. **The signal network** (ADR-0012) has input ports called `Enable` and
   `Disable`. Nothing stops a wire setting `Enable` on a camera whose supply was
   severed a minute ago. A wire can therefore assert a fact that is not true.

The gameplay question underneath all three is not "how many volts". It is:

> The device is off. **Which rule won** — its order, its supply, its damage, or a
> safety interlock?

That question has a small, complete answer, and it is the whole of this ADR.

## Decision

Add `crates/idaptik-core/src/scenario/power/`, a deterministic,
definition-as-data electrical layer that sits **beside** the signal network and
**above** the netsim cascade, and is optional per floor:

| Piece | Module | Contract |
|---|---|---|
| Definition + validator | `power/mod.rs` | `PowerNetworkDef`, format `idaptik-power/1`, `load_power_network` (the UMS seam) |
| Runtime | `power/sim.rs` | `PowerSim::apply(cmd, access)` / `step(readout)`, snapshot `idaptik-power-snapshot/1` |
| Test floor | `power/test_floor.rs` + golden JSON | two independent feeds, one backup-powered device, on the Ghost Lobby |
| Host seam | `scenario/sim/power.rs` | access, readout, floor effects, `Command::Power` |

### The six distinctions, as types

| Piece | Type | What it is |
|---|---|---|
| **Power source** | `SourceDef` (`mains` / `genset` / `battery`) | where energy enters; `startup_ticks` is how long it takes to arrive |
| **Distribution** | `FeedDef` | the circuit: one primary source, one `capacity` rating, one `breaker`, one room the run can be reached in |
| **Load / device** | `LoadDef` | what draws: `draw` units, a `kind`, a `restart` policy, optionally the `signal` device it *is* |
| **Backup supply** | `BackupSpec` on a source | `runtime_ticks` of carrying named loads, engaged `automatic`ally or `manual`ly |
| **Protective isolation** | `BreakerDef` | the device that opens before the wiring does, and needs a deliberate reset |
| **Damaged / disconnected / unavailable** | `Condition` + `OffReason` | two authored device states, and one derived verdict |

Every quantity is abstract and every threshold is readable: draw units against a
capacity rating, runtime and warm-up in ticks (seconds at the fixed 60 Hz),
noise on the same 0..1 scale movement already uses. There is no impedance, no
inrush and no voltage in this model, and there will not be.

### The arbitration rule

One line, and every conflict in the subsystem is a corollary of it:

```text
operating = commanded AND energized AND intact AND not interlocked AND warmed up
```

A command — manual or remote, from either seat, through any number of wires — is
**one conjunct**. It cannot beat a flat battery, a cut conductor, a tripped
breaker, an open interlock or a broken pump. This is the requirement that a
command to enable a device shall not make an unpowered device operate, expressed
as the shape of the type rather than a rule to remember.

When the conjunction is false, the runtime reports the **first** rung of this
ladder that fails, walking upstream from the device:

| # | `OffReason` | Meaning | Cured by |
|---|---|---|---|
| 1 | `Damaged` | the device is broken | nothing this run |
| 2 | `Disconnected` | it is wired to nothing | a `Route` |
| 3 | `Interlock { interlock, label }` | a local safety is holding it | the condition that holds it |
| 4 | `Unavailable { feed }` | the supply is gone for good (cut conductor) | another feed |
| 5 | `Isolated { breaker }` | its protection is open | a deliberate reset |
| 6 | `Unpowered { feed }` | its supply is off, and recoverable | closing the breaker / the source |
| 7 | `Restarting { ticks }` | supply is back, it is warming up | time |
| 8 | `NoCommand` | everything is ready and nobody asked | a command |

The order is the diagnostic order a competent person uses: check the device, then
its safeties, then its supply, then the order. `LoadReport::explain()` renders it
as one line; `PowerCommand::Inspect` returns it as an event.

### Conflict behaviour, stated

| Conflict | Winner | Why |
|---|---|---|
| remote command vs local switch (both `SetCommand`) | whichever arrived last | they are the same verb with the same authority; only access and cost differ |
| command vs supply | supply | `commanded` is one conjunct of `operating` |
| command vs interlock | interlock | an interlock is not a state, it is a veto |
| command vs fault/damage | fault | a broken device is not a device that is off |
| breaker reset vs standing fault | fault | `FaultPresent` refuses the reset; the fault must go first |
| two interlocks on one device | first in definition order | the model is deliberately not a solver |
| interlock reading a machine that is interlocked by it | start-of-tick state | interlocks read contactors, not this tick's arithmetic, so mutual pairs are deterministic rather than order-dependent |

### Tick semantics (fixed and tested)

Sources (start-up, backup engagement and runtime) → protection (an overload
counts up to `trip_ticks` and then trips; an overload fault clears the moment the
rating is respected) → feeds (live iff not cut, breaker closed, primary source
delivering; one event per feed transition, not per device) → loads (supply, then
restart policy, then the ladder) → hazards (live by their own rule; one shock per
contact).

A backup engages against *last* tick's primary supply — never against its own
output, which would make it oscillate every other tick — and it spends its
runtime *after* the tick it paid for, so a backup authored for one tick delivers
one tick rather than none. Because a `Command` is applied before the tick, a cut
cable and the UPS picking up the load it serves land in the same tick: a
backed-up camera does not blink. That is a feature and it is asserted.

Power also runs **before** the player's input is chosen, because it is the only
system that can make the body unable to act on the same tick it acts: a first
contact with live metal stuns now, not next tick.

### Restart behaviour is stated per device, never discovered

`Restart` is required in the definition, so a level cannot omit the decision:

- **`FailSafe`** — losing supply drops the command. Power returning does **not**
  resume it: it sits off until a fresh command, and `why` says `NoCommand`. The
  correct policy for anything that moves, because a door that reopens on restore
  is a gift to the infiltrator.
- **`Resume`** — the command survives, and supply returning starts `restart_ticks`
  of warm-up (`Restarting { ticks }`) before it runs again.
- **`Hold`** — the command survives with no warm-up. Lights and instruments.

Three consequences fall out. A wire that was live through an outage does **not**
fire an edge when supply returns, because the signal runtime remembers the level
it saw — and a relay's cached edge bits are cleared rather than synchronised,
since an edge that was never delivered is not one to replay. A pulse interrupted
mid-flight is **not** cancelled: the mechanism cannot move while it is dark, so
the countdown and the state to restore both survive, and the door finishes
travelling when it has current again. And a rewired load starts from rest: the
rewire drops the old warm-up *and* the claim that supply was already present, so
the next tick sees a real rise and the authored delay applies again. Without
that, routing would be a way to skip the restart policy a level states.

### Physical sabotage and logical rewiring are different commands

| | `PowerCommand::CutCable` | `PowerCommand::Route` |
|---|---|---|
| What it does | severs the conductor feeding a feed | moves one load onto another feed |
| Who may | the infiltrator, standing in the feed's room — **never** the hacker | either seat that can reach the load's panel |
| Cost | busy ticks and noise | bandwidth (hacker) or busy ticks (infiltrator) |
| Reversible | **no**; the run has no splice | yes, and it also reconnects a disconnected load |
| Consequence | feed dead for the run, its breaker tripped on `ConductorCut`, every load on it `Unavailable` | its new feed's rating decides; a rewire that overrates a feed trips its protection |
| Counter-play | `Route` the loads onto the other feed | route them back, or reset the protection |

The asymmetry is the point: a cut cannot be undone remotely, so the *logical*
verb is the only answer to the *physical* one. In the signal network the
equivalent pair is `Wire`/`Cut`, which act on control rather than supply; the
installer's builtin wires there are the analogue of a cable that cannot be pulled.

`PowerNetworkDef` also validates that its ids are unique across loads, feeds,
breakers, sources and hazards, because seat access is one id space — an ambiguous
id would make "can the hacker reach this?" unanswerable.

### Preservation when the subsystem is absent

`ScenarioDefinition::power` and `RuntimeState::power` are
`#[serde(default, skip_serializing_if = "Option::is_none")]`. A floor without a
power network:

- carries no `power` key in its JSON (the Ghost Lobby golden and every recorded
  replay are unchanged),
- takes none of the new code paths (each seam early-returns),
- reports no extra validation checks,
- snapshots and restores unchanged (the run snapshot embeds `RuntimeState` whole,
  so the absent field simply is not written — no format bump, exactly as the
  signal network's arrival needed none),
- and behaves exactly as it did under `netsim`'s existing power cascade, which is
  untouched.

### Hosted in Ghost Lobby

`power::test_floor::powered_ghost_lobby()` is `ghost_lobby_wired()` plus the
power network. `Command::Power { op }` carries an index-addressed `PowerOp` (so
`Command` stays `Copy`) and is routed to the seat `op` names. Each tick, right
before the signal network, `system_power` steps the network and applies what it
holds: a load that is not operating forces its door shut, stops the vacuum, blinds
its camera and darkens its room. The signal runtime receives the inert set through
its `Readout`, so **no wire can make an inert device work** — the signal network
still owns what a device does; power owns whether it can.

## Not decided here

1. **Repairs and splices.** A cut conductor is permanent for the run. A costed
   `Repair` (spare cable, hands, time) is the obvious next verb.
2. **Voltage, inrush, motor start, selective coordination.** Deliberately absent.
3. **Netplay role table.** burble's relay routes by `cmd` tag; its role table
   needs `Power` before power works over the network, exactly as `Signal` did.
4. **Nickel schema** (`config/power-schema.ncl`) and `bad_*.ncl` fixtures for
   `just config-scenario-check`.
5. **Frontends.** No TUI view and no Bevy overlay for the boards yet; the
   `PowerEvent` stream and `LoadReport` are the surface they will read.
6. **Real wiring for real floors.** The fixture is a test floor, not level design.

## Consequences

- The hacker gains selective, legible control of the building's supply —
  isolate a board, ride a UPS window, trip protection on purpose — and cannot use
  any of it to make a dead device obey.
- The infiltrator gains physical sabotage with a permanent consequence, and a
  reason to walk into the plant room.
- A device being off becomes a sentence a player can read, which is the difference
  between a puzzle and a bug report.
- The three power models now agree in one direction: `netsim` says who is fed from
  what, `power` says what that is worth, and `signal` says what anybody does with
  it — and the last can no longer contradict the first two.
