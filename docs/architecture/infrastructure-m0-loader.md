<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# Infrastructure M0 — the loader slice, designed

**Status: design of record. Not implemented.** This document answers the
questions the [M0 acceptance tests](infrastructure-m0-acceptance-tests.md)
raise, and nothing else. Where it disagrees with
[`infrastructure-interactions.md`](infrastructure-interactions.md), that
document is corrected in the same commit; the tests and this design are the
narrower, more recent statement of M0.

---

## 1. Scope

M0 lands definition, validation and load. It runs no ticks and produces no
events. Concretely, four things:

1. the `InfrastructureDefinition` payload and its serde;
2. `InfrastructureDefinition::validate() -> InfraValidationReport`;
3. the seam that lets a scenario payload carry one, and refuses a bad one;
4. two committed fixtures that load and validate.

M0 does **not** land `InfrastructureSim`, `system_infrastructure`, the signal
bridge, or any rule actually firing. A rule in M0 is data that can be validated
and that cannot yet run.

---

## 2. Two corrections M0 forces

### 2.1 `infra` hangs off the scenario, not the building

[`infrastructure-interactions.md`](infrastructure-interactions.md) §10.1 shows:

```rust
pub struct BuildingDefinition {
    pub signal: Option<SignalNetworkDef>,   // ← does not exist
    pub infra: Option<InfrastructureDefinition>,
}
```

`BuildingDefinition` has no `signal` field. `signal` is on `ScenarioDefinition`
(`crates/idaptik-core/src/scenario/definition.rs`), added by ADR-0012 with the
comment "Absent on the canonical Ghost Lobby, so its golden and every replay are
unchanged."

M0 follows the code:

```rust
pub struct ScenarioDefinition {
    // … unchanged …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<SignalNetworkDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub infra: Option<InfrastructureDefinition>,
}
```

The consequence is worth stating plainly, because it changes what a level *is*:
a level is a scenario payload, and its plant travels with it. `BuildingDefinition`
keeps owning `circuits`, `portals` and `zones`; the plant references them and
never redeclares them.

### 2.2 Two checks cannot exist before the evaluator

Design §14 lists `rules.cited_if_reachable` ("a rule whose preconditions can
arise but which the level did not cite") and `objectives.reachable` ("a search
over rules"). Both require evaluating rule preconditions over a level's state,
which is M1's `InfrastructureSim`. M0 does not implement them and does not
pretend to: they are marked deferred, and a level citing an unknown rule is still
refused by `rules.resolve`, which is the part that can be checked statically.

---

## 3. Module layout

```
crates/idaptik-core/src/scenario/infrastructure/
├── mod.rs        re-exports, INFRA_FORMAT, load_infra
├── def.rs        the authored types and their serde
├── catalogue.rs  MAT_*, GAS_*, RULE_CATALOGUE, ruleset_version
└── validate.rs   InfraValidationReport, Check ids, InfraValidationError
```

`scenario/mod.rs` gains `pub mod infrastructure;` and re-exports
`InfrastructureDefinition` alongside `SignalNetworkDef`.

The split is the one `scenario/signal/` already uses (`mod.rs`, `sim.rs`), minus
`sim.rs`, which is M1.

---

## 4. Types

Only what M0 must validate; runtime fields (`HazardState`, `ActorExposure`, …)
are M1 and are not defined here.

```rust
pub const INFRA_FORMAT: &str = "idaptik-infrastructure/1";

pub struct InfrastructureDefinition {
    pub format: String,                    // must equal INFRA_FORMAT
    pub ruleset_version: u16,
    pub materials: Vec<MaterialDef>,
    pub gases: Vec<GasDef>,
    pub compartments: Vec<CompartmentDef>,
    pub links: Vec<FluidLinkDef>,
    pub entities: Vec<EntityDef>,
    #[serde(default)]
    pub controllers: Vec<ControllerDef>,
    pub rules: Vec<RuleId>,                // citations, never bodies — §5
    #[serde(default)]
    pub budgets: CascadeBudgets,
}
```

`CompartmentDef`, `FluidLinkDef`, `EntityDef`, `Capability`, `ControllerDef` and
`RuleDef` are as specified in design §2.1–§2.4; M0 adds nothing to them except
the derives and the serde tags in §5 below. `CascadeBudgets` gets `Default` so a
level that does not care can omit it.

---

## 5. The rule catalogue: cited, not embedded

Design §10.1 of the catalogue says a level's `rules` array is a list of ids. M0
honours that literally, and it has a consequence the tests depend on:

- **the rule bodies live in the engine** (`catalogue.rs`, one `RULE_CATALOGUE`
  of the 36 catalogue entries at `ruleset_version`), not in the level;
- a level cites ids; `rules.resolve` refuses an id the catalogue lacks;
- rule-body checks (`rules.conditions_resolve`, `rules.no_self_schedule`,
  `rules.tests_present`, `rules.irreversible_has_recovery`) run against the
  catalogue, so they cannot be broken by a level — but they must still be
  *testable*, or they are decoration.

So validation takes the catalogue as a parameter:

```rust
impl InfrastructureDefinition {
    /// Validate against the built-in catalogue.
    pub fn validate(&self) -> InfraValidationReport {
        self.validate_with_rules(&RULE_CATALOGUE)
    }

    /// Validate against a supplied catalogue. Tests use this to prove each
    /// rule-body check fires; nothing else should.
    pub fn validate_with_rules(&self, catalogue: &[RuleDef]) -> InfraValidationReport;
}
```

That is the seam that lets `a_rule_without_a_test_id_is_refused` exist at all,
and it is the reason the acceptance tests mutate `Vec<RuleDef>` rather than
`infra.rules` for those four cases.

**Why not embed rule bodies in the level.** It would make the same 36 rows
appear in every level file, let two levels disagree about what `wet-arc` means,
and put the physics in content rather than in the engine — the opposite of the
brief's "typed capabilities, not per-pair special cases". If authoring new rules
from UMS becomes a requirement, that is a versioned change (`ruleset_version`
plus a catalogue extension point), not a default.

---

## 6. The serde contract

| Rule | Consequence |
|---|---|
| discriminant in `"kind"`, snake_case | matches `Command`'s tagged-union convention |
| `"self"` resolves to the enclosing subject | one rule lights on a pump on `M` and a fan on `N` |
| comparisons use `op: ge \| le \| between` plus a **named unit field** | `"permille": 350`, `"mm": 180`; no anonymous numbers |
| `#[serde(default, skip_serializing_if = "Option::is_none")]` on `infra` | an absent plant never appears in a golden |
| unknown fields are **refused** (`deny_unknown_fields`) | a typo like `sill_cm2` fails the load instead of silently defaulting |

The last one is a departure from some existing types and is deliberate for the
plant: every field here is physical, and a silently ignored one is a level that
looks authored and behaves as if it were not. `deny_unknown_fields` costs
forward-compatibility inside a payload version — which is correct, because
`INFRA_FORMAT` *is* the version.

---

## 7. Validation

### 7.1 Report shape

`InfraValidationReport` mirrors `ValidationReport`/`BuildingValidationReport`:
a `Vec<Check>` of `{ id: &'static str, passed: bool }`, plus
`ok() -> Result<(), Vec<InfraValidationError>>`.

Checks are **evaluated in full and reported in name order**, not short-circuited:
an author fixing a level should see everything wrong at once. `ok()` returns
every error, and a test asserts that two independent faults produce two errors.

Each check reports **the first violation in its family** and no more — the same
convention `ScenarioDefinition::validate` documents — so one bad field yields one
named failure rather than a flood.

### 7.2 The error enum

```rust
pub enum InfraValidationError {
    UnsupportedFormat { found: String },
    DuplicateCompartment(VolumeId),
    RoomlessCompartmentNotExterior(VolumeId),
    RoomShared { room: RoomRef, a: VolumeId, b: VolumeId },
    LinkEndpointMissing { link: LinkId, end: VolumeId },
    GateMissing { link: LinkId, gate: GateRef },
    PassableWithoutClearance(LinkId),
    EntityCompartmentMissing { entity: EntityId, compartment: VolumeId },
    CapabilityTargetMissing { entity: EntityId, target: GateTarget },
    SignalRoleMismatch { entity: EntityId, expected: SignalRole },
    NodeMissing { entity: EntityId, node: String },
    PlinthAboveCompartment { entity: EntityId, plinth_mm: u32, full_depth_mm: u32 },
    MoverEndpointMissing { entity: EntityId, end: VolumeId },
    MoverPathBroken { entity: EntityId, link: LinkId },
    CircuitMissing { entity: EntityId, circuit: CircuitId },
    GasMissing { rule: RuleId, gas: GasId },
    UnsupportedBuoyancy { gas: GasId, density_vs_air_permille: i16 },
    MaterialMissing { subject: String, material: MaterialId },
    ControllerReadMissing { controller: ControllerId, signal: SignalId },
    ControllerLawMismatch { controller: ControllerId, quantity: String },
    DuplicateRule(RuleId),
    UnknownRule { rule: RuleId },
    RuleIsCrossCutting { rule: RuleId },
    ConditionSubjectMissing { rule: RuleId, subject: String },
    SelfSchedulingRule { rule: RuleId },
    RuleWithoutTest { rule: RuleId },
    IrreversibleWithoutRecovery { rule: RuleId },
}
```

### 7.3 `entities.plinth_within_volume` is arithmetic, not a heuristic

A plinth deeper than the compartment can ever fill is unreachable content, so it
is refused. The maximum standing depth is the compartment's air column:

```
full_depth_mm = air_l × 10 000 / floor_cm2
```

which is `air_l / (floor_cm2 × 0.1)` in consistent units: a litre is 10 cm²·cm,
so one litre spread over 1 cm² is 1 000 mm of depth. For level 1's plant —
3 800 L over 16 000 cm² — that is 2 375 mm, and the pump's 150 mm plinth sits
comfortably inside it. The test asserts the boundary at 3 000 mm and pins
`full_depth_mm: 2_375` in the error, so a change to either number is visible.

---

## 8. The loader seam

Today `load_building(&str) -> Result<BuildingDefinition, BuildingPackError>`
exists for buildings and **nothing equivalent exists for scenarios** — they are
parsed by serde wherever they are needed. M0 adds the mirror, because a scenario
that carries a plant now has something that can be wrong:

```rust
pub fn load_scenario(json: &str) -> Result<ScenarioDefinition, ScenarioPackError> {
    let def: ScenarioDefinition =
        serde_json::from_str(json).map_err(|e| ScenarioPackError::Parse(e.to_string()))?;
    def.validate().ok().map_err(ScenarioPackError::Invalid)?;
    Ok(def)
}

pub enum ScenarioPackError {
    Parse(String),
    Invalid(Vec<ValidationError>),
}
```

`ScenarioDefinition::validate()` gains the delegation: when `infra` is `Some`,
its checks are folded into the report under an `infra.*` prefix, so one call
reports the floor and its plant together. A level whose plant is invalid does not
load, which is the point of M0.

### 8.1 Contract impact: none

`contracts/idaptik/v1/package.schema.json` declares `scenario` as:

```json
{
  "scenario": {
    "type": "object",
    "description": "Serialized authoritative IDApTIK ScenarioDefinition; validated semantically by the game loader."
  }
}
```

No `additionalProperties: false`, no property enumeration — the schema explicitly
defers to the loader. Adding `infra` inside the scenario payload therefore needs
**no contract version bump**, which is the one piece of good news in this slice.
The `idaptik-infrastructure/1` tag carries the infrastructure payload's own
version, and `ruleset_version` is separate again (content revision vs payload
shape).

---

## 9. Fixtures

```
crates/idaptik-core/tests/fixtures/infra_sump.json
crates/idaptik-core/tests/fixtures/infra_sable.json
```

Both are whole scenario payloads so that `load_scenario` is exercised end to end,
not just `InfrastructureDefinition`. They are transcribed from
`infrastructure-level-1.md` and `infrastructure-level-2.md` and are **the
authority once committed** — the prose may drift, the fixture may not.

That drift is the one risk this slice introduces and cannot automate without
parsing Markdown in Rust, which is not worth it. The mitigation is procedural and
stated in the level documents: change the table and the fixture in the same
commit, and let `infra_sump.json` be the thing CI loads.

---

## 10. Delivery order

| # | Lands | Test that proves it |
|---|---|---|
| 1 | `def.rs` + serde + `INFRA_FORMAT` | `the_sump_fixture_loads_and_validates`, `an_infra_definition_survives_a_serde_round_trip` |
| 2 | `ScenarioDefinition.infra` + goldens | `the_canonical_ghost_lobby_carries_no_infra_block`, `a_floor_without_infra_serialises_byte_identically` |
| 3 | `validate.rs`, structural checks | the 13 structural negatives |
| 4 | `catalogue.rs` + `validate_with_rules` | the 8 rule-body negatives |
| 5 | `load_scenario` + `ScenarioPackError` | the fixtures, and `an_unknown_infra_format_is_refused` |
| 6 | fixtures from the two level docs | `level_two_cites_strictly_more_rules_than_level_one` |

Steps 1–2 are the risky ones: they touch a type the goldens depend on. They land
first and alone, and `the_ghost_lobby_golden_is_unchanged_by_the_new_field` is
the tripwire.

M0 is done when every test in the acceptance document exists in
`crates/idaptik-core/tests/infrastructure_m0.rs`, compiles, passes, and
`test-ghost` is still green. **None of that has happened yet**, and this
environment cannot do it: no `cargo`, no `rustc`, no `rustfmt`.

---

## 11. Open questions

1. **Where the fixtures come from.** Hand-transcribed for now. A generator that
   reads the level tables would be better and needs a table parser; not in M0.
2. **`deny_unknown_fields` and UMS.** If UMS emits a newer `idaptik-infrastructure/2`
   payload into a v1 build, the load fails at parse rather than at the format
   check. That is the right failure, but the message should name the format —
   worth a follow-up.
3. **Snapshot.** `ruleset_version` must be in the M1 snapshot (§12 of the design),
   not the definition. M0 only stores it.
