//! A small deterministic host world for headless replay.
//!
//! This is **not** the game. It is a minimal, self-contained host that owns a
//! corridor, a guard and a scripted infiltrator, generates honest observations
//! from what the guard can perceive, applies the kernel's proposals through its
//! own navigation, and records a replay. It exists so the adapter and the
//! replay machinery can be exercised end-to-end — and checked — without a
//! window or the full IDApTIK sim.
//!
//! The architecture is the same as the real integration: the world owns truth,
//! physics, collision and action application; the kernel owns memory and
//! proposes intents. Whatever this model does, the game does authoritatively.

use crate::error::Result;
use crate::fx::{Fx2, from_fx, to_fx};
use crate::host::MindRecord;
use crate::host::{AgentState, WeevilHost};
use crate::intent::{Action, Intent};
use crate::observation::{Contact, ContactSource, Observation};
use crate::profile::{Profile, ScriptedContact};
use crate::replay::{
    AppliedEffect, ContactRecord, IntentRecord, ObservationRecord, ReplayEntry, ReplayRecorder,
};

/// One entity in the world.
#[derive(Debug, Clone, Copy)]
pub struct Entity {
    pub id: u32,
    pub x: f64,
    pub kind: EntityKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    Player,
    Object,
}

/// A scripted decoy: present to perception (but not to truth) for a window.
#[derive(Debug, Clone, Copy)]
pub struct Decoy {
    pub id: u32,
    pub x: f64,
    pub active_until_tick: u64,
}

/// The host world.
pub struct HostWorld {
    pub tick: u64,
    /// Room spans as `(left, width)`, contiguous.
    pub rooms: Vec<(f64, f64)>,
    /// Closed-door x positions (walls).
    pub doors: Vec<f64>,
    pub guard: Guard,
    pub player: Option<Entity>,
    pub objects: Vec<Entity>,
    pub decoy: Option<Decoy>,
    /// The guard's own last perceived position of the player, for investigation.
    pub last_known_player_x: Option<f64>,
    pub last_seen_ago: u64,
    /// Scripted schedule: the player is visible on these ticks.
    player_visible: Vec<u64>,
    player_x: f64,
    /// ABI fx units per world unit, mirrored from the profile.
    profile_scale: f64,
    /// A scripted cast ([`crate::profile::CorridorScript`]). When it is set it
    /// IS the cast: there is no default player and no decoy.
    scripted: Option<Vec<ScriptedContact>>,
    /// The scripted contacts that hit the guard on the current tick.
    hits_this_tick: Vec<u32>,
}

/// The guard's host-side state.
#[derive(Debug, Clone, Copy)]
pub struct Guard {
    pub x: f64,
    pub facing: f64,
    pub health: f64,
    pub ammo: f64,
    pub patrol_index: usize,
}

impl HostWorld {
    /// The canonical test corridor: five contiguous rooms, four doors, a guard,
    /// a scripted player and one object.
    pub fn canonical(profile: &Profile) -> Self {
        let rooms = vec![
            (0.0, 200.0),
            (200.0, 200.0),
            (400.0, 200.0),
            (600.0, 200.0),
            (800.0, 200.0),
        ];
        let doors = vec![200.0, 400.0, 600.0, 800.0];
        let mut w = Self {
            tick: 0,
            rooms,
            doors,
            guard: Guard {
                x: 50.0,
                facing: 1.0,
                health: 1.0,
                ammo: 3.0,
                patrol_index: 0,
            },
            player: None,
            objects: vec![Entity {
                id: 100,
                x: 700.0,
                kind: EntityKind::Object,
            }],
            // The deception is scenario content, so the corridor always carries
            // one. A profile that declares its own decoy replaces the scripted
            // one; the fields the corridor does not read (`distance_world`,
            // `threat`) it derives for itself from the guard's position.
            decoy: Some(match profile.host.decoy.as_ref() {
                Some(d) => Decoy {
                    id: d.id,
                    x: f64::from(d.origin_world.x),
                    active_until_tick: d.active_ticks,
                },
                None => Decoy {
                    id: 200,
                    x: 450.0,
                    active_until_tick: 60,
                },
            }),
            last_known_player_x: None,
            last_seen_ago: 0,
            player_visible: vec![10, 11, 12, 13, 14, 15],
            player_x: 60.0,
            profile_scale: 1.0,
            scripted: None,
            hits_this_tick: Vec::new(),
        };
        w.profile_scale = profile.host.fx_per_world_unit;
        if let Some(script) = &profile.host.script {
            w.player_visible.clear();
            w.decoy = None;
            w.scripted = Some(script.contacts.clone());
        }
        w
    }

    /// The fx scale in use (set from the profile).
    pub fn scale(&self) -> f64 {
        self.profile_scale
    }

    /// Advance the scripted player and apply the schedule.
    fn advance_script(&mut self) {
        self.tick += 1;
        let visible = self.player_visible.contains(&self.tick);
        if visible {
            // Walk the player rightward, bouncing at the corridor ends.
            self.player_x += 2.0;
            if self.player_x > 950.0 {
                self.player_x = 50.0;
            }
            self.player = Some(Entity {
                id: 1,
                x: self.player_x,
                kind: EntityKind::Player,
            });
            self.last_known_player_x = Some(self.player_x);
            self.last_seen_ago = 0;
        } else {
            self.player = None;
            self.last_seen_ago += 1;
        }
        // Scripted hits land before the guard observes, so the snapshot of this
        // tick carries the drop in health, as the kernel's fixture does.
        self.hits_this_tick.clear();
        let tick = self.tick;
        let mut damage = 0.0;
        for c in self.scripted.iter().flatten() {
            if !present(c, tick) {
                continue;
            }
            for h in c.hits.iter().filter(|h| h.tick == tick) {
                damage += h.damage;
                self.hits_this_tick.push(c.id);
            }
        }
        self.guard.health = (self.guard.health - damage).max(0.0);
    }

    /// The scripted contact with this id, if it is in the corridor this tick.
    fn scripted_present(&self, id: u32) -> Option<&ScriptedContact> {
        self.scripted
            .iter()
            .flatten()
            .find(|c| c.id == id && present(c, self.tick))
    }

    /// The contact the host attributes this tick's damage to: the first
    /// scripted contact that hit the guard and that the guard perceives. A hit
    /// from a contact the guard cannot see is damage with no attribution.
    pub fn damage_source(&self, obs: &Observation) -> Option<u32> {
        self.hits_this_tick
            .iter()
            .copied()
            .find(|id| obs.contacts.iter().any(|c| c.id == *id))
    }

    /// Whether a closed door lies strictly between two x positions.
    fn blocked_by_door(&self, from: f64, to: f64) -> bool {
        let lo = from.min(to);
        let hi = from.max(to);
        self.doors.iter().any(|d| *d > lo && *d < hi)
    }

    /// The room index containing `x`.
    pub fn room_at(&self, x: f64) -> usize {
        self.rooms
            .iter()
            .position(|(l, w)| x >= *l && x < l + w)
            .unwrap_or(self.rooms.len() - 1)
    }

    /// Can the guard perceive the player right now? Same room, no wall between,
    /// within the profile's engage-ish range. Honest perception: a guard that
    /// cannot see the player reports no player contact.
    fn can_see_player(&self, profile: &Profile) -> bool {
        let Some(p) = self.player else {
            return false;
        };
        if self.room_at(self.guard.x) != self.room_at(p.x) {
            return false;
        }
        if self.blocked_by_door(self.guard.x, p.x) {
            return false;
        }
        let dist = (p.x - self.guard.x).abs();
        dist <= profile.host.engage_dist_world * 2.0
    }

    /// Build this tick's observation for the guard.
    pub fn observation(&self, profile: &Profile) -> Observation {
        let mut contacts = Vec::new();
        if let Some(p) = self.player
            && self.can_see_player(profile)
        {
            let dist = (p.x - self.guard.x).abs();
            contacts.push(Contact {
                id: p.id,
                source: ContactSource::Player,
                bearing: Fx2 {
                    x: to_fx(if p.x >= self.guard.x { 1.0 } else { -1.0 }),
                    y: 0,
                },
                distance: to_fx(dist * profile.host.fx_per_world_unit),
                threat: to_fx(0.5),
                confidence: 1.0,
                age_ticks: 0,
            });
        }
        if profile.host.report_objects {
            for o in &self.objects {
                if self.room_at(self.guard.x) == self.room_at(o.x) {
                    let dist = (o.x - self.guard.x).abs();
                    contacts.push(Contact {
                        id: o.id,
                        source: ContactSource::Object(o.id),
                        bearing: Fx2 {
                            x: to_fx(if o.x >= self.guard.x { 1.0 } else { -1.0 }),
                            y: 0,
                        },
                        distance: to_fx(dist * profile.host.fx_per_world_unit),
                        threat: to_fx(0.2),
                        confidence: 0.8,
                        age_ticks: 0,
                    });
                }
            }
        }
        if let Some(d) = self.decoy
            && self.tick <= d.active_until_tick
            && self.room_at(self.guard.x) == self.room_at(d.x)
        {
            let dist = (d.x - self.guard.x).abs();
            contacts.push(Contact {
                id: d.id,
                source: ContactSource::Decoy,
                bearing: Fx2 {
                    x: to_fx(if d.x >= self.guard.x { 1.0 } else { -1.0 }),
                    y: 0,
                },
                distance: to_fx(dist * profile.host.fx_per_world_unit),
                threat: to_fx(0.4),
                confidence: 0.6,
                age_ticks: 0,
            });
        }
        for c in self.scripted.iter().flatten() {
            if present(c, self.tick) && self.room_at(self.guard.x) == self.room_at(c.x) {
                let dist = (c.x - self.guard.x).abs();
                contacts.push(Contact {
                    id: c.id,
                    // A scripted contact is a person standing in the corridor:
                    // the closest source the closed vocabulary has.
                    source: ContactSource::Player,
                    bearing: Fx2 {
                        x: to_fx(if c.x >= self.guard.x { 1.0 } else { -1.0 }),
                        y: 0,
                    },
                    distance: to_fx(dist * profile.host.fx_per_world_unit),
                    threat: to_fx(c.threat),
                    confidence: 1.0,
                    age_ticks: 0,
                });
            }
        }
        Observation {
            agent_id: 0,
            tick: self.tick,
            self_pos: self.guard_pos(),
            self_vel: Fx2 {
                x: to_fx(self.guard.facing * 0.0),
                y: 0,
            },
            health: to_fx(self.guard.health),
            ammo: to_fx(self.guard.ammo),
            capability_mask: profile.capabilities,
            contacts,
            cooldown: 0,
        }
    }

    fn guard_pos(&self) -> Fx2 {
        Fx2 {
            x: to_fx(self.guard.x),
            y: 0,
        }
    }

    /// Move the guard toward `target_x` by at most `speed_world` units, blocked
    /// by closed doors. Returns the applied speed and whether it was blocked.
    fn move_toward(&mut self, target_x: f64, speed_world: f64) -> (i32, bool) {
        let dx = target_x - self.guard.x;
        if dx.abs() < 1e-6 {
            return (0, false);
        }
        let dir = if dx > 0.0 { 1.0 } else { -1.0 };
        self.guard.facing = dir;
        let step = dir * speed_world.min(dx.abs());
        let new_x = self.guard.x + step;
        if self.blocked_by_door(self.guard.x, new_x) {
            // Pressed against a wall: do not pass through it.
            return (to_fx(speed_world), true);
        }
        self.guard.x = new_x.clamp(0.0, 999.0);
        (to_fx(speed_world), false)
    }

    /// Apply a validated intent. This is the host's authoritative application.
    pub fn apply(&mut self, intent: &Intent, profile: &Profile) -> AppliedEffect {
        // The movement limit is a *world* limit, so it is applied AFTER the
        // fx→world conversion. Clamping the kernel's fx speed against the fx
        // form of the limit instead compares the kernel's 1.0-fx advance
        // against a 1.6-fx ceiling and never bites — the guard would then run
        // at 8 canvas units per tick whatever the profile declared, and
        // `max_speed_world` would be a number nothing read.
        let mut speed_world = from_fx(intent.speed.max(0)) / profile.host.fx_per_world_unit;
        if speed_world > profile.host.max_speed_world {
            speed_world = profile.host.max_speed_world;
        }

        let (target_x, reason) = match intent.action {
            Action::Advance => {
                // Host navigation: cycle the patrol route, or follow a contact
                // the guard can still see.
                if let Some(p) = self.player
                    && self.can_see_player(profile)
                {
                    (p.x, "advance-toward-contact")
                } else {
                    let waypoints = &profile.host.patrol_x;
                    if waypoints.is_empty() {
                        (self.guard.x, "advance-hold")
                    } else {
                        let wp = waypoints[self.guard.patrol_index % waypoints.len()];
                        if (wp - self.guard.x).abs() < 4.0 {
                            self.guard.patrol_index =
                                (self.guard.patrol_index + 1) % waypoints.len();
                        }
                        (
                            waypoints[self.guard.patrol_index % waypoints.len()],
                            "advance-patrol",
                        )
                    }
                }
            }
            Action::Attack => match self.resolve_target(intent, profile) {
                Some(x) => (x, "attack"),
                None => {
                    return AppliedEffect {
                        moved: false,
                        speed_applied: 0,
                        blocked: false,
                        reason: "attack-invalid-target".to_owned(),
                    };
                }
            },
            Action::Flee => match self.resolve_target(intent, profile) {
                Some(x) => {
                    let away = if self.guard.x >= x { 999.0 } else { 0.0 };
                    (away, "flee")
                }
                None => {
                    return AppliedEffect {
                        moved: false,
                        speed_applied: 0,
                        blocked: false,
                        reason: "flee-invalid-target".to_owned(),
                    };
                }
            },
            Action::Investigate => {
                // The kernel remembers a bearing, not a position. The host owns
                // the last perceived position, so it investigates there — and
                // never treats the kernel's memory as a coordinate.
                match self.last_known_player_x {
                    Some(x) => (x, "investigate-last-known"),
                    None => (self.guard.x, "investigate-nothing-remembered"),
                }
            }
            Action::Reload => {
                self.guard.ammo = 3.0;
                (self.guard.x, "reload")
            }
            Action::Hold | Action::Idle | Action::TakeCover | Action::Regroup => {
                (self.guard.x, "hold")
            }
        };

        let before = self.guard.x;
        let (applied, blocked) = self.move_toward(target_x, speed_world);
        // `moved` means the guard's position changed, not that a non-zero speed
        // was requested: a tick pressed against a closed door asks for speed and
        // gets nowhere, and recording it as "moved" would make the replay say
        // the guard crossed a wall. Computed from the position delta, the same
        // way the game's own `apply_guard_decision` does it.
        let moved = (self.guard.x - before).abs() > f64::EPSILON;
        AppliedEffect {
            moved,
            speed_applied: applied,
            blocked,
            reason: reason.to_owned(),
        }
    }

    /// Resolve an intent's target id to a world x, refusing ids no live entity
    /// owns. This is the host's authoritative target validation.
    fn resolve_target(&self, intent: &Intent, _profile: &Profile) -> Option<f64> {
        let id = intent.target_id;
        if let Some(p) = self.player
            && p.id == id
        {
            return Some(p.x);
        }
        if let Some(o) = self.objects.iter().find(|o| o.id == id) {
            return Some(o.x);
        }
        if let Some(d) = self.decoy
            && d.id == id
        {
            return Some(d.x);
        }
        if let Some(c) = self.scripted_present(id) {
            return Some(c.x);
        }
        None
    }

    /// One headless tick: advance the script, observe, ask the kernel, validate,
    /// apply, and record. Returns the world's truth at the instant the guard
    /// observed, before the host applied anything.
    pub fn step(
        &mut self,
        host: &mut WeevilHost,
        profile: &Profile,
        recorder: &mut ReplayRecorder,
    ) -> Result<TickTruth> {
        self.step_with(host, profile, recorder, false)
            .map(|(truth, _)| truth)
    }

    /// [`HostWorld::step`], through `ee_tick` or, with `mind`, through the ABI 1.1
    /// `ee_tick_mind`, which also returns the tick's mind record. The mind tick
    /// is told which perceived contact this tick's damage came from.
    pub fn step_with(
        &mut self,
        host: &mut WeevilHost,
        profile: &Profile,
        recorder: &mut ReplayRecorder,
        mind: bool,
    ) -> Result<(TickTruth, Option<MindRecord>)> {
        self.advance_script();
        let obs = self.observation(profile);
        let mut truth = self.truth();
        truth.visible = obs.contacts.iter().map(|c| c.id).collect();
        let agent_before = host.agent(obs.agent_id).unwrap_or(AgentState {
            rng_state: 0,
            tick_last: 0,
            memory: None,
            cached_action: 0,
            cached_target: 0,
            budget_used: 0,
            degraded_ticks: 0,
            mode: 0,
            flags: 0,
        });
        let (intent, record) = if mind {
            let m = host.tick_mind(&obs, self.damage_source(&obs))?;
            (m.intent, Some(m.record))
        } else {
            (host.tick(&obs)?.intent, None)
        };

        // Host-side validation of the proposal. The kernel's speed is advisory
        // and the host's movement limit is applied when the effect is applied,
        // so the only speed condition worth refusing on is a negative one.
        let accepted = self.target_is_live(intent.target_id) && intent.speed >= 0;
        let applied = if accepted {
            self.apply(&intent, profile)
        } else {
            AppliedEffect {
                moved: false,
                speed_applied: 0,
                blocked: false,
                reason: "rejected-by-host-validator".to_owned(),
            }
        };

        recorder.record(ReplayEntry {
            tick: self.tick,
            agent_id: obs.agent_id,
            observation: ObservationRecord::from(&obs),
            agent_before,
            intent: IntentRecord::from(&intent),
            accepted,
            applied,
            note: String::new(),
        });
        Ok((truth, record))
    }

    fn target_is_live(&self, id: u32) -> bool {
        self.player.is_some_and(|p| p.id == id)
            || self.objects.iter().any(|o| o.id == id)
            || self.decoy.is_some_and(|d| d.id == id)
            || self.scripted_present(id).is_some()
            || id == 0
    }
}

/// Where things really are on one tick, as the corridor's own state has them.
///
/// This is the world's truth, not the guard's perception, and it is recorded
/// beside the replay rather than inside it so the replay format is unchanged.
/// It is what lets a reader see where the guard is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickTruth {
    pub tick: u64,
    /// The room the guard stands in when it perceives. Perception needs a
    /// shared room, so this is also the room of every contact it reports.
    pub guard_room: usize,
    /// `(contact id, room index)` for every entity the world holds, in a
    /// stable order: the player, then the objects, then the scripted cast.
    pub contacts: Vec<(u32, usize)>,
    /// The ids that perceptibly reached the guard: the contacts in its
    /// observation this tick, in its order.
    pub visible: Vec<u32>,
    /// The scripted contacts that hit the guard this tick.
    pub hits: Vec<u32>,
    /// The guard's health and ammunition after this tick's hits, in fx.
    pub guard_health: i32,
    pub guard_ammo: i32,
}

impl HostWorld {
    /// The world's truth on the current tick.
    ///
    /// * The player is at the scripted `player_x` on every tick. The visible
    ///   entity (`player`) exists only on the schedule's ticks, but the
    ///   script keeps the position between them and resumes from it, so the
    ///   infiltrator is somewhere real while unseen.
    /// * Each object is where it stands.
    /// * A scripted cast replaces the player: each member is where it stands
    ///   while present, and absent otherwise.
    /// * The decoy is absent. It is "present to perception (but not to
    ///   truth)" (see [`Decoy`]): no entity stands behind the contact, so a
    ///   belief that it is somewhere is never true.
    pub fn truth(&self) -> TickTruth {
        let mut contacts = Vec::new();
        if self.scripted.is_none() {
            contacts.push((1, self.room_at(self.player_x)));
        }
        contacts.extend(self.objects.iter().map(|o| (o.id, self.room_at(o.x))));
        for c in self.scripted.iter().flatten() {
            if present(c, self.tick) {
                contacts.push((c.id, self.room_at(c.x)));
            }
        }
        TickTruth {
            tick: self.tick,
            guard_room: self.room_at(self.guard.x),
            contacts,
            visible: Vec::new(),
            hits: self.hits_this_tick.clone(),
            guard_health: to_fx(self.guard.health),
            guard_ammo: to_fx(self.guard.ammo),
        }
    }
}

/// Whether a scripted contact is in the corridor on `tick`.
fn present(c: &ScriptedContact, tick: u64) -> bool {
    (c.present_from..c.present_until).contains(&tick)
}

/// The observation's contact list, exposed for the developer view.
impl HostWorld {
    /// The contacts the guard perceived this tick.
    pub fn contacts(&self, profile: &Profile) -> Vec<ContactRecord> {
        self.observation(profile)
            .contacts
            .iter()
            .map(ContactRecord::from)
            .collect()
    }
}

/// Guard the world's scale field the way the profile owns it.
impl HostWorld {
    /// Set the fx-per-world-unit scale (mirrors the profile).
    pub fn set_scale(&mut self, scale: f64) {
        self.profile_scale = scale;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::ProfileSet;

    #[test]
    fn the_guard_cannot_pass_a_closed_door() {
        let mut w = HostWorld::canonical(&ProfileSet::canonical().cautious_sentry);
        w.guard.x = 100.0;
        // A step large enough to cross the door at x=200 if it were open.
        let (_, blocked) = w.move_toward(900.0, 200.0);
        assert!(blocked, "the guard must not pass through a closed door");
        assert!(w.guard.x < 200.0, "the guard stays on its side of the wall");
    }

    #[test]
    fn unknown_targets_are_refused() {
        let profile = ProfileSet::canonical().cautious_sentry;
        let mut w = HostWorld::canonical(&profile);
        let intent = Intent {
            action: Action::Attack,
            target_id: 9999,
            r#move: Fx2::ZERO,
            look: Fx2::ZERO,
            speed: 100,
            priority: 0,
            budget_used: 0,
            flags: Default::default(),
        };
        assert!(!w.target_is_live(9999));
        assert_eq!(w.apply(&intent, &profile).reason, "attack-invalid-target");
    }

    #[test]
    fn a_guard_pressed_against_a_closed_door_reports_blocked_without_moving() {
        let profile = ProfileSet::canonical().cautious_sentry;
        let mut w = HostWorld::canonical(&profile);
        // One step from the door at x=200, advancing along the patrol route.
        w.guard.x = 199.0;
        let intent = Intent {
            action: Action::Advance,
            target_id: 0,
            r#move: Fx2::ZERO,
            look: Fx2::ZERO,
            // The kernel's own advance speed: 1.0 fx, which the corridor turns
            // into 8 canvas units and then clamps to the profile's limit.
            speed: crate::abi::FX_ONE,
            priority: 0,
            budget_used: 0,
            flags: Default::default(),
        };
        let effect = w.apply(&intent, &profile);
        assert!(effect.blocked, "the closed door blocks the advance");
        assert!(
            !effect.moved,
            "a tick that moved nothing must not be recorded as a move"
        );
        assert!(w.guard.x < 200.0, "the guard stays on its side of the wall");
    }

    #[test]
    fn the_movement_limit_is_applied_in_world_units() {
        let profile = ProfileSet::canonical().cautious_sentry;
        let mut w = HostWorld::canonical(&profile);
        // Far from any door, so the step is a full one.
        w.guard.x = 100.0;
        let intent = Intent {
            action: Action::Advance,
            target_id: 0,
            r#move: Fx2::ZERO,
            look: Fx2::ZERO,
            speed: crate::abi::FX_ONE,
            priority: 0,
            budget_used: 0,
            flags: Default::default(),
        };
        let effect = w.apply(&intent, &profile);
        assert!(!effect.blocked, "nothing blocks a step inside a room");
        let moved = w.guard.x - 100.0;
        assert!(
            (moved - profile.host.max_speed_world).abs() < 1e-9,
            "the guard must move the profile's world limit, not the kernel's \
             8 canvas units (moved {moved})"
        );
        assert_eq!(effect.speed_applied, to_fx(profile.host.max_speed_world));
    }

    #[test]
    fn the_guard_observes_only_what_it_can_perceive() {
        let profile = ProfileSet::canonical().cautious_sentry;
        let mut w = HostWorld::canonical(&profile);
        // No player present: nothing to see.
        assert!(w.observation(&profile).contacts.is_empty());
        // A player in the guard's own room, close enough to engage.
        w.player = Some(Entity {
            id: 1,
            x: 60.0,
            kind: EntityKind::Player,
        });
        let obs = w.observation(&profile);
        assert!(
            obs.contacts
                .iter()
                .any(|c| c.source == ContactSource::Player),
            "the guard sees the player when it is in the same room and in range"
        );
        // A player in another room is not reported.
        w.player = Some(Entity {
            id: 1,
            x: 700.0,
            kind: EntityKind::Player,
        });
        assert!(
            w.observation(&profile)
                .contacts
                .iter()
                .all(|c| c.source != ContactSource::Player),
            "a guard does not see through walls"
        );
    }
}
