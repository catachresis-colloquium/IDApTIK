<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# Infrastructure: implementation status

> This is a **checklist of what the engine does not do**. It exists because two
> designs for the same subject are both on `main`: one shipped as code in
> `crates/idaptik-core/src/scenario/infrastructure/`, one written as prose
> before that code existed. A reader who cannot tell them apart will assume the
> engine behaves as the prose describes. It does not.

**Generated against** `main` @ `7076154`. Regenerate the counts before quoting
them; they are `grep`-derivable and will drift.

---

## 1. The two designs

| | Shipped (`scenario/infrastructure/`) | Specified (ADR-0018 + docs) |
|---|---|---|
| Size | 5 files, 5 286 lines Rust | 652 + 906 + 519 + 589 lines prose |
| Rules | hard-coded fixed per-tick order | 36 authored `RuleDef`s, each cited |
| Flow | scalar `water_conductance` / `air_conductance` per opening | derived from `aperture_cm2`, `sill_mm`, head |
| Depth | not modelled | `depth_mm = water_ml * 10 / floor_cm2` |
| Capabilities | none | closed set of typed actor capabilities |
| Precedence | none | `LockOut` > automatic > manual |
| Cues | 10 `CueKind` variants | 20 named cues |
| Gas | fixed `GasClass` (`Haze`/`Hush`/`Ember`) | per-level authored species + `GasWaterRule` |
| Tests | 8 in `tests/infrastructure.rs` | 2 named levels with scripted runs |

Both are on `main`. Only the left column runs.

## 2. Identifiers promised by the design, absent from the code

`git grep -c '\b<Name>\b' -- crates/` for each. Zero means the type does not
exist anywhere in the engine.

| Identifier | Occurrences in `crates/` | Status |
|---|---|---|
| `RuleDef` | 0 | absent |
| `RuleId` | 0 | absent |
| `Capability` | 0 | absent |
| `ActorCapability` | 0 | absent |
| `AccessReq` | 0 | absent |
| `LockOut` | 0 | absent |
| `Irreversible` | 0 | absent |
| `RateLimit` | 0 | absent |
| `ReadoutRef` | 0 | absent |
| `CausalTag` | 0 | absent |
| `InfraFormat` | 0 | absent |
| `MaterialDef` | 0 | absent |
| `GasDef` | 0 | absent |
| `FluidLinkDef` | 0 | absent |
| `EntityDef` | 0 | absent |
| `ControllerDef` | 0 | absent |
| `CascadeBudgets` | 0 | absent |
| `EntityKind` | 0 | absent |

`Template`, `Scope`, `Recovery`, `Effect` and `Condition` do occur in
`crates/` (1, 1, 8, 79, 25) but as unrelated generic identifiers — the rule
layer's versions of these names do not exist. Counting them as present would
be exactly the kind of false green this document exists to prevent.

## 3. `InfrastructureDefinition` — same name, different type

| Field | Docs | Code |
|---|---|---|
| `format` | `InfraFormat` | `String` |
| `infrastructure_id` | — | `String` |
| `name` | — | `String` |
| `compartments` | ✅ | ✅ |
| `openings` / `links` | `links: Vec<FluidLinkDef>` | `openings: Vec<OpeningDef>` |
| `water` | — | `Option<WaterNetworkDef>` |
| `air` | — | `Option<AirNetworkDef>` |
| `tuning` | `budgets: CascadeBudgets` | `InfrastructureTuning` |
| `ruleset_version` | ✅ | — |
| `materials` | ✅ | — |
| `gases` | ✅ | — |
| `entities` | ✅ | — |
| `controllers` | ✅ | — |
| `rules` | ✅ | — |

Six of the ten documented fields have no counterpart. Two fields are shared
(`compartments`, and the tuning/budget slot under a different name and shape).

## 4. Cue vocabulary — 1 of 20 survives

| | Count | Members |
|---|---|---|
| Named in `infrastructure-rule-catalogue.md` §2.3 | 20 | `WaterRunning`, `WaterDrip`, `WaterSlosh`, `WaterRise`, `Hiss`, `PumpHum`, `PumpCavitation`, `VentHum`, `VentStops`, `RelayClack`, `ArcCrackle`, `BreakerTrip`, `GasReek`, `GasAlarm`, `FireWhoomph`, `Steam`, `WetFootprints`, `IsolatorTag`, `CutterGrating`, `Scraping` |
| `CueKind` in code | 10 | `Puddle`, `Spray`, `Hiss`, `Haze`, `PressureWhine`, `FilterLoaded`, `Ignition`, `Gurgle`, `Scent`, `Noise` |
| **Shared** | **1** | `Hiss` |

Consequence: `infrastructure-level-1.md` names 11 cues (0 exist) and
`infrastructure-level-2.md` names 6 (0 exist). Neither level can be built
against the shipped enum without adding 19 variants or rewriting both.

## 5. What does match

These were the parts the design got right and the code adopted — worth
recording so the document is not read as "nothing landed":

| Element | Docs | Code | Match |
|---|---|---|---|
| Gas species | `Haze` / `Hush` / `Ember`; only `Ember` ignites, only at `Critical` | `GasClass` enum, same three | ✅ |
| Cue ladder | `Trace → Warning → Critical` over `Clear` | `Tier` enum, same four | ✅ |
| Water ledger | `initial_reservoir`, `recharged`, `drained_to_exterior`, `spilled_to_exterior` | `WaterLedger`, same fields | ✅ |
| Wire format | `idaptik-infrastructure/1` | `INFRASTRUCTURE_FORMAT` | ✅ |
| Snapshot format | `idaptik-infrastructure-snapshot/1` | `INFRASTRUCTURE_SNAPSHOT_FORMAT` | ✅ |
| Determinism | integers only, no float/RNG/wall clock | enforced throughout `sim.rs` | ✅ |
| `not_simulated` disclosure | required by the design | `model_declaration()` | ✅ |

## 6. Validation — different questions, both large

The deleted `infrastructure-m0-acceptance-tests.md` specified **26 validation
checks**, one test each. The shipped validator has **83 error variants**.

They are not the same 26. The specified set asks about rules, entities,
controllers, gas species and materials — none of which exist. The shipped set
asks about compartments, openings, reservoirs, junctions, pipes, valves,
outlets, leaks, drains, ducts, fans, dampers, filters, gas sources, trace
sources and equipment — none of which the deleted document anticipated in that
detail, because the module it describes had not been designed yet.

**Net:** the shipped validator is substantially better than the specified one
at validating the model that exists, and validates none of the model that does
not. Recovering the 26 was not worth a document; this paragraph is the delta.

## 7. Tests

8 tests in `crates/idaptik-core/tests/infrastructure.rs`, all exercising the
shipped model:

| Test | Covers |
|---|---|
| `every_fixture_validates_and_declares_its_model` | validation + `model_declaration()` |
| `fixtures_round_trip_through_the_wire_format` | `idaptik-infrastructure/1` |
| `water_fixture_is_conservative_and_causal` | water ledger, causality |
| `ventilation_fixture_clears_haze_and_loads_a_filter` | air, filters, cues |
| `ember_ignites_only_while_equipment_is_live` | ignition gating on equipment state |
| `combined_fixture_couples_water_air_and_power_through_one_trip` | cross-system coupling |
| `snapshot_round_trip_preserves_the_run` | snapshot format + determinism |
| `commands_that_cannot_apply_are_denied_not_ignored` | denial vs silent ignore |

Note: these did not run in CI until `19a9b7f` (#170, 2026-10-25T01:23Z) fixed a
missing import that broke compilation of the test targets. Between #167
merging and #170, the module's tests existed but never executed.

## 8. What to do with this

- **If you are touching `scenario/infrastructure/`:** the prose does not
  describe your code. Use the module and its tests as the reference.
- **If you want the derived-depth model:** that is the one item in the prose
  that changes a shipped type's contract — `OpeningDef`'s scalar conductances.
  It needs a change request, not a documentation pass. Argue it on reviewability:
  with a scalar, depth is an authored number and cannot be reviewed; with
  geometry, it is a consequence of room size.
- **If you want the rule catalogue:** it is a 36-rule backlog. Nothing in
  `sim.rs` encodes it, and `main`'s ADR-0013 deliberately chose fixed order
  instead. Treat it as a proposal against that decision, not a spec that was
  forgotten.
- **If you are deleting documentation:** do not delete this file. It is the
  record that the two designs are different, which is the fact most likely to
  be lost.
