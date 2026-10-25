//! A small, complete power test floor: **two independent feeds and one
//! backup-powered device**.
//!
//! It is the Ghost Lobby with its signal wiring *and* an electrical
//! infrastructure attached, and it exists to demonstrate every supported effect
//! in one place:
//!
//! | Effect | Where to look |
//! |---|---|
//! | Selective loss of power | `feed-a` (plant and lighting) versus `feed-b` (security) |
//! | Backup power, temporary | `ups-hall` carries `cam-0` for 900 ticks, then is spent |
//! | Disabled fans / pumps / doors / cameras | `fan-kitchen`, `pump-laundry`, `door-d4`, `cam-0..2` |
//! | Protective trip and deliberate reset | `brk-b`: route anything heavy onto `feed-b` and it trips |
//! | Exposed-power hazard | `haz-pump`: the pump housing in the laundry, while `feed-a` is up |
//! | Command is not power | switch `cam-1` on with `feed-b` dead: the command lands, the camera stays blind |
//!
//! Two permission contrasts sit on the same floor. `brk-b` and `mains-b` are
//! `remote` on the maintenance bridge node, so the hacker can work them once
//! pivoted; `brk-a` and `mains-a` are local-only and are the infiltrator's to
//! touch. No amount of bandwidth cuts a cable.
//!
//! The loads are the Ghost Lobby's own devices (one per door, camera and room
//! light, plus the pump and the extract fan that no signal device owns), so the
//! signal network's ids and this network's ids line up without a mapping table —
//! the same trick [`super::super::signal::ghost_lobby_wiring`] plays with the
//! grounded network's node ids.

use super::{
    BackupMode, BackupSpec, BreakerDef, Condition, FeedDef, HazardDef, HazardLive, InterlockDef,
    InterlockWhen, LoadDef, LoadKind, POWER_FORMAT, PowerNetworkDef, PowerTuning, Restart,
    SourceDef, SourceKind,
};
use crate::scenario::definition::ScenarioDefinition;
use crate::scenario::ids::FeedId;
use crate::scenario::signal::ghost_lobby_wiring::ghost_lobby_wired;

/// The committed golden of [`power_test_floor`].
pub const POWER_TEST_FLOOR_JSON: &str = include_str!("power_test_floor.json");

/// The utility supply for plant and lighting.
pub const MAINS_A: &str = "mains-a";
/// The utility supply for security.
pub const MAINS_B: &str = "mains-b";
/// The hall's UPS: the floor's one backup-powered device hangs off it.
pub const UPS_HALL: &str = "ups-hall";

/// Plant and lighting: five room lights, the extract fan, the laundry pump.
pub const FEED_A: &str = "feed-a";
/// Security: three cameras and the service exit's mechanism.
pub const FEED_B: &str = "feed-b";

/// The plant board. Local only — the infiltrator's to touch.
pub const BRK_A: &str = "brk-a";
/// The security board. Remote on the maintenance bridge — the hacker's.
pub const BRK_B: &str = "brk-b";

/// The hall camera, the one device the UPS carries.
pub const CAM_HALL: &str = "cam-0";
/// The kitchen extract fan.
pub const FAN_KITCHEN: &str = "fan-kitchen";
/// The laundry pump.
pub const PUMP_LAUNDRY: &str = "pump-laundry";
/// The scenario door the service exit's mechanism belongs to, as the floor
/// definition spells it.
pub const EXIT_DOOR: &str = "D4";
/// The service exit's mechanism: the load, the signal device and the grounded
/// node are all one string, [`crate::scenario::floor_graph::door_node_id`] of
/// [`EXIT_DOOR`]. [`tests::the_exit_load_is_the_door_it_mechanises`] is what
/// keeps those two spellings honest — an invariant belongs in a test, not in a
/// build that can panic on the way to the menu.
pub const DOOR_EXIT: &str = "door-d4";

/// The maintenance bridge's grounded node: the security board's remote panel.
const BRIDGE_NODE: &str = "bms-bridge";

fn source(
    id: &str,
    label: &str,
    kind: SourceKind,
    startup_ticks: u32,
    room: Option<&str>,
    node: Option<&str>,
    remote: bool,
) -> SourceDef {
    SourceDef {
        id: id.into(),
        label: label.to_owned(),
        kind,
        start_on: true,
        startup_ticks,
        backup: None,
        room: room.map(|r| r.into()),
        node: node.map(|n| n.to_owned()),
        remote,
    }
}

fn feed(
    id: &str,
    label: &str,
    source: &str,
    capacity: u32,
    breaker: &str,
    room: &str,
    node: Option<&str>,
) -> FeedDef {
    FeedDef {
        id: id.into(),
        label: label.to_owned(),
        source: source.into(),
        capacity,
        breaker: breaker.into(),
        room: Some(room.into()),
        node: node.map(|n| n.to_owned()),
    }
}

fn breaker(
    id: &str,
    label: &str,
    feed: &str,
    room: &str,
    node: Option<&str>,
    remote: bool,
) -> BreakerDef {
    BreakerDef {
        id: id.into(),
        label: label.to_owned(),
        feed: feed.into(),
        start_open: false,
        remote,
        room: Some(room.into()),
        node: node.map(|n| n.to_owned()),
    }
}

#[allow(clippy::too_many_arguments)]
fn load(
    id: &str,
    label: &str,
    kind: LoadKind,
    feed: &str,
    draw: u32,
    room: &str,
    restart: Restart,
    restart_ticks: Option<u32>,
) -> LoadDef {
    LoadDef {
        id: id.into(),
        label: label.to_owned(),
        kind,
        feed: feed.into(),
        draw,
        signal: None,
        room: Some(room.into()),
        restart,
        start_on: true,
        condition: Condition::Intact,
        restart_ticks,
        node: None,
    }
}

/// A load that *is* a signal device: same id, same room, its node for a remote
/// panel.
fn signal_load(
    id: &str,
    label: &str,
    kind: LoadKind,
    feed: &str,
    draw: u32,
    room: &str,
    restart: Restart,
) -> LoadDef {
    LoadDef {
        signal: Some(id.into()),
        node: Some(id.to_owned()),
        ..load(id, label, kind, feed, draw, room, restart, None)
    }
}

/// The test floor's power network.
pub fn power_test_floor() -> PowerNetworkDef {
    use LoadKind::*;
    let loads = vec![
        signal_load(
            "light-kitchen",
            "Kitchen lighting",
            Light,
            FEED_A,
            2,
            "kitchen",
            Restart::Hold,
        ),
        signal_load(
            "light-hall",
            "Hallway lighting",
            Light,
            FEED_A,
            2,
            "hall",
            Restart::Hold,
        ),
        signal_load(
            "light-office",
            "Office lighting",
            Light,
            FEED_A,
            2,
            "office",
            Restart::Hold,
        ),
        signal_load(
            "light-laundry",
            "Laundry lighting",
            Light,
            FEED_A,
            2,
            "laundry",
            Restart::Hold,
        ),
        signal_load(
            "light-exit",
            "Exit lighting",
            Light,
            FEED_A,
            2,
            "exit",
            Restart::Hold,
        ),
        // Nothing in the signal network owns a fan or a pump: they are power's
        // alone, and a player learns about them from the power model.
        load(
            FAN_KITCHEN,
            "Kitchen extract",
            Fan,
            FEED_A,
            4,
            "kitchen",
            Restart::Resume,
            Some(180),
        ),
        load(
            PUMP_LAUNDRY,
            "Laundry pump",
            Pump,
            FEED_A,
            6,
            "laundry",
            Restart::Resume,
            Some(240),
        ),
        signal_load(
            CAM_HALL,
            "Hall camera",
            Camera,
            FEED_B,
            3,
            "hall",
            Restart::Resume,
        ),
        signal_load(
            "cam-1",
            "Office camera",
            Camera,
            FEED_B,
            3,
            "office",
            Restart::Resume,
        ),
        signal_load(
            "cam-2",
            "Laundry camera",
            Camera,
            FEED_B,
            3,
            "laundry",
            Restart::Resume,
        ),
        // A door that re-opens by itself when the lights come back is a gift to
        // the infiltrator: the exit is fail-safe.
        signal_load(
            DOOR_EXIT,
            "Service exit mechanism",
            Door,
            FEED_B,
            4,
            "exit",
            Restart::FailSafe,
        ),
    ];
    let mut sources = vec![
        source(
            MAINS_A,
            "Utility supply — plant",
            SourceKind::Mains,
            0,
            Some("kitchen"),
            None,
            false,
        ),
        source(
            MAINS_B,
            "Utility supply — security",
            SourceKind::Mains,
            0,
            Some("hall"),
            Some(BRIDGE_NODE),
            true,
        ),
        source(
            UPS_HALL,
            "Hall UPS",
            SourceKind::Battery,
            0,
            Some("hall"),
            None,
            false,
        ),
    ];
    if let Some(ups) = sources.get_mut(2) {
        // Fifteen seconds of a camera that will not blink.
        ups.backup = Some(BackupSpec {
            runtime_ticks: 900,
            serves: vec![CAM_HALL.into()],
            mode: BackupMode::Automatic,
        });
    }

    PowerNetworkDef {
        format: POWER_FORMAT.to_owned(),
        network_id: "ghost-lobby-power".to_owned(),
        name: "Envelope 001 — Ghost Lobby — power test floor".to_owned(),
        sources,
        feeds: vec![
            feed(
                FEED_A,
                "Plant & lighting board",
                MAINS_A,
                20,
                BRK_A,
                "kitchen",
                None,
            ),
            feed(
                FEED_B,
                "Security board",
                MAINS_B,
                14,
                BRK_B,
                "hall",
                Some(BRIDGE_NODE),
            ),
        ],
        loads,
        breakers: vec![
            breaker(BRK_A, "Plant board main", FEED_A, "kitchen", None, false),
            breaker(
                BRK_B,
                "Security board main",
                FEED_B,
                "hall",
                Some(BRIDGE_NODE),
                true,
            ),
        ],
        interlocks: vec![InterlockDef {
            id: "il-pump".to_owned(),
            label: "Laundry occupancy interlock".to_owned(),
            load: PUMP_LAUNDRY.into(),
            holds_when: InterlockWhen::SignalActive("light-laundry".into()),
        }],
        hazards: vec![HazardDef {
            id: "haz-pump".into(),
            label: "Exposed pump housing".to_owned(),
            feed: FeedId::from(FEED_A),
            room: "laundry".into(),
            // The far end of the laundry (795..1040 canvas units), clear of the
            // chute and the door: a hazard you can walk around once you know.
            x: (920.0, 1000.0),
            live: HazardLive::FeedEnergized,
            shock_ticks: 90,
        }],
        tuning: PowerTuning::default(),
    }
}

/// The Ghost Lobby with wiring *and* power: the floor the frontends play when
/// both subsystems are on. [`crate::scenario::ghost_lobby::ghost_lobby`] itself
/// stays bare, so its golden and every recorded replay are untouched.
pub fn powered_ghost_lobby() -> ScenarioDefinition {
    let mut def = ghost_lobby_wired();
    def.name = format!("{} — power test floor", def.name);
    def.power = Some(power_test_floor());
    def
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::ids::DoorId;

    #[test]
    fn the_exit_load_is_the_door_it_mechanises() {
        // One spelling, derived once: the scenario door's grounded node is the
        // signal device's node and this load's id. Nothing in the two networks
        // maps between them because there is nothing to map.
        assert_eq!(
            crate::scenario::floor_graph::door_node_id(DoorId::from(EXIT_DOOR)),
            DOOR_EXIT
        );
        let net = power_test_floor();
        let load = net
            .loads
            .iter()
            .find(|l| l.id.as_str() == DOOR_EXIT)
            .expect("the exit mechanism is a load");
        assert_eq!(load.signal.as_ref().map(|s| s.as_str()), Some(DOOR_EXIT));
    }

    #[test]
    fn the_test_floor_validates() {
        let errors = power_test_floor().validate();
        assert_eq!(errors, vec![]);
    }

    #[test]
    fn two_feeds_are_independent_and_one_device_has_a_backup() {
        let net = power_test_floor();
        assert_eq!(net.feeds.len(), 2);
        assert_ne!(net.feeds[0].source, net.feeds[1].source);
        let backups: Vec<_> = net
            .sources
            .iter()
            .filter_map(|s| s.backup.as_ref().map(|b| (s, b)))
            .collect();
        assert_eq!(backups.len(), 1, "exactly one backup supply");
        assert_eq!(backups[0].1.serves.len(), 1, "and it carries one device");
    }

    #[test]
    fn both_feeds_start_inside_their_rating() {
        let net = power_test_floor();
        let assignment: Vec<FeedId> = net.loads.iter().map(|l| l.feed.clone()).collect();
        for f in &net.feeds {
            let draw = net.connected_draw(&f.id, &assignment);
            assert!(draw <= f.capacity, "{} is {draw}/{}", f.id, f.capacity);
        }
        // Feed A is exactly at its rating: routing anything onto it trips it.
        assert_eq!(net.connected_draw(&FeedId::from(FEED_A), &assignment), 20);
    }

    #[test]
    fn the_floor_is_the_wired_lobby_with_power_on_top() {
        let def = powered_ghost_lobby();
        assert!(def.signal.is_some(), "wiring is kept");
        assert!(def.power.is_some(), "power is attached");
        assert!(crate::scenario::ghost_lobby::ghost_lobby().power.is_none());
    }
}
