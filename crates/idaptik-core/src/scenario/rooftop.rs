//! The played Ghost Lobby, begun from outside.
//!
//! The infiltrator starts on the roof. There are two ways in:
//!
//! * the rope, tied off at the roof's front edge: abseil down the facade to
//!   the street, then through the front door into the kitchen. The door is
//!   locked; the infiltrator picks it (slow, and it scratches), or the hacker
//!   opens it like any other door;
//! * the hatch over the laundry: quiet to reach, but it drops you in at the
//!   far end of the floor, and the landing is heard.
//!
//! Billy's arrival clock waits until the infiltrator is inside the building.
//! Everything else is the wired floor (signal network, consoles), so the
//! canonical floor and its replays are untouched.

use crate::scenario::constants as c;
use crate::scenario::definition::{DoorDef, ExteriorDef, HatchDef, RopeDef, ScenarioDefinition};
use crate::scenario::signal::ghost_lobby_wiring::ghost_lobby_wired;

/// The front door's id.
pub const FRONT_DOOR: &str = "D0";

/// Build the rooftop start of the played floor.
pub fn ghost_lobby_rooftop() -> ScenarioDefinition {
    let mut def = ghost_lobby_wired();
    let left = def.rooms.first().map_or(20.0, |r| r.x);
    let right = def.rooms.last().map_or(1260.0, |r| r.right());
    // The front door, on the kitchen's street wall.
    def.doors.insert(
        0,
        DoorDef {
            id: FRONT_DOOR.into(),
            x: left,
            label: "Front door".into(),
        },
    );
    let ext = ExteriorDef {
        roof_y: c::ROOF_Y,
        // The far end stops at a parapet; the near end is the rope.
        roof: (left, right - c::PARAPET_W - def.player.w),
        street_lo: c::STREET_LO,
        rope: RopeDef {
            top_x: left + 16.0,
            foot_x: left - 34.0,
            speed: c::ROPE_SPEED,
        },
        hatches: vec![HatchDef {
            id: "hatch-laundry".into(),
            x: 990.0,
            room: "laundry".into(),
        }],
        front_door: FRONT_DOOR.into(),
        pick_time: c::PICK_TIME,
        support: c::EXTERIOR_SUPPORT,
    };
    // On the roof, over the kitchen, a few steps from the rope.
    def.player.spawn_x = left + 120.0;
    def.player.spawn_y = ext.roof_y - def.player.h;
    def.exterior = Some(ext);
    def
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::command::{Button, Buttons, Edge, RunConfig, TickInput};
    use crate::scenario::event::Event;
    use crate::scenario::sim::GhostLobbySim;
    use crate::scenario::state::Place;

    fn sim() -> GhostLobbySim {
        GhostLobbySim::new(ghost_lobby_rooftop(), RunConfig::standard(), 7).expect("valid")
    }

    fn hold(buttons: &[Button]) -> TickInput {
        let mut b = Buttons::default();
        for x in buttons {
            b.set(*x, true);
        }
        TickInput {
            buttons: b,
            ..TickInput::default()
        }
    }

    fn press_interact() -> TickInput {
        let mut b = Buttons::default();
        b.set(Button::Interact, true);
        TickInput {
            buttons: b,
            edges: vec![Edge::InteractPress],
            ..TickInput::default()
        }
    }

    /// Walk (left or right) until the body's centre is within `reach` of `x`.
    fn walk_to(sim: &mut GhostLobbySim, x: f64, reach: f64) {
        for _ in 0..1200 {
            let p = &sim.state().player;
            let cx = p.x + sim.definition().player.w / 2.0;
            if (cx - x).abs() <= reach {
                sim.tick(&TickInput::default());
                return;
            }
            let dir = if cx < x { Button::Right } else { Button::Left };
            sim.tick(&hold(&[dir]));
        }
        panic!("never reached x {x}");
    }

    fn rope(def: &ScenarioDefinition) -> RopeDef {
        def.exterior.as_ref().unwrap().rope
    }

    #[test]
    fn it_validates_and_starts_on_the_roof_outside() {
        let def = ghost_lobby_rooftop();
        assert!(def.validate().ok().is_ok(), "{:?}", def.validate().ok());
        let s = sim();
        assert_eq!(s.state().player.place, Place::Roof);
        assert!(s.infiltrator_outside());
        let ext = def.exterior.unwrap();
        assert!((s.state().player.y + def.player.h - ext.roof_y).abs() < 1e-9);
    }

    #[test]
    fn the_canonical_floor_has_no_outside() {
        let s = GhostLobbySim::new(crate::scenario::ghost_lobby(), RunConfig::standard(), 7)
            .expect("valid");
        assert_eq!(s.state().player.place, Place::Floor);
        assert!(!s.infiltrator_outside());
    }

    #[test]
    fn the_roof_holds_you_up_and_in() {
        let mut s = sim();
        for _ in 0..600 {
            s.tick(&hold(&[Button::Right]));
        }
        let ext = s.definition().exterior.clone().unwrap();
        let p = &s.state().player;
        assert_eq!(p.place, Place::Roof);
        assert!(p.x <= ext.roof.1 + 1e-9, "walked off the roof: {}", p.x);
        assert!((p.y + s.definition().player.h - ext.roof_y).abs() < 1e-9);
    }

    #[test]
    fn abseil_down_then_the_front_door_is_locked_until_picked() {
        let mut s = sim();
        let r = rope(s.definition());
        walk_to(&mut s, r.top_x, 4.0);
        let events = s.tick(&press_interact());
        assert!(events.contains(&Event::RopeTaken { down: true }));
        for _ in 0..240 {
            s.tick(&TickInput::default());
        }
        assert_eq!(s.state().player.place, Place::Floor, "reached the street");
        assert!(s.infiltrator_outside());

        // The locked front door stops the body on the street.
        for _ in 0..240 {
            s.tick(&hold(&[Button::Right]));
        }
        assert!(s.infiltrator_outside(), "walked through a locked door");

        // Pick it: hold interact at the door.
        let mut picked = false;
        for _ in 0..((c::PICK_TIME * 60.0) as usize + 30) {
            let events = s.tick(&hold(&[Button::Interact]));
            picked |= events.contains(&Event::FrontDoorPicked);
        }
        assert!(picked, "the lock never gave");
        for _ in 0..60 {
            s.tick(&hold(&[Button::Right]));
        }
        assert!(!s.infiltrator_outside(), "in through the front door");
    }

    #[test]
    fn the_rope_climbs_back_up() {
        let mut s = sim();
        let r = rope(s.definition());
        walk_to(&mut s, r.top_x, 4.0);
        s.tick(&press_interact());
        for _ in 0..240 {
            s.tick(&TickInput::default());
        }
        assert_eq!(s.state().player.place, Place::Floor);
        s.tick(&press_interact());
        for _ in 0..240 {
            s.tick(&TickInput::default());
        }
        assert_eq!(s.state().player.place, Place::Roof);
    }

    #[test]
    fn the_hatch_drops_you_into_its_room_with_a_noise() {
        let mut s = sim();
        let hatch = s.definition().exterior.as_ref().unwrap().hatches[0].clone();
        walk_to(&mut s, hatch.x, 4.0);
        let events = s.tick(&press_interact());
        assert!(events.contains(&Event::HatchDropped {
            hatch: hatch.id.clone()
        }));
        let mut loudest: f64 = 0.0;
        for _ in 0..90 {
            s.tick(&TickInput::default());
            loudest = loudest.max(s.state().player.noise);
        }
        assert!(!s.infiltrator_outside());
        assert!(s.state().player.grounded);
        // The landing's noise, less the one tick of decay it lands on.
        let floor = c::NOISE_LAND - c::NOISE_DECAY * crate::scenario::TICK_DT;
        assert!(loudest >= floor - 1e-9, "a silent landing: {loudest}");
        let room = GhostLobbySim::room_id_at(s.definition(), s.state().player.x);
        assert_eq!(room, Some(hatch.room.as_str()));
    }

    #[test]
    fn billys_clock_waits_until_the_infiltrator_is_inside() {
        let mut s = sim();
        let start = s.state().crisis_at;
        for _ in 0..300 {
            s.tick(&hold(&[Button::Right]));
        }
        assert!(
            s.state().crisis_at > start + 4.0,
            "the clock ran on the roof"
        );
        assert!(s.state().arrival_held);
    }
}
