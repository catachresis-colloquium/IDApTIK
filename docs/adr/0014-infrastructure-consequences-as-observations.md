# ADR-0014: Infrastructure consequences are canonical-sim facts that reach actors only as observations

- Status: Proposed
- Date: 2026-10-24
- Relates to: ADR-0004 (deterministic event-sourced sim), ADR-0005 (relay-only
  lockstep), ADR-0012 (signal network), `docs/scenarios/ghost-lobby.md`,
  `docs/scenarios/challenge-set.md`

## Context

The floor grew water, alarms, shorting electrics and a tunnelling companion,
and the brief was explicit about what they must *not* do:

- Guards must react to **observations** — darkness, unusual noise, standing
  water, equipment failing, alarms — never directly to the hidden player
  actions behind them. Nothing may make an actor omniscient; the archetype's
  existing perception (sight lines, hearing distance and threshold, room
  boundaries, hiding) stays exactly as authoritative as it was.
- The canonical simulation and its versioned command/event stream remain the
  only source of gameplay outcomes. A rendering or acceleration backend must
  never grow an independent authoritative fluid or hazard state to disagree
  with.
- Multiplayer ordering, access checks, replay and resynchronisation ride the
  existing networking model (ADR-0005): typed commands through the relay,
  lockstep folds, snapshots that carry everything a rejoining seat needs.
- Level authoring must distinguish an *intended challenge* (a locked detour,
  a loud shortcut) from *invalid content* (a dangling reference, a stranded
  objective, a destructive cut), and floors without any of this must keep
  their existing fixtures byte-identical.

Before this decision none of that machinery existed: actors perceived the
player and the camera network and nothing else; the Moletaire companion lived
in `crates/idaptik-core/src/companion/` as a ported, self-contained sim that
no scenario hosted; and there was no water, no alarms, and no way for the
building itself to be news to anybody. There is no "Evil Weevil" actor
adapter in this repository — the observation surface below is the seam any
future adapter consumes.

## Decision

### 1. Infrastructure is definition-as-data, state-is-canonical

`scenario/infra.rs` declares water zones (`fill_time`, `fail_open`, a manual
`valve_x` standing in the zone's room) and alarms (`AlarmTrip::Water { zone }`
— the level trips it; `AlarmTrip::PowerLost { watch }` — a normally-energised
tamper circuit whose watched node losing power *is* the trip). `InfraDef` is
an optional block on `ScenarioDefinition`, validated by the same
`validate()`/`ok()` report every other block walks: a zone in an unknown
room, a duplicate id, a non-positive fill time, a wheel outside its zone's
room, or an alarm watching a node the floor graph does not carry are typed
`ValidationError`s, not runtime surprises.

`InfraState` and `MoleHostState` live on `RuntimeState` as `Option`s with
`#[serde(default, skip_serializing_if)]`: **a floor without infrastructure
produces byte-identical snapshots**, so every existing golden, fixture and
replay artifact is preserved untouched.

### 2. One consequence engine, several doors in

`system_infra` (canonical sim, fixed 60 Hz timestep, no RNG) is the only
place water rises, rooms short, alarms trip and clear, and alert climbs while
a bell rings. A zone releases exactly once, first cause wins, and the cause
is typed provenance the player can read:

| Door | Path | `ReleaseCause` |
|---|---|---|
| The wheel | hold interact beside `valve_x` (`VALVE_TURN_HOLD`) | `Valve` |
| The network | `NetHack` the derived `valve-<zone>` node (`Actuation::OpenValve`) | `Valve` |
| The cascade | power loss reaches the derived `pump-<zone>` node of a `fail_open` zone — a cut aimed anywhere upstream floods the hall *by consequence* | `PumpFailure` |

All three walk the same `InfraState::release` and emit the same
`Event::WaterReleased { zone, room, cause }`. Rising water then does what the
floor's ordinary systems already do: it wades (`WATER_WADE_*` — slower, and
loud on the player's own noise meter), it trips the water alarm at
`ALARM_WATER_TRIP_LEVEL`, and at `WATER_SHORT_AT` it shorts the room's
electrics **through the ordinary power-loss path** (`Effect::DevicePowerLost`
on the room's lights, cameras, doors, valve and pump) — no bespoke consequence
code. A light losing power darkens its room and, once per episode, the whole
floor flickers and the guard is stunned, exactly as a deliberate lights kill
does.

### 3. Actors perceive; the world does not leak

`GhostLobbySim::actor_observations(at_x) -> Vec<Observation>` is the **only**
door infrastructure and companion noise have into any actor. An observation
is a kind (`Darkness`, `UnusualNoise`, `Water`, `EquipmentFailure`, `Alarm`),
a world-x to investigate, an intensity on the player-noise scale, and a room.
The actor's *own* hearing gates it — `invest_dist` and `invest_noise` from
the archetype — with alarms skipping the distance gate (that is what makes
them alarms). The player's own footstep noise is deliberately **not** an
observation kind: it travels the archetype's existing `invest_noise` hearing
path unchanged.

Billy's Assess mode consults `infra_investigation_target()`: the strongest
*heard* observation (tie-break: earlier-declared first, so it is
deterministic) pulls `last_known_x` to where the **noise** is — never to the
player's hidden position — and only when he neither sees the player nor hears
their footsteps. Observations inside his arrival distance do not re-pull (he
is already standing in them). Every new episode is announced once as
`Event::GuardObserved { kind, room }` — the player-visible record that the
guard reacted to a perception, and the latch (`guard_noticed`) lives in the
snapshot, so replay and resync agree on what has already been noticed.

The same function is the seam for future actor adapters (the "Evil Weevil"
slot): an adapter consumes `actor_observations` and nothing behind it.

### 4. The companion is hosted, not wired in sideways

A floor may declare a `MoleHostDef` (spawn, underground start, fitted
coprocessor upgrades). The sim owns a `MoletaireSim`, steps it inside the
tick (`system_mole`, after the signal network), folds its typed events into
the world as `Event::Mole { mole }`, and keeps the canonical host record —
**including the companion's RNG stream** — in `state.mole`, so snapshots,
replay determinism and resync carry it with no special cases. Orders arrive
as `Command::Mole { op }` (`MoveTo`/`Dig`/`Sabotage`/`Synth`) through the
same envelope, fold and lockstep as every other command; the seat is the
infiltrator. An order the companion cannot take is a typed
`Event::MoleDenied { reason }` (`NotHosted` on a floor without one, `Gone`,
`Unavailable`), never a swallowed no-op. A running synth decoy is canonical
state (`DiversionState`) and reaches guards as an `UnusualNoise` observation
like any other — the companion fools nobody's *senses*, only their
attention.

### 5. Rendering and acceleration backends derive; they never own

Water level, short flashes, alarm states, dead nodes and diversions are
canonical state; the events above are the stream. A frontend may interpolate
a fluid surface for display, but the *gameplay fact* — how deep, what is
shorted, what is ringing — is read from state and events. Nothing
render-derived feeds back in. (This is the repository invariant restated for
hazards; `replay_determinism` and `snapshot_equivalence` are the gates that
keep it true.)

### 6. Authoring: audit is a report, validation is a gate

`ScenarioDefinition::validate` refuses *invalid content* (the typed errors
above). `BuildingDefinition::audit` (in `scenario/building/audit.rs`) reports
on top of the validated building with two severities:

- **Error** — content that cannot be shipped: a lock controller with no
  network ground (`refs.controller_grounded`), an objective no route reaches
  (`routes.objectives_reachable`), a circuit cut that strands an objective
  that was reachable before the cut (`softlock.cut_strands_objectives`).
- **Advisory** — an intended challenge worth a second look, surfaced not
  enforced: a door portal crossing floors (`refs.door_crosses_floors`), a
  locked portal with a walkable detour (`challenge.locked_detour` — the
  lock is a cost, not a wall), a destructive cut to store rooms
  (`softlock.destructive_cut`).

Reachability walks the building graph under the *author's* semantics: locked
portals block and unpowered circuits block, and a cut is blamed only for
rooms the uncut building could actually reach. Buildings without circuits,
locks or embedded scenarios audit clean; floors without infrastructure keep
the identical six-check shape — infrastructure extended the authoring
surface, it did not fork it.

## Consequences

**Gains.** Guards acquire building awareness without a single new sense: the
same ears, gated the same way, now carry the building's own news. The flood
timeline (wade at `WATER_WADE_AT`, bell at `ALARM_WATER_TRIP_LEVEL`, short at
`WATER_SHORT_AT`) is authored data, so a floor tunes its own pressure. The
fail-open cascade makes genuinely emergent cross-system play possible — the
recorded fourth solution of the challenge set floods the hall by cutting a
grid the player aimed at the cameras' upstream, with no objective anywhere
scripting water. Determinism gates extend over all of it with no new RNG in
the consequence path (the companion's RNG is snapshot-carried like every
other stream).

**Costs.** A ringing bell climbs the existing alert meter
(`ALARM_ALERT_RATE`), so floods are a clock, not a free lever — a run in
progress can finish, but a forgotten alarm ends in lockdown. The relay's
role table is external (ADR-0006: `metadatastician/burble`): `Mole` orders
route to the infiltrator seat in this repository's envelope and lockstep
fixtures, and the pinned relay must learn `"Mole" → infiltrator` in
`command_roles()` before a live burble session carries them — tracked as an
external follow-up on the introducing PR.

**Relationship to the adjacent building systems.** Three neighbours landed on
`main` while this ADR was written, all currently sharing the 0013 slot pending
a numbering cleanup there: the power network
(`0016-electrical-infrastructure.md`), the coarse water/gas/airflow systems
(`0013-water-gas-airflow.md`) and the Evil Weevil adversarial testing adapter
(`0017-evil-weevil-adversarial-testing-adapter.md`). This ADR's water zones are
scenario-local flood machinery for driving actors; they do not compete with the
coarse water system, which moves the building's water between rooms — one owns
"this room is flooding", the other owns "where the building's water goes". The
perception contract (`actor_observations`) is the seam the neighbours should
consume: power failures already land in the same canonical `Event` log, and
surfacing them to Billy as `equipment_failure`/`darkness` observations is
named follow-up work, not a redesign. The weevil corpus carries the
observation trail today: scenario EW-06 asserts that an accepted companion
order returns to the player-visible log only as `GuardObserved`
observations, and evaluates the complete `actor_observations()` contract
where the guard stands. Its regression controls keep the original quiet,
out-of-range setup silent and distinguish companion noise from the free
`observations()` function called without `extra_noise` (issue #191).

**Alternatives considered.** Giving guards direct reads of infrastructure
state (rejected: omniscience — the brief, and the game, is about perception).
Per-backend fluid simulation with reconciliation (rejected: two authorities
is one too many; ADR-0004 makes the canonical sim the single writer).
Scripting the emergent solution as an objective trigger (rejected: an
objective trigger is a script; the point is that the cascade is a
consequence the systems produce together).
