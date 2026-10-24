<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# Infrastructure M0 — acceptance tests, written first

**Status: specification. These tests do not exist in the tree and have never been
compiled or run.** They are written before the M0 slice precisely so that the
slice has a target, and they are the authority for what M0 must do.

Why they are here rather than in `crates/idaptik-core/tests/`: this environment
has no `cargo`, `rustc` or `rustfmt`, and two CI jobs on the current pull request
are green and must stay green — `fmt (rustfmt --check)` and
`test-ghost (core/tui/ffi + clippy + fmt)`. Hand-written Rust that cannot be
compiled *or* formatted would turn both red, which is a worse outcome than a
test spec the implementer can paste in and run. Dropping them into the crate is
the first commit of M0, and it must come with a toolchain present.

`docs/DOCUMENTATION-CLAIMS.md` applies: nothing below may be described as
passing until it has been run.

---

## 1. What M0 is

M0 is **definition, validation and load. No runtime.** It lands:

- `ScenarioDefinition.infra: Option<InfrastructureDefinition>`;
- the `idaptik-infrastructure/1` payload and its serde;
- `InfrastructureDefinition::validate() -> InfraValidationReport`;
- the loader seam, refusing a bad payload with a typed error.

No `InfrastructureSim`, no `system_infrastructure`, no signal bridge. Anything
that needs the evaluator — `rules.cited_if_reachable`, `objectives.reachable` —
is **M1**, and is marked deferred in the tables below rather than pretended.

### 1.1 Where `infra` hangs — a correction

The design document's §10.1 shows `infra` on `BuildingDefinition`, beside a
`signal` field it also shows there. **That is wrong about the code**: `signal`
lives on `ScenarioDefinition` (`crates/idaptik-core/src/scenario/definition.rs`),
and `BuildingDefinition`
(`crates/idaptik-core/src/scenario/building/mod.rs`) has no `signal` field at
all.

M0 therefore puts `infra` where the precedent actually is:

```rust
pub struct ScenarioDefinition {
    // … unchanged fields …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<SignalNetworkDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub infra: Option<InfrastructureDefinition>,
}
```

Reasons, in order: it is the seam that exists and is tested; the plant is
per-floor content whose compartments are anchored to scenario rooms, exactly as
the signal network is; and the cross-layer references (`circuits` on the
building, `node` on the grounded graph) are resolved *through* the scenario at
validation time, which is how `signal` already handles its own.

The design doc, ADR-0013 and both level excerpts are corrected to match — see
§6 of this document and the commit that follows.

---

## 2. Harness

### 2.1 Fixtures

Two committed fixtures, each a whole scenario payload, not a fragment:

```
crates/idaptik-core/tests/fixtures/infra_sump.json     // level 1, "Sump"
crates/idaptik-core/tests/fixtures/infra_sable.json    // level 2, "Sable"
```

and, in the test module, `include_str!` plus a parse helper. The negative tests
are **mutations of the valid fixture**, applied in code, so that a validator
change breaks one named test rather than an unreadable diff in a fixture nobody
reads:

```rust
fn sump() -> ScenarioDefinition {
    serde_json::from_str(INFRA_SUMP_JSON).expect("the level 1 fixture parses")
}

/// Apply one mutation and return the first typed error the validator reports.
fn rejected(mutate: impl FnOnce(&mut InfrastructureDefinition)) -> InfraValidationError {
    let mut def = sump();
    let infra = def.infra.as_mut().expect("the fixture ships an infra block");
    mutate(infra);
    infra
        .validate()
        .ok()
        .expect_err("the mutation must be refused")
        .into_iter()
        .next()
        .expect("a refusal carries at least one typed error")
}

/// The names of the checks that failed, in report order.
fn failed_checks(infra: &InfrastructureDefinition) -> Vec<&'static str> {
    infra
        .validate()
        .checks
        .iter()
        .filter(|c| !c.passed)
        .map(|c| c.id)
        .collect()
}
```

`Check` and `ValidationReport` are the existing shapes; `InfraValidationReport`
mirrors them so that a failure names `compartments.unique_ids` and not just
"invalid".

### 2.2 The two-level assertion

Every negative test asserts **both** that the named check failed and that the
typed error came back, because the two serve different readers: the check id is
the author-facing message, the error is the API. A test that asserts only the
error cannot catch a check being silently dropped from the report.

---

## 3. Positive tests

```rust
//! M0 acceptance tests for the infrastructure layer. Compile with a toolchain;
//! see docs/architecture/infrastructure-m0-acceptance-tests.md.

use idaptik_core::scenario::infrastructure::{
    INFRA_FORMAT, InfraValidationError, InfrastructureDefinition,
};
use idaptik_core::scenario::{GHOST_LOBBY_JSON, ScenarioDefinition, ghost_lobby};

const INFRA_SUMP_JSON: &str = include_str!("fixtures/infra_sump.json");
const INFRA_SABLE_JSON: &str = include_str!("fixtures/infra_sable.json");

#[test]
fn the_canonical_ghost_lobby_carries_no_infra_block() {
    let def: ScenarioDefinition =
        serde_json::from_str(GHOST_LOBBY_JSON).expect("the golden parses");
    assert!(def.infra.is_none(), "the canonical floor has no plant");
    assert_eq!(def, ghost_lobby(), "adding the field changed nothing");
}

#[test]
fn a_floor_without_infra_serialises_byte_identically() {
    let def: ScenarioDefinition =
        serde_json::from_str(GHOST_LOBBY_JSON).expect("the golden parses");
    let round = serde_json::to_string_pretty(&def).expect("serialises");
    assert_eq!(
        round.trim(),
        GHOST_LOBBY_JSON.trim(),
        "an absent infra block must not appear in the golden"
    );
}

#[test]
fn the_ghost_lobby_golden_is_unchanged_by_the_new_field() {
    // Pinned digest of the committed golden, so a serde mistake shows up as a
    // digest mismatch rather than as a mysterious replay failure later.
    assert_eq!(
        fnv_hex(GHOST_LOBBY_JSON),
        GHOST_LOBBY_GOLDEN_DIGEST,
        "if this moves, the default path changed and M0 broke byte-parity"
    );
}

#[test]
fn the_sump_fixture_loads_and_validates() {
    let def = sump();
    let infra = def.infra.as_ref().expect("infra present");
    assert_eq!(infra.format, INFRA_FORMAT);
    assert_eq!(
        failed_checks(infra),
        Vec::<&str>::new(),
        "the level 1 fixture must be valid"
    );
}

#[test]
fn the_sable_fixture_loads_and_validates() {
    let def: ScenarioDefinition =
        serde_json::from_str(INFRA_SABLE_JSON).expect("the level 2 fixture parses");
    let infra = def.infra.as_ref().expect("infra present");
    assert_eq!(failed_checks(infra), Vec::<&str>::new());
}

#[test]
fn an_infra_definition_survives_a_serde_round_trip() {
    let def = sump();
    let text = serde_json::to_string_pretty(&def).expect("serialises");
    let again: ScenarioDefinition = serde_json::from_str(&text).expect("re-parses");
    assert_eq!(def, again, "the payload is a fixed point of serde");
}

#[test]
fn level_two_cites_strictly_more_rules_than_level_one() {
    let one = sump().infra.expect("infra");
    let two: ScenarioDefinition =
        serde_json::from_str(INFRA_SABLE_JSON).expect("parses");
    let two = two.infra.expect("infra");
    let one_ids: BTreeSet<_> = one.rules.iter().collect();
    let two_ids: BTreeSet<_> = two.rules.iter().collect();
    assert!(
        one_ids.is_subset(&two_ids),
        "every rule level 1 cites must also be cited by level 2"
    );
    assert!(two_ids.len() > one_ids.len(), "level 2 adds ventilation");
}

#[test]
fn a_level_may_not_cite_a_cross_cutting_rule() {
    // X-1..X-4 are engine machinery, always active, never cited (§10.1 of the
    // catalogue). Citing one is an author error, not a silent no-op.
    let mut def = sump();
    def.infra
        .as_mut()
        .expect("infra")
        .rules
        .push("cascade-budget".into());
    let errs = def.infra.as_ref().expect("infra").validate().ok().expect_err("refused");
    assert_eq!(errs[0], InfraValidationError::RuleIsCrossCutting { rule: "cascade-budget".into() });
}
```

`fnv_hex` and `GHOST_LOBBY_GOLDEN_DIGEST` are the digest helper and constant the
test module introduces in the same commit; if the repository would rather reuse
an existing helper, use that instead — the assertion is what matters, not the
hash function.

---

## 4. Negative tests, one per validation check

Each test names the check from §14 of the design document. The mapping is
deliberately one-to-one so a check that loses its test is obvious.

| Check | Test |
|---|---|
| `infra.format` | `an_unknown_infra_format_is_refused` |
| `compartments.unique_ids` | `duplicate_compartment_ids_are_refused` |
| `compartments.room_anchor` | `a_roomless_compartment_must_be_exterior` |
| `compartments.rooms_disjoint` | `two_compartments_may_not_share_a_room` |
| `links.resolve` | `a_link_to_a_missing_compartment_is_refused` |
| `links.gate_resolves` | `a_gate_naming_a_missing_portal_is_refused` |
| `links.actor_and_aperture` | `a_passable_link_needs_clearance` |
| `entities.resolve` | `an_entity_in_a_missing_compartment_is_refused` |
| `entities.capability_targets` | `a_capability_naming_a_missing_target_is_refused` |
| `entities.signal_role` | `a_sensor_bound_to_an_actuator_is_refused` |
| `entities.node_resolves` | `a_node_the_grounded_graph_lacks_is_refused` |
| `entities.plinth_within_volume` | `a_plinth_above_the_compartment_is_refused` |
| `circuits.resolve` | `a_powered_entity_naming_a_missing_circuit_is_refused` |
| `gases.resolve` | `a_rule_naming_a_missing_gas_is_refused` |
| `gases.unsupported_buoyancy` | `a_buoyant_species_is_refused_in_v1` |
| `materials.resolve` | `an_entity_naming_a_missing_material_is_refused` |
| `controllers.resolve` | `a_controller_naming_a_missing_signal_is_refused` |
| `controllers.law_matches_reading` | `a_threshold_law_on_an_unreported_quantity_is_refused` |
| `rules.unique_ids` | `duplicate_rule_ids_are_refused` |
| `rules.resolve` | `a_level_citing_an_unknown_rule_is_refused` |
| `rules.conditions_resolve` | `a_condition_naming_a_missing_subject_is_refused` |
| `rules.no_self_schedule` | `a_rule_may_not_schedule_itself` |
| `rules.tests_present` | `a_rule_without_a_test_id_is_refused` |
| `rules.irreversible_has_recovery` | `an_irreversible_effect_needs_a_recovery_path` |
| `rules.cited_if_reachable` | **deferred to M1** — needs the evaluator |
| `objectives.reachable` | **deferred to M1** — needs the evaluator |

```rust
#[test]
fn an_unknown_infra_format_is_refused() {
    let mut def = sump();
    def.infra.as_mut().expect("infra").format = "idaptik-infrastructure/2".into();
    let errs = def.infra.as_ref().expect("infra").validate().ok().expect_err("refused");
    assert!(matches!(
        errs[0],
        InfraValidationError::UnsupportedFormat { .. }
    ));
}

#[test]
fn duplicate_compartment_ids_are_refused() {
    let err = rejected(|infra| {
        let mut dup = infra.compartments[0].clone();
        dup.floor_cm2 += 1;
        infra.compartments.push(dup);
    });
    assert!(matches!(err, InfraValidationError::DuplicateCompartment(id) if id.as_str() == "plant"));
}

#[test]
fn a_roomless_compartment_must_be_exterior() {
    let err = rejected(|infra| {
        infra
            .compartments
            .iter_mut()
            .find(|c| c.id.as_str() == "lobby")
            .expect("lobby")
            .room = None;
    });
    assert_eq!(err, InfraValidationError::RoomlessCompartmentNotExterior(
        "lobby".into()
    ));
}

#[test]
fn two_compartments_may_not_share_a_room() {
    let err = rejected(|infra| {
        infra.compartments[1].room = infra.compartments[0].room.clone();
    });
    assert!(matches!(err, InfraValidationError::RoomShared { .. }));
}

#[test]
fn a_link_to_a_missing_compartment_is_refused() {
    let err = rejected(|infra| infra.links[0].b = "nowhere".into());
    assert!(matches!(err, InfraValidationError::LinkEndpointMissing { .. }));
}

#[test]
fn a_gate_naming_a_missing_portal_is_refused() {
    let err = rejected(|infra| {
        infra.links[0].gate = Some(GateRef::Portal("door-none".into()));
    });
    assert!(matches!(err, InfraValidationError::GateMissing { .. }));
}

#[test]
fn a_passable_link_needs_clearance() {
    let err = rejected(|infra| {
        infra.links[0].actor = ActorPassage::Passable;
        infra.links[0].clearance_mm = 0;
    });
    assert!(matches!(err, InfraValidationError::PassableWithoutClearance(_)));
}

#[test]
fn an_entity_in_a_missing_compartment_is_refused() {
    let err = rejected(|infra| infra.entities[0].compartment = "nowhere".into());
    assert!(matches!(err, InfraValidationError::EntityCompartmentMissing { .. }));
}

#[test]
fn a_capability_naming_a_missing_target_is_refused() {
    let err = rejected(|infra| {
        infra.entities[0].caps = vec![Capability::Gates(GatesCfg {
            flow: GateTarget::Link("no-such-link".into()),
            positions: vec![GatePosition::Open, GatePosition::Shut],
        })];
    });
    assert!(matches!(err, InfraValidationError::CapabilityTargetMissing { .. }));
}

#[test]
fn a_sensor_bound_to_an_actuator_is_refused() {
    let err = rejected(|infra| infra.entities[0].signal = Some("sig-gd1".into()));
    assert_eq!(err, InfraValidationError::SignalRoleMismatch {
        entity: "pump-p1".into().into(),
        expected: SignalRole::Actuator,
    });
}

#[test]
fn a_node_the_grounded_graph_lacks_is_refused() {
    let err = rejected(|infra| infra.entities[0].node = Some("no-such-node".into()));
    assert!(matches!(err, InfraValidationError::NodeMissing { .. }));
}

#[test]
fn a_plinth_above_the_compartment_is_refused() {
    // The plant is 3 800 L of air over 16 000 cm², so water can never stand
    // deeper than 2 375 mm in it; a 3 000 mm plinth is unreachable by construction.
    let err = rejected(|infra| infra.entities[0].init.plinth_mm = 3_000);
    assert_eq!(
        err,
        InfraValidationError::PlinthAboveCompartment {
            entity: "pump-p1".into().into(),
            plinth_mm: 3_000,
            full_depth_mm: 2_375,
        }
    );
}

#[test]
fn a_powered_entity_naming_a_missing_circuit_is_refused() {
    let err = rejected(|infra| {
        infra.entities[0].caps = vec![Capability::Powered(PoweredCfg {
            circuit: "Z".into(),
            load: 1,
        })];
    });
    assert!(matches!(err, InfraValidationError::CircuitMissing { .. }));
}

#[test]
fn a_rule_naming_a_missing_gas_is_refused() {
    let err = rejected(|infra| {
        infra.gases.clear();
    });
    assert!(matches!(err, InfraValidationError::GasMissing { .. }));
}

#[test]
fn a_buoyant_species_is_refused_in_v1() {
    let mut def: ScenarioDefinition =
        serde_json::from_str(INFRA_SABLE_JSON).expect("parses");
    let infra = def.infra.as_mut().expect("infra");
    infra.gases[0].density_vs_air_permille = 900;
    let errs = infra.validate().ok().expect_err("refused");
    assert!(matches!(
        errs[0],
        InfraValidationError::UnsupportedBuoyancy { .. }
    ));
}

#[test]
fn an_entity_naming_a_missing_material_is_refused() {
    let err = rejected(|infra| infra.entities[0].material = "unobtainium".into());
    assert!(matches!(err, InfraValidationError::MaterialMissing { .. }));
}

#[test]
fn a_controller_naming_a_missing_signal_is_refused() {
    let mut def: ScenarioDefinition =
        serde_json::from_str(INFRA_SABLE_JSON).expect("parses");
    let infra = def.infra.as_mut().expect("infra");
    infra.controllers[0].reads = Some("sig-nope".into());
    let errs = infra.validate().ok().expect_err("refused");
    assert!(matches!(errs[0], InfraValidationError::ControllerReadMissing { .. }));
}

#[test]
fn a_threshold_law_on_an_unreported_quantity_is_refused() {
    // `ctl-vent` reads a gas detector; pointing it at the isolator's integrity
    // sensor is an author error, not a law that silently never fires.
    let mut def: ScenarioDefinition =
        serde_json::from_str(INFRA_SABLE_JSON).expect("parses");
    let infra = def.infra.as_mut().expect("infra");
    infra.controllers[0].reads = Some("sig-iso".into());
    let errs = infra.validate().ok().expect_err("refused");
    assert!(matches!(
        errs[0],
        InfraValidationError::ControllerLawMismatch { .. }
    ));
}

#[test]
fn duplicate_rule_ids_are_refused() {
    let mut def: ScenarioDefinition =
        serde_json::from_str(INFRA_SABLE_JSON).expect("parses");
    let infra = def.infra.as_mut().expect("infra");
    infra.rules.push(infra.rules[0].clone());
    let errs = infra.validate().ok().expect_err("refused");
    assert!(matches!(errs[0], InfraValidationError::DuplicateRule(_)));
}

#[test]
fn a_level_citing_an_unknown_rule_is_refused() {
    let err = rejected(|infra| infra.rules.push("make-it-up".into()));
    assert_eq!(
        err,
        InfraValidationError::UnknownRule { rule: "make-it-up".into() }
    );
}

#[test]
fn a_condition_naming_a_missing_subject_is_refused() {
    // A rule body is only reachable through the catalogue, so the mutation is
    // applied to the embedded rule, not to the citation list.
    let err = rejected(|infra| {
        let rule = infra
            .embedded_rules
            .iter_mut()
            .find(|r| r.id.as_str() == "wet-arc")
            .expect("wet-arc is embedded in level 1");
        rule.when[0] = Condition::Wetness {
            entity: "no-such-thing".into(),
            op: Compare::Ge,
            permille: 350,
        };
    });
    assert!(matches!(err, InfraValidationError::ConditionSubjectMissing { .. }));
}

#[test]
fn a_rule_may_not_schedule_itself() {
    let err = rejected(|infra| {
        let rule = infra
            .embedded_rules
            .iter_mut()
            .find(|r| r.id.as_str() == "wet-arc")
            .expect("wet-arc");
        rule.delay_ticks = 0;
        rule.period_ticks = Some(0);
    });
    assert!(matches!(err, InfraValidationError::SelfSchedulingRule { .. }));
}

#[test]
fn a_rule_without_a_test_id_is_refused() {
    let err = rejected(|infra| {
        infra
            .embedded_rules
            .iter_mut()
            .find(|r| r.id.as_str() == "wet-arc")
            .expect("wet-arc")
            .tests
            .clear();
    });
    assert!(matches!(err, InfraValidationError::RuleWithoutTest { .. }));
}

#[test]
fn an_irreversible_effect_needs_a_recovery_path() {
    let err = rejected(|infra| {
        let rule = infra
            .embedded_rules
            .iter_mut()
            .find(|r| r.id.as_str() == "wet-arc")
            .expect("wet-arc");
        rule.recovery = Recovery::None;
    });
    assert!(matches!(
        err,
        InfraValidationError::IrreversibleWithoutRecovery { .. }
    ));
}
```

### 4.1 Notes that keep these honest

- `rejected(...)` takes the **first** error. The validator's report order is
  itself a test (below), so "first" is a stable claim and not an accident of
  `HashMap` iteration.
- Where a mutation needs the *rule body*, the test mutates
  `embedded_rules`. That is a real design consequence, not a convenience: the
  catalogue is data, so a level carries resolved rule bodies, and validation can
  check them. §2 of the M0 loader document explains what `embedded_rules` is and
  why it is not a `rules` list of ids.
- `a_buoyant_species_is_refused_in_v1` and the two controller tests use the
  Sable fixture because they need a gas and a controller, which level 1 does not
  have.

---

## 5. Report-shape tests

```rust
#[test]
fn every_check_reports_exactly_once_and_in_a_fixed_order() {
    let def = sump();
    let infra = def.infra.as_ref().expect("infra");
    let first = failed_checks(infra);
    let second = failed_checks(infra);
    assert_eq!(first, second, "validation is a pure function of the input");
    let report = infra.validate();
    let mut ids: Vec<_> = report.checks.iter().map(|c| c.id).collect();
    let sorted = {
        let mut s = ids.clone();
        s.sort_unstable();
        s
    };
    assert_eq!(ids, sorted, "checks are reported in name order");
    ids.dedup();
    assert_eq!(ids.len(), report.checks.len(), "no check appears twice");
}

#[test]
fn validation_reports_every_failure_not_just_the_first() {
    let mut def = sump();
    let infra = def.infra.as_mut().expect("infra");
    infra.format = "wrong".into();
    infra.compartments[1].id = infra.compartments[0].id.clone();
    let errs = infra.validate().ok().expect_err("refused");
    assert!(
        errs.len() >= 2,
        "an author fixing a level should see everything at once, got {errs:?}"
    );
}

#[test]
fn a_valid_definition_produces_a_report_with_no_failures() {
    let def = sump();
    let report = def.infra.as_ref().expect("infra").validate();
    assert!(report.ok().is_ok(), "the fixture is the positive control");
    assert!(
        report.checks.iter().all(|c| c.passed),
        "the positive control must pass every check, not merely avoid errors"
    );
}
```

The positive-control test is the one that catches the failure mode this whole
document exists to prevent: a validator that passes everything because it
checked nothing. If `validate()` returns an empty `checks` vector, `ok()` is
`Ok` and the fixture loads — so the test asserts that the checks *ran*.

---

## 6. Documents that must change with M0

These are corrections, not additions. They land in the commit that follows
this test document, so that the tests are committed against the shape they
actually pin rather than against a shape the documents still contradict.

| File | Change |
|---|---|
| `infrastructure-interactions.md` §10.1 | `infra` on `ScenarioDefinition`, beside `signal`; the `BuildingDefinition` example is wrong and is replaced |
| ADR-0013 §"Decision", item 1 | same: the seam is the scenario's, as `signal` is |
| `infrastructure-level-1.md` §9 | the excerpt is a scenario payload: `scenario_id`/`floor_id`, not `building_id`/`entry`/`circuits`. Circuits stay on the building and are referenced, not redeclared |
| `infrastructure-level-2.md` §10 | same |
| `infrastructure-interactions.md` §14 | mark `rules.cited_if_reachable` and `objectives.reachable` **deferred to M1**, with the reason |

---

## 7. What M0 does not test

Runtime behaviour, deliberately: no staged tick, no water, no water front
budget, no cascade bound, no signal hop. Those are `infrastructure.rs` in M1 and
the test index in §15 of the design document is their home.

No test here asserts a duration in ticks, because nothing here runs a tick.
