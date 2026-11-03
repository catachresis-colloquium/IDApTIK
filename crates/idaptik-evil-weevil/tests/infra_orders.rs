//! EW-06 regressions for issue #191.
//!
//! The corpus test proves the whole acceptance/observation trail. These
//! smaller controls keep its premises honest: the original quiet, out-of-
//! range setup must stay silent, and an audible companion diversion must be
//! read through the sim's complete perception door, not the free function
//! with its companion-noise argument omitted.

use idaptik_core::scenario::GhostLobbySim;
use idaptik_core::scenario::command::{Command, MoleOp, TickInput};
use idaptik_core::scenario::common::{BillyMode, Phase};
use idaptik_core::scenario::event::Event;
use idaptik_core::scenario::infra::{self, Hearing, ObservationKind};
use idaptik_evil_weevil::harness::load_live_script;
use idaptik_evil_weevil::scenarios::{Scenario, infra_orders::InfraOrdersScenario};

fn pump_house() -> GhostLobbySim {
    let mut script = load_live_script();
    script.scenario = Some("pump-house".to_owned());
    idaptik_tui::headless::build(&script).expect("shared fixture builds the pump-house floor")
}

fn tick(sim: &mut GhostLobbySim, commands: &[Command]) -> Vec<Event> {
    sim.tick(&TickInput {
        immediates: commands.to_vec(),
        ..TickInput::default()
    })
}

fn heard_noise(sim: &GhostLobbySim, source_x: f64) -> bool {
    sim.actor_observations(sim.state().billy.x)
        .iter()
        .any(|o| o.kind == ObservationKind::UnusualNoise && o.x == source_x)
}

#[test]
fn the_original_quiet_out_of_range_setup_is_not_a_witness() {
    let mut sim = pump_house();
    for _ in 0..40 {
        tick(&mut sim, &[]);
    }
    assert_eq!(sim.state().phase, Phase::Quiet);
    assert_eq!(sim.state().billy.mode, BillyMode::Offsite);
    let billy_x = sim.state().billy.x;
    let mole_x = sim.state().mole.as_ref().unwrap().state.x;
    assert_eq!(billy_x, -90.0);
    assert_eq!(mole_x, 390.0);
    assert!((mole_x - billy_x).abs() > sim.actor().stats.invest_dist);

    let events = tick(
        &mut sim,
        &[Command::Mole {
            op: MoleOp::Synth { index: 0 },
        }],
    );
    assert_eq!(sim.state().diversion.as_ref().unwrap().x, mole_x);
    assert!(!heard_noise(&sim, mole_x));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::GuardObserved { .. }))
    );

    // Presence alone is not enough: even with Billy on shift, the spawn-point
    // diversion remains outside his hearing. The choreography must close the
    // distance; merely forcing a crisis cannot repair the original setup.
    let events = tick(&mut sim, &[Command::ForceCrisis]);
    assert_eq!(sim.state().phase, Phase::Crisis);
    assert_ne!(sim.state().billy.mode, BillyMode::Offsite);
    assert!((mole_x - sim.state().billy.x).abs() > sim.actor().stats.invest_dist);
    assert!(!heard_noise(&sim, mole_x));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::GuardObserved { .. }))
    );
}

#[test]
fn an_audible_diversion_uses_the_sim_perception_door() {
    let mut sim = pump_house();
    // Aim at the seeded snack position and allow enough fixed ticks for both
    // bodies to reach it. No state is injected: crisis and movement are the
    // same typed commands the scenario sends through the production validator.
    let target_x = sim.state().billy.snack_x;
    tick(
        &mut sim,
        &[
            Command::ForceCrisis,
            Command::Mole {
                op: MoleOp::MoveTo {
                    x: target_x,
                    underground: true,
                },
            },
        ],
    );
    for _ in 0..90 {
        tick(&mut sim, &[]);
    }
    assert_ne!(sim.state().billy.mode, BillyMode::Offsite);
    assert_eq!(sim.state().mole.as_ref().unwrap().state.x, target_x);
    assert!((target_x - sim.state().billy.x).abs() < sim.actor().stats.invest_dist);

    let events = tick(
        &mut sim,
        &[Command::Mole {
            op: MoleOp::Synth { index: 0 },
        }],
    );
    assert!(heard_noise(&sim, target_x));
    let source = sim
        .actor_observations(sim.state().billy.x)
        .into_iter()
        .find(|o| o.kind == ObservationKind::UnusualNoise && o.x == target_x)
        .expect("the companion diversion is audible");
    assert!(events.iter().any(|e| matches!(
        e,
        Event::GuardObserved { kind: ObservationKind::UnusualNoise, room }
            if *room == source.room
    )));

    // This is the wrong-door control from #191. The diversion is companion
    // noise, supplied by Sim::extra_noise() through actor_observations(); it
    // is not in infra::noise_sources(). Calling the free function with an
    // empty extra_noise slice must not be mistaken for the full contract.
    let without_companion_noise = infra::observations(
        sim.definition(),
        sim.state(),
        sim.state().billy.x,
        Hearing {
            dist: sim.actor().stats.invest_dist,
            threshold: sim.actor().stats.invest_noise,
        },
        &[],
    );
    assert!(
        !without_companion_noise
            .iter()
            .any(|o| o.kind == ObservationKind::UnusualNoise && o.x == target_x)
    );
}

#[test]
fn the_corpus_keeps_the_source_specific_observation_checks() {
    let result = InfraOrdersScenario.run();
    for name in [
        "guard-on-shift-when-the-synth-plays",
        "shadow-position-matches-billy-at-synth",
        "diversion-source-within-billys-hearing",
        "diversion-announcement-source-unambiguous",
        "guard-observed-the-diversion",
        "perception-contract-lists-the-noise",
        "only-the-scene-diversion-announced",
    ] {
        let check = result
            .checks
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("EW-06 dropped required check {name}"));
        assert!(check.passed, "{}: {}", check.name, check.detail);
    }
}

#[test]
fn the_scene_evidence_records_one_source_and_one_same_tick_announcement() {
    let result = InfraOrdersScenario.run();
    let detail = |name: &str| {
        let check = result
            .checks
            .iter()
            .find(|check| check.name == name)
            .unwrap_or_else(|| panic!("EW-06 dropped check {name}"));
        assert!(check.passed, "{}: {}", check.name, check.detail);
        check.detail.as_str()
    };

    // The event carries only kind and room. The recorded count is necessary
    // to distinguish the diversion from an unrelated same-room sound.
    assert!(
        detail("diversion-announcement-source-unambiguous")
            .starts_with("1 audible UnusualNoise observations in the diversion's room")
    );
    assert!(
        detail("guard-observed-the-diversion")
            .starts_with("1 matching GuardObserved(UnusualNoise) events on command tick 81")
    );
    assert!(
        detail("only-the-scene-diversion-announced")
            .starts_with("1 UnusualNoise announcements through the scene synth")
    );

    // The recorded source must be the same one in the hearing and
    // perception evidence, rather than a different noise with the same tag.
    let hearing = detail("diversion-source-within-billys-hearing");
    let perception = detail("perception-contract-lists-the-noise");
    let field = |text: &str, prefix: &str| -> String {
        text.split_once(prefix)
            .unwrap_or_else(|| panic!("missing {prefix} in {text}"))
            .1
            .split(|c: char| c == ',' || c.is_whitespace())
            .next()
            .unwrap()
            .to_owned()
    };
    assert_eq!(
        field(hearing, "diversion.x="),
        field(perception, "source x=")
    );
    assert_eq!(
        field(hearing, "intensity="),
        field(perception, "intensity=")
    );
}
