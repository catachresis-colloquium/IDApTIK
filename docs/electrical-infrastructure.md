# Electrical infrastructure — designer's reference

The design rationale is [ADR-0016](adr/0016-electrical-infrastructure.md). This
page is the working reference: what the model distinguishes, what the numbers
mean, who may do what, and how to read a device that will not run.

Truth lives in `crates/idaptik-core/src/scenario/power/`. Nothing here is
simulated beyond what that module does.

## The six things a level can say

| Piece | Type | You author | It decides |
|---|---|---|---|
| Source | `SourceDef` | kind, `start_on`, `startup_ticks`, `remote` | whether energy exists, and how long it takes |
| Distribution | `FeedDef` | one source, `capacity`, one breaker, a room, a node | what carries it, and to where |
| Load | `LoadDef` | `draw`, `kind`, `restart`, optionally the `signal` device it *is* | what draws, and how it behaves after an outage |
| Backup | `BackupSpec` | `runtime_ticks`, `serves`, `mode` | what survives an outage, for how long |
| Protection | `BreakerDef` | `start_open`, `remote`, room, node | what opens before the cable does, and who may reset it |
| Damage | `Condition` | `intact` / `damaged` / `disconnected` | what no supply can fix |

`Unavailable` is not a condition: it is the runtime's verdict that the supply is
gone for the rest of the run (`OffReason::Unavailable`).

## The one rule

```text
operating = commanded AND energized AND intact AND not interlocked AND warmed up
```

A command is one term. Not a switch that overrules the world.

## Why is this device off?

`LoadReport` (via `PowerCommand::Inspect`, `GhostLobbySim::power_report`) reports
the **first** rung that fails, upstream to downstream from the device:

| Rung | Reason | Reads as | Fixed by |
|---|---|---|---|
| 1 | `Damaged` | damaged | nothing this run |
| 2 | `Disconnected` | disconnected | `Route` |
| 3 | `Interlock` | interlock: <label> | clearing the interlock condition |
| 4 | `Unavailable` | no supply (<feed> is severed) | a different feed |
| 5 | `Isolated` | isolated at <breaker> | a deliberate reset |
| 6 | `Unpowered` | unpowered (<feed>) | source on, breaker closed |
| 7 | `Restarting` | restarting (n ticks) | waiting |
| 8 | `NoCommand` | no command | a command |

The log line is the same sentence, e.g.
`Laundry pump is off — interlock: Laundry occupancy interlock. [pump 6/20 on feed-a]`.

## Readable numbers (defaults, at 60 Hz)

| Quantity | Default | Feels like |
|---|---|---|
| `trip_ticks` | 45 | three quarters of a second of overload before the breaker opens |
| `restart_ticks` (warm-up) | 120 | two seconds |
| Infiltrator cable cut | 120 busy ticks, noise 0.8 | two seconds of hands, and the floor hears it |
| Infiltrator rewire | 90 busy ticks | one and a half seconds at the panel |
| Infiltrator breaker | 45 busy ticks | quick, if you are standing there |
| Hacker rewire | 8 bandwidth | instant, and it costs the pipe |
| Hacker breaker reset / isolate | 6 bandwidth | instant, if the board is `remote` |
| Hacker source throw | 5 bandwidth | instant, if the source is `remote` |
| Hacker device switch | 4 bandwidth | instant, if the device has a node |
| Inspect | free | knowing why is not a resource |

Draw is a **connected rating**, not an instantaneous load: sum every load wired to
a feed and compare with `capacity`. It does not change when a device cycles, so a
player can read `13/14` off the board and reason about it. A feed is over its
rating the moment the sum exceeds it, whatever the devices are doing.

## Who may do what

| Command | Infiltrator | Hacker | Reversible | Cost |
|---|---|---|---|---|
| `CutCable` | in the feed's room | **never** — `NotPhysical` | no | hands + noise |
| `Route` | at the load, or a wired console | the load's node, and the load must have one | yes | busy / bandwidth |
| `Isolate` (open or reset) | at the board | at the board's node, **and** `remote: true` | yes | busy / bandwidth |
| `SwitchSource` | at the source | at the source's node, **and** `remote: true` | yes (a spent backup stays spent) | busy / bandwidth |
| `SetCommand` | at the load | the load's node | yes | busy / bandwidth |

Three permission contrasts are worth designing around: a **local-only board**
(`remote: false`) is reachable by the hacker and not workable by them; a **dumb
load** (no node) has no remote panel at all; and a **cable cut** is not a network
operation under any circumstances.

## Restart after power returns

Per device, required in the definition:

- `fail_safe` — the command is dropped; power returning leaves it off until asked
  again (`NoCommand`). Use for doors, lifts, anything that moves.
- `resume` — the command survives, `restart_ticks` of warm-up, then it runs again.
- `hold` — no warm-up; it resumes the tick supply returns. Lights and instruments.

Nothing replays an edge. A wire held high through an outage does not fire again
when supply returns: the runtime keeps the level it saw, and a relay's cached
edge bits are cleared rather than synchronised, because an edge that was never
delivered is not one to replay. A pulse interrupted mid-flight is not cancelled —
the mechanism cannot move while it is dark, so the countdown and the state to
restore both survive it. And a rewired load starts from rest: the rewire drops
the old warm-up and the claim that supply was already present, so the authored
delay applies again instead of being bypassed.

## Supported effects and how to reach them

| Effect | Mechanism | On the test floor |
|---|---|---|
| Selective loss of power | a feed's own breaker, source or cable | `feed-a` (plant, lighting) vs `feed-b` (security) |
| Backup power, temporary operation | a backup source serving named loads for `runtime_ticks` | `ups-hall` carries `cam-0` for 900 ticks |
| Disabled fan / pump / door / camera | a load that is not operating forces its device inert | `fan-kitchen`, `pump-laundry`, `door-d4`, `cam-0..2` |
| Protective trip | connected draw over `capacity` for `trip_ticks` | route the 6-unit pump onto `feed-b` (13/14) |
| Deliberate reset | `Isolate { open: false }`, refused while a fault stands | `brk-b`, and it refuses with `FaultPresent` |
| Exposed-power hazard | authored bounds in a room, live by rule, shock once per contact | `haz-pump` in the laundry while `feed-a` is up |

## The test floor

```
        kitchen (20..270)      hall (270..510)      office (510..795)     laundry (795..1040)      exit (1040..1260)
        ────────────────       ───────────────      ─────────────────     ──────────────────       ─────────────────
FEED-A  mains-a ── brk-a ──▶  light-kitchen        light-hall            light-office          light-laundry            light-exit
 20/20  (local board)         fan-kitchen (4)                            pump-laundry (6)  ▲hazard 920..1000
FEED-B  mains-b ── brk-b ──▶  cam-0 (3) ★UPS      light-hall            cam-1 (3)             cam-2 (3)                door-d4 (4)
 13/14  (remote)              ★ ups-hall: cam-0 only, 900 ticks, automatic
```

Two independent feeds, one backup-powered device, and exactly one feed at its
rating so that routing anything heavy onto it trips protection. Three interlocks,
permissions and hazards are all reachable in one room each.

## Deliberately not modelled

Voltage, current, impedance, inrush, motor start curves, selective coordination,
thermal ratings, cable derating, phase, and any repair of a severed conductor.
This is a game model whose only job is to make "off" legible and "on" earned.

## Where it is tested

- `crates/idaptik-core/tests/power_network.rs` — the golden, the validator, every
  rung of the ladder, selective loss, restart policy, backup runtime, trips and
  resets, exposed power, permissions and costs, the wire form and snapshots.
- `crates/idaptik-core/src/scenario/power/` — module tests for the definition and
  the runtime, including the test floor's own shape.
- `crates/idaptik-core/src/scenario/sim/power.rs` — the seam against a real
  Ghost Lobby run: an unpowered floor is unchanged, a command cannot beat a dead
  supply, the UPS window, the dead exit door, and the laundry interlock.
