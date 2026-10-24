# ADR-0013: Infrastructure interactions are composable rules over typed capabilities

- Status: Proposed
- Date: 2026-10-24
- Relates to: ADR-0004 (deterministic event-sourced sim), ADR-0007 (UMS package
  boundary), ADR-0012 (signal network), `docs/level-design.md`,
  `docs/architecture/infrastructure-interactions.md`,
  `docs/architecture/infrastructure-rule-catalogue.md`

## Context

The building runtime already models **space** (`BuildingDefinition`: floors,
rooms, portals), **reach** (`netsim::GroundedGraph`) and **boolean consequence**
(`SignalNetworkDef`, ADR-0012). What it does not model is the **plant**: the
pipes, cables, sumps, fans, valves and drains that make a building behave like a
building. Today the closest thing is a per-node `Actuation` (`CutPower`,
`KillLights`, `HoldDoor`): one verb, one fixed effect, no state, no physical
quantity, and no interaction between systems.

The consequence is that a designer cannot express the interactions the game
actually wants — water reaching a live motor, a pump losing power while water
keeps entering, a duct that stops carrying air once it floods — without
hard-coding a special case per pair of object names. That does not scale past
the first level, and it cannot be authored by UMS, which sees only data.

Three existing decisions constrain the answer:

- **Determinism** (ADR-0004): a run is a pure function of
  `(definition, config, seed, command stream)`. Anything added must be
  order-independent and must not consume RNG.
- **The signal network** (ADR-0012): outputs are read from state at the start of
  a tick, inputs then change state, and a signal crosses exactly one wire per
  tick. That propagation contract is load-bearing and tested.
- **The tick** (`crates/idaptik-bevy/src/driver.rs` at 60 Hz, `TICK_DT` in
  `scenario/mathf.rs`, the twelve systems in `scenario/sim/systems.rs`):
  time is accumulated, never recomputed, and system order is a contract.

## Decision

Add a third, optional sub-network to the building model — the **infrastructure
layer** — as a deterministic rule engine over **typed capabilities**, not over
object names.

1. **Data, not code.** An `InfrastructureDefinition` is attached to
   **`ScenarioDefinition`** as `infra: Option<…>`, `skip_serializing_if =
   "Option::is_none"`, exactly as `signal` already is — the same struct, the
   same seam. (An earlier draft of this ADR said `BuildingDefinition`; that
   type has no `signal` field, and the code is the authority.) Absent means
   absent: existing goldens and replays stay byte-identical.

2. **Entities declare capabilities; rules match capabilities.** A pump is not
   "a pump" to the rules. It `Moves { medium: Water, rate_ml_per_s, check_valve, return_ml }`;
   a fan `Moves { medium: Gas, … }`; a valve `Gates { flow }`; a heater
   `Ignites { HotSurface }`. Rules match `(template, capability, material
   property, gas class)` and never `EntityKind` or an entity id. §4 of the
   catalogue lists the eight closed templates; §3 fixes the vocabularies.

3. **No universal physics.** Compartments exchange a bounded set of scalar
   quantities (water ml, gas ml and ‰, wetness ‰, integrity ‰, fire ‰, power
   boolean per circuit) by authored rates and clamps. There is no pressure
   field, no chemistry, no thermodynamics and no fluid solver.

4. **Integer quantities, no RNG.** All replicated quantities are integers;
   derived values are recomputed in a fixed order. Nothing in the layer draws
   from the RNG, so replays cannot drift.

5. **A staged tick.** Five phases per tick — sense, intent, resolve, commit,
   aftermath — with at most three waves, a per-tick delta budget, and a
   **deferral queue** rather than a silent drop when the budget is reached.
   Flows are computed from the sense snapshot and applied antisymmetrically, so
   entity iteration order cannot change the result.

6. **A precedence ladder.** `LockOut (0) > Manual (1) > Interlock (2) >
   Control (3) > Remote (4) > Default (5)`. Manual isolation therefore beats
   remote operation by construction, not by a special case.

7. **Preserved signal semantics.** The plant contributes facts to
   `signal_readout()` and consumes a signal device's state; it never adds a hop
   inside a tick. The one vocabulary addition is `SignalKind::Plant`, carried by
   `idaptik-signal/2`, and it changes no propagation rule (§11 of the design).

8. **Two observable surfaces per outcome.** Every rule names a player cue and a
   guard-observable evidence item. Developer telemetry is a third view over the
   same causal event stream: no outcome exists only in the log.

## Alternatives considered

| Option | Why not |
|---|---|
| A per-pair interaction table (`water × pump`, `water × fan`, …) | O(n²) authored outcomes, unmaintainable, and exactly the hard-coding the brief forbids. Names leak into logic; renaming content changes behaviour. |
| A general ECS physics/chemistry framework | Far beyond the game's needs, floats would threaten determinism, and the design surface becomes unreviewable. The layer would become the product. |
| Carry quantities on the signal network | The signal network is boolean by design (ADR-0012), and adding scalars would change its propagation contract. A wire would stop meaning "a bool crosses one hop per tick". |
| Extend `netsim::Actuation` with plant verbs | `Actuation` is a closed, `Copy` enum; adding payloads breaks `Copy`, adding variants breaks reach/effect semantics and the graph's role as a *reach* model. Plant actuation belongs above it. |
| Hard-code the two test levels in `idaptik-core` | Goldens diverge per level, UMS cannot author, and the rules cannot be reused by a third level. |
| Keep plant state out of snapshots (recompute on restore) | Leaks, wetness, gas and fire are path-dependent. Recomputing them silently changes a restored run. |

## Consequences

- Content cost moves into authored data: materials, gases, compartments, links,
  entities, rules and two levels. The engine stays small.
- A designer gets legible cause and effect, and a reviewer gets a rule id and a
  test id per interaction.
- The building runtime gains a fixed tick. `BuildingSim::apply` stays the
  command entry; `tick` adds the per-tick subsystems in the order defined in
  §10 of the design. That is a real change to a currently event-only runtime and
  is called out as such rather than smuggled in.
- `idaptik-signal/2` is a second format version in the estate. It is additive,
  it loads v1 unchanged, and §11.4 records the no-change fallback if the
  maintainers refuse it.
- The rule catalogue is long. That length is the point: the brief asks for
  eleven named properties per interaction, and a rule that cannot fill all
  eleven is underspecified.

## Implementation status

**None of this is implemented.** This ADR and the two documents it references
are a design of record: there is no `scenario/infrastructure` module, no
`SystemInfrastructure`, no `SignalKind::PlantActuator`/`PlantSensor`, and none of the test names in
the catalogue exist yet. They are written as specifications precisely so that
the implementation can be checked against them.

The slices, in dependency order:

| Slice | Lands | Gate |
|---|---|---|
| M0 | `InfrastructureDef` + validator + `idaptik-infrastructure/1` loader; no runtime | validation fixture suite, negative fixtures |
| M1 | `InfrastructureSim` with quantities, staged deltas, precedence, cascade budget; level 1 authored | determinism and ordering tests (§15) |
| M2 | `system_infrastructure` in `GhostLobbySim`; `BuildingSim::tick` in the building host | `signal_hop_count_unchanged_*`, replay equivalence |
| M3 | signal bridge: `SignalKind::PlantActuator` / `PlantSensor`, `idaptik-signal/2`, sensor/actuator rules | v1 golden byte-identity, bridge tests |
| M4 | level 2: ventilation, `sable-2`, detector, spoofing, fire | gas/fire/control tests |
| M5 | developer trace surface (`idaptik-infra-trace/1`) and TUI overlay | golden trace fixtures |

No slice may land with a rule that has no test id, and no test may be added that
reports success without asserting on state.
