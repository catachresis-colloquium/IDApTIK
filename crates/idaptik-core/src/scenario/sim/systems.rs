//! The twelve simulation systems, in the fixed pipeline order. Each is an
//! `impl GhostLobbySim` method the tick loop calls in sequence. The order is
//! load-bearing (belief reads the player's NEW position and Billy's OLD one;
//! Billy moves only in `system_billy`; collisions see Billy's NEW position).
//!
//! The behaviour and FSM systems are parameterised by the sim's composed
//! [`crate::scenario::actor::ComposedActor`]: every speed, timer, engagement
//! distance and belief number comes from the archetype (Billy is the default),
//! and the interest arithmetic goes through the shared engine in
//! [`crate::scenario::actor::belief`] rather than a private copy.

use super::GhostLobbySim;
use crate::netsim::Effect;
use crate::scenario::actor::{self, Leakage, belief};
use crate::scenario::agents::apply_effects;
use crate::scenario::command::{Button, Edge, TickInput};
use crate::scenario::common::{
    BillyMode, ChuteMethod, CrisisReason, ExtractMethod, ObjectKind, ObjectiveStatus, Phase,
    ReportedTarget,
};
use crate::scenario::constants as c;
use crate::scenario::event::Event;
use crate::scenario::floor_graph::{camera_node_id, valve_node_id};
use crate::scenario::ids::RoomId;
use crate::scenario::mathf::{TICK_DT, approach, clamp, dist, powf, sin};
use crate::scenario::state::Place;

impl GhostLobbySim {
    // 1. TIMERS ------------------------------------------------------------
    pub(super) fn system_timers(&mut self) {
        let dt = TICK_DT;
        let s = &mut self.state;
        s.camera_ping = (s.camera_ping - dt).max(0.0);
        s.lights_flicker = (s.lights_flicker - dt).max(0.0);
        s.camera_lockout = (s.camera_lockout - dt).max(0.0);
        s.player.caught_grace = (s.player.caught_grace - dt).max(0.0);
        // Looped footage runs out camera by camera, in definition order.
        for loop_left in &mut s.camera_looped {
            *loop_left = (*loop_left - dt).max(0.0);
        }
        for a in s.actions.values_mut() {
            a.cd = (a.cd - dt).max(0.0);
        }
        s.net_hack_cd = (s.net_hack_cd - dt).max(0.0);
        let mut hold_events: Vec<Event> = Vec::new();
        for door in &mut s.doors {
            door.open = (door.open - dt).max(0.0);
            if door.pending > 0.0 {
                door.pending -= dt;
                if door.pending <= 0.0 {
                    door.open = door.route_duration;
                    hold_events.push(Event::DoorHoldActive {
                        door: door.id.clone(),
                        duration: door.route_duration,
                    });
                }
            }
        }
        let regen_factor = c::BW_REGEN_A + s.support * c::BW_REGEN_B;
        s.bandwidth = clamp(
            s.bandwidth + self.preset.bandwidth_regen * regen_factor * dt,
            0.0,
            100.0,
        );
        let decay = if s.phase == Phase::Quiet {
            c::ALERT_DECAY_QUIET
        } else {
            c::ALERT_DECAY_CRISIS
        };
        s.alert = clamp(s.alert - decay * dt, 0.0, 100.0);
        s.max_alert = s.max_alert.max(s.alert);
        self.events.append(&mut hold_events);
    }

    // 2. PLAYER ------------------------------------------------------------
    pub(super) fn system_player(&mut self, input: &TickInput) {
        let dt = TICK_DT;
        // On the rope the body only goes up or down it.
        if let Place::Rope { down } = self.state.player.place {
            self.rope_step(down);
            return;
        }
        let old_x = self.state.player.x;
        let left = input.buttons.has(Button::Left);
        let right = input.buttons.has(Button::Right);
        let crouch = input.buttons.has(Button::Crouch);
        let sprint_wanted = input.buttons.has(Button::Sprint)
            && self.state.player.stamina > c::SPRINT_MIN_STAM
            && !crouch;
        let dir = (right as i32 - left as i32) as f64;
        self.state.player.crouching = crouch;
        self.state.player.sprinting = sprint_wanted && dir != 0.0;

        if self.state.player.sprinting {
            self.state.player.stamina = (self.state.player.stamina - c::STAM_DRAIN * dt).max(0.0);
            self.state.stats.sprints += dt;
        } else {
            let regen = if crouch {
                c::STAM_REGEN_CROUCH
            } else {
                c::STAM_REGEN
            };
            self.state.player.stamina = (self.state.player.stamina + regen * dt).min(100.0);
        }

        let base = if self.state.player.sprinting {
            self.preset.sprint
        } else {
            self.preset.player_speed
        };
        // Released water is knee-deep on the floor line (ADR-0013): it slows
        // the player to a wade wherever it has been let in. The roof never
        // floods, and a dry floor keeps the speed bit-identical to before.
        let wade = if self.state.player.place == Place::Floor {
            self.wade_factor_at(self.state.player.x)
        } else {
            1.0
        };
        let speed = base * if crouch { c::CROUCH_SPEED } else { 1.0 } * wade;
        let target_vx = dir * speed;
        let accel = if dir != 0.0 {
            c::ACCEL_DRIVE
        } else {
            c::ACCEL_BRAKE
        };
        self.state.player.vx = approach(self.state.player.vx, target_vx, accel * dt);
        if dir != 0.0 {
            self.state.player.facing = dir;
        }

        if input.edges.contains(&Edge::JumpUp) && self.state.player.grounded && !crouch {
            self.state.player.vy = c::JUMP_VY;
            self.state.player.grounded = false;
            self.state.player.noise = self.state.player.noise.max(c::NOISE_JUMP);
            self.state.stats.jumps += 1;
        }

        self.state.player.vy += c::GRAVITY * dt;
        self.state.player.x += self.state.player.vx * dt;
        self.state.player.y += self.state.player.vy * dt;
        let h = self.def.player.h;
        // The roof is the ground up there; the floor line, indoors and out.
        let place = self.state.player.place;
        let ext = self.def.exterior.as_ref();
        let ground = match (place, ext) {
            (Place::Roof, Some(e)) => e.roof_y,
            _ => c::FLOOR,
        };
        if self.state.player.y + h >= ground {
            if !self.state.player.grounded && self.state.player.vy > c::LAND_VY {
                self.state.player.noise = self.state.player.noise.max(c::NOISE_LAND);
            }
            self.state.player.y = ground - h;
            self.state.player.vy = 0.0;
            self.state.player.grounded = true;
        }

        let (lo, hi) = match (place, ext) {
            (Place::Roof, Some(e)) => e.roof,
            (_, Some(e)) => (e.street_lo, c::PLAYER_CLAMP_HI),
            _ => (c::PLAYER_CLAMP_LO, c::PLAYER_CLAMP_HI),
        };
        self.state.player.x = clamp(self.state.player.x, lo, hi);
        // Doors stand on the floor line; the roof has none.
        let (nx, blocked) = if place == Place::Floor {
            Self::constrain_by_doors(
                &self.state.doors,
                old_x,
                self.state.player.x,
                self.def.player.w,
            )
        } else {
            (self.state.player.x, false)
        };
        self.state.player.x = nx;
        if blocked {
            self.state.player.vx = 0.0;
        }

        // hidden
        let cx = self.state.player.x + self.def.player.w / 2.0;
        let room_id = Self::room_id_at(&self.def, self.state.player.x).map(|s| s.to_owned());
        let outside = self.infiltrator_outside();
        let spot = self.def.hide_spots.iter().position(|hspot| {
            !outside
                && dist(cx, hspot.x) <= hspot.radius
                && room_id.as_deref() == Some(hspot.room.as_str())
        });
        self.state.player.hide_spot = spot;
        self.state.player.hidden = crouch
            && spot.is_some()
            && self.state.player.vx.abs() < c::HIDE_MAX_VX
            && self.state.player.grounded;
        if self.state.player.hidden {
            self.state.stats.hidden_seconds += dt;
        }

        // noise (all contributions max()'d, then a single decay)
        let movement_noise = self.state.player.vx.abs() / self.preset.sprint;
        if self.state.player.sprinting {
            self.state.player.noise = self
                .state
                .player
                .noise
                .max(c::NOISE_SPRINT_BASE + movement_noise * c::NOISE_SPRINT_K);
        } else if self.state.player.vx.abs() > c::NOISE_MOVE_MIN_VX {
            let n = if crouch {
                c::NOISE_CROUCH
            } else {
                c::NOISE_WALK_K * movement_noise
            };
            self.state.player.noise = self.state.player.noise.max(n);
        }
        // Wading through released water is loud: the same flood that hides a
        // hack announces the wader to every ear in the room. A floor with no
        // water contributes nothing here.
        if self.state.player.place == Place::Floor
            && self.state.player.vx.abs() > c::NOISE_MOVE_MIN_VX
        {
            let wade_noise = self.wade_noise_at(self.state.player.x);
            if wade_noise > 0.0 {
                self.state.player.noise = self.state.player.noise.max(wade_noise);
            }
        }
        self.state.player.noise = (self.state.player.noise - c::NOISE_DECAY * dt).max(0.0);
    }

    // 3. INTERACTIONS ------------------------------------------------------
    pub(super) fn system_interactions(&mut self, input: &TickInput) {
        let dt = TICK_DT;
        let pw = self.def.player.w;
        let cx = self.state.player.x + pw / 2.0;
        let interact_held = input.buttons.has(Button::Interact);
        let interact_pressed = input.edges.contains(&Edge::InteractPress);
        let jump_pressed = input.edges.contains(&Edge::JumpUp);
        let throw = input.edges.contains(&Edge::Throw);
        // Outside, only the outside's own handholds: the rope, the hatches
        // and the front door's lock.
        if self.def.exterior.is_some() {
            if self.exterior_interaction(interact_pressed, interact_held) {
                return;
            }
            if self.infiltrator_outside() {
                return;
            }
        }
        let room = Self::room_id_at(&self.def, self.state.player.x).map(|s| s.to_owned());
        let room = room.as_deref();
        let mut active = false;

        // note peel
        if !active
            && !self.state.note.held
            && !self.state.note.billy_has
            && dist(cx, self.state.note.x) < c::NOTE_DIST
            && room == Some("kitchen")
        {
            active = true;
            if interact_held {
                self.state.note.progress += dt;
                self.state.player.vx *= 0.72;
                if self.state.phase == Phase::Crisis && self.can_billy_see_player() {
                    self.state.billy.note_interest = clamp(
                        self.state.billy.note_interest + c::PEEL_INTEREST * dt,
                        0.0,
                        100.0,
                    );
                    if !self.state.note.exposed {
                        self.state.note.exposed = true;
                        self.events.push(Event::NoteExposed);
                    }
                    self.state.stats.note_exposed = true;
                }
                if self.state.note.progress >= c::NOTE_HOLD {
                    self.secure_note();
                }
            } else {
                self.state.note.progress = (self.state.note.progress - 1.5 * dt).max(0.0);
            }
        }

        // usb take
        if !active
            && !self.state.usb.held
            && !self.state.usb.billy_has
            && dist(cx, self.state.usb.x) < c::USB_DIST
            && room == Some("office")
        {
            active = true;
            if interact_pressed {
                self.take_usb();
            }
        }

        // chute enter
        if !active
            && self.state.chute.revealed
            && dist(cx, self.state.chute.x) < c::CHUTE_ENTER
            && room == Some("laundry")
        {
            active = true;
            if interact_pressed || jump_pressed {
                self.extract(ExtractMethod::LaundryChute);
                return;
            }
        }

        // chute search
        if !active
            && !self.state.chute.revealed
            && dist(cx, self.state.chute.x - c::CHUTE_SEARCH_OFF) < c::CHUTE_SEARCH_DIST
            && room == Some("laundry")
        {
            active = true;
            if interact_held {
                self.state.chute.progress += dt;
                self.state.player.vx *= 0.68;
                if self.state.chute.progress >= c::CHUTE_HOLD {
                    self.reveal_chute(ChuteMethod::Physical);
                }
            } else {
                self.state.chute.progress = (self.state.chute.progress - 1.2 * dt).max(0.0);
            }
        }

        // valve handle — the physical release (ADR-0013). A floor that
        // declares a water zone with a handle in this room lets the
        // infiltrator turn it by hand, and the zone releases through the
        // same canonical path a hacked valve node walks: the splash, the
        // shorts, the alarm and the guard's reaction are identical whichever
        // way the water was let in.
        if !active {
            if let Some(zone) = self.valve_handle_at(room, cx) {
                active = true;
                if interact_held {
                    self.state.player.valve_turn += dt;
                    self.state.player.vx *= 0.7;
                    if self.state.player.valve_turn >= c::VALVE_TURN_HOLD {
                        self.state.player.valve_turn = 0.0;
                        let effects = vec![Effect::WaterReleased(valve_node_id(
                            zone.as_str(),
                        ))];
                        let events =
                            apply_effects(&self.def, &mut self.state, self.cfg, &effects);
                        self.events.extend(events);
                    }
                } else {
                    self.state.player.valve_turn =
                        (self.state.player.valve_turn - 1.5 * dt).max(0.0);
                }
            }
        }

        // console use: the lowest ordinary priority, so it never steals the
        // note, the USB or the chute (floors without consoles never get here
        // with anything to use)
        if !active && self.console_interaction(interact_pressed) {
            active = true;
        }

        // pickpocket during lights-out
        let billy_center = self.state.billy.x + self.def.billy.w / 2.0;
        if !active
            && self.state.note.billy_has
            && dist(cx, billy_center) < c::PICKPOCKET_DIST
            && self.state.lights_flicker > 0.0
        {
            // Pickpocket is the last priority; `active` is not read afterwards.
            if interact_held {
                self.state.player.interaction_progress += dt;
                if self.state.player.interaction_progress >= c::PICKPOCKET_HOLD {
                    self.state.note.billy_has = false;
                    self.state.billy.has_note = false;
                    self.state.player.has_note = true;
                    self.state.note.held = true;
                    self.state.billy.player_interest = 100.0;
                    self.state.player.interaction_progress = 0.0;
                    self.set_billy_mode(BillyMode::Pursue);
                    self.events.push(Event::PickpocketSucceeded);
                }
            } else {
                self.state.player.interaction_progress = 0.0;
            }
        } else {
            self.state.player.interaction_progress = 0.0;
        }

        // service-exit extraction (x > EXIT_PROMPT_X shows the prompt; x >= EXIT_X extracts)
        if room == Some("exit")
            && self.state.player.x > c::EXIT_PROMPT_X
            && self.state.player.x >= c::EXIT_X
        {
            self.extract(ExtractMethod::ServiceExit);
            return;
        }

        // throw
        if self.state.player.has_usb && throw {
            self.throw_usb();
        }
    }

    /// One tick on the rope: down to the street, or up to the roof.
    fn rope_step(&mut self, down: bool) {
        let dt = TICK_DT;
        let (w, h) = (self.def.player.w, self.def.player.h);
        let Some((rope, roof_y)) = self.def.exterior.as_ref().map(|e| (e.rope, e.roof_y)) else {
            self.state.player.place = Place::Floor;
            return;
        };
        let p = &mut self.state.player;
        p.vx = 0.0;
        p.vy = 0.0;
        p.grounded = false;
        p.sprinting = false;
        p.hidden = false;
        p.hide_spot = None;
        p.x = rope.foot_x - w / 2.0;
        if down {
            p.y += rope.speed * dt;
            if p.y + h >= c::FLOOR {
                p.y = c::FLOOR - h;
                p.place = Place::Floor;
                p.grounded = true;
            }
        } else {
            p.y -= rope.speed * dt;
            if p.y + h <= roof_y {
                p.y = roof_y - h;
                p.x = rope.top_x - w / 2.0;
                p.place = Place::Roof;
                p.grounded = true;
            }
        }
        p.noise = (p.noise - c::NOISE_DECAY * dt).max(0.0);
    }

    /// The outside's handholds, in order: the rope (top or foot), a hatch,
    /// and from the street the front door's lock. Returns whether one was in
    /// reach (it then owns the interact key this tick).
    fn exterior_interaction(&mut self, pressed: bool, held: bool) -> bool {
        let dt = TICK_DT;
        let Some(ext) = self.def.exterior.clone() else {
            return false;
        };
        let w = self.def.player.w;
        let cx = self.state.player.x + w / 2.0;
        match self.state.player.place {
            Place::Roof => {
                if dist(cx, ext.rope.top_x) <= c::ROPE_REACH {
                    if pressed {
                        self.state.player.place = Place::Rope { down: true };
                        self.state.player.x = ext.rope.foot_x - w / 2.0;
                        self.events.push(Event::RopeTaken { down: true });
                    }
                    return true;
                }
                if let Some(hatch) = ext.hatches.iter().find(|h| dist(cx, h.x) <= c::HATCH_REACH) {
                    if pressed {
                        // Down through the ceiling; the fall makes its own noise.
                        let p = &mut self.state.player;
                        p.place = Place::Floor;
                        p.x = hatch.x - w / 2.0;
                        p.y = c::FLOOR - c::ROOM_H;
                        p.vx = 0.0;
                        p.vy = 0.0;
                        p.grounded = false;
                        self.events.push(Event::HatchDropped {
                            hatch: hatch.id.clone(),
                        });
                    }
                    return true;
                }
                false
            }
            Place::Floor if self.infiltrator_outside() => {
                if dist(cx, ext.rope.foot_x) <= c::ROPE_REACH {
                    if pressed {
                        self.state.player.place = Place::Rope { down: false };
                        self.events.push(Event::RopeTaken { down: false });
                    }
                    return true;
                }
                let Some(i) = self.state.doors.iter().position(|d| d.id == ext.front_door) else {
                    return false;
                };
                let door = &self.state.doors[i];
                let locked = !door.unlocked && door.open <= 0.0;
                if !(locked && cx < door.x && door.x - cx <= c::PICK_REACH) {
                    return false;
                }
                let p = &mut self.state.player;
                if held {
                    p.lockpick += dt;
                    p.vx *= 0.7;
                    p.noise = p.noise.max(c::NOISE_PICK);
                    if p.lockpick >= ext.pick_time {
                        p.lockpick = 0.0;
                        self.state.doors[i].unlocked = true;
                        self.events.push(Event::FrontDoorPicked);
                    }
                } else {
                    p.lockpick = (p.lockpick - 1.5 * dt).max(0.0);
                }
                true
            }
            _ => false,
        }
    }

    fn secure_note(&mut self) {
        if self.state.note.held || self.state.note.billy_has {
            return;
        }
        self.state.note.held = true;
        self.state.player.has_note = true;
        self.state.note.progress = c::NOTE_HOLD;
        let seen = self.state.phase == Phase::Crisis && self.can_billy_see_player();
        if seen {
            self.state.billy.note_interest = clamp(
                self.state.billy.note_interest + c::SECURE_NOTE_SEEN,
                0.0,
                100.0,
            );
            if !self.state.note.exposed {
                self.state.note.exposed = true;
                self.events.push(Event::NoteExposed);
            }
            self.state.stats.note_exposed = true;
        }
        self.events.push(Event::NoteSecured { seen });
    }

    fn take_usb(&mut self) {
        if self.state.usb.held || self.state.usb.billy_has {
            return;
        }
        self.state.usb.held = true;
        self.state.usb.on_floor = false;
        self.state.player.has_usb = true;
        self.state.usb.timer = self.preset.usb_timer;
        let seen = self.state.phase == Phase::Crisis && self.can_billy_see_player();
        let gain = if seen {
            c::TAKE_USB_SEEN
        } else {
            c::TAKE_USB_UNSEEN
        };
        self.state.billy.usb_interest = clamp(self.state.billy.usb_interest + gain, 0.0, 100.0);
        self.events.push(Event::UsbTaken { seen });
        if self.state.phase == Phase::Quiet {
            self.begin_crisis(CrisisReason::Usb);
        }
    }

    fn throw_usb(&mut self) {
        if !self.state.player.has_usb {
            return;
        }
        let facing = self.state.player.facing;
        let px = self.state.player.x;
        let py = self.state.player.y;
        let pw = self.def.player.w;
        self.state.player.has_usb = false;
        self.state.usb.held = false;
        self.state.usb.thrown = true;
        self.state.usb.billy_has = false;
        self.state.usb.x = px + pw / 2.0 + facing * c::USB_THROW_OFF;
        self.state.usb.y = py + 18.0;
        self.state.usb.vx = facing * c::USB_THROW_VX;
        self.state.usb.vy = c::USB_THROW_VY;
        self.state.billy.usb_interest = 100.0;
        self.state.billy.target = Some(ObjectKind::Usb);
        self.state.billy.belief = Some(ObjectKind::Usb);
        if !matches!(
            self.state.billy.mode,
            BillyMode::CallBoss | BillyMode::Pursue
        ) {
            self.set_billy_mode(BillyMode::Secure);
        }
        self.state.alert = clamp(
            self.state.alert + c::USB_THROW_ALERT * self.preset.alert_gain,
            0.0,
            100.0,
        );
        self.events.push(Event::UsbThrown);
    }

    fn reveal_chute(&mut self, method: ChuteMethod) {
        if self.state.chute.revealed {
            return;
        }
        self.state.chute.revealed = true;
        self.state.chute.progress = c::CHUTE_HOLD;
        self.state.stats.chute_revealed_by = Some(method);
        self.events.push(Event::ChuteRevealed { method });
    }

    // 4. USB ---------------------------------------------------------------
    pub(super) fn system_usb(&mut self) {
        let dt = TICK_DT;
        if self.state.usb.held {
            self.state.usb.x = self.state.player.x
                + self.def.player.w / 2.0
                + self.state.player.facing * c::USB_HELD_OFF;
            self.state.usb.y = self.state.player.y + 19.0;
        }
        if self.state.usb.thrown && !self.state.usb.billy_has {
            self.state.usb.vy += c::USB_GRAV * dt;
            self.state.usb.x += self.state.usb.vx * dt;
            self.state.usb.y += self.state.usb.vy * dt;
            self.state.usb.vx *= powf(c::USB_DRAG, dt * 60.0);
            if self.state.usb.y >= c::USB_FLOOR {
                self.state.usb.y = c::USB_FLOOR;
                self.state.usb.vy *= c::USB_BOUNCE;
                self.state.usb.vx *= c::USB_FRICTION;
                if self.state.usb.vx.abs() < c::USB_REST_VX
                    && self.state.usb.vy.abs() < c::USB_REST_VY
                {
                    self.state.usb.vx = 0.0;
                    self.state.usb.vy = 0.0;
                    self.state.usb.on_floor = true;
                }
            }
            self.state.usb.x = clamp(self.state.usb.x, c::USB_CLAMP_LO, c::USB_CLAMP_HI);
        }
        if (self.state.usb.held || self.state.usb.thrown)
            && !self.state.usb.wiped
            && !self.state.usb.billy_has
            && self.state.usb.timer > 0.0
        {
            self.state.usb.timer = (self.state.usb.timer - dt).max(0.0);
            if self.state.usb.timer <= 0.0 {
                self.state.usb.wiped = true;
                self.state.stats.usb_trace = true;
                self.state.alert = clamp(
                    self.state.alert + c::USB_WIPE_ALERT * self.preset.alert_gain,
                    0.0,
                    100.0,
                );
                self.events.push(Event::UsbSelfWiped);
            }
        }
    }

    // 5. VACUUM ------------------------------------------------------------
    pub(super) fn system_vacuum(&mut self) {
        let dt = TICK_DT;
        if !self.state.vacuum.active || self.state.vacuum.fallen {
            return;
        }
        if self.state.vacuum.x > c::VAC_LAG_X {
            self.state.vacuum.control =
                (self.state.vacuum.control - c::VAC_CTRL_LOSS * dt).max(c::VAC_CTRL_MIN);
            if !self.state.vacuum.lag_warned && self.state.vacuum.control < c::VAC_LAG_WARN {
                self.state.vacuum.lag_warned = true;
                self.events.push(Event::VacuumLagWarned);
            }
        } else {
            self.state.vacuum.control =
                (self.state.vacuum.control + c::VAC_CTRL_GAIN * dt).min(1.0);
        }
        self.state.vacuum.x += c::VAC_SPEED * self.state.vacuum.control.max(c::VAC_MOVE_MIN) * dt;
        if self.state.phase == Phase::Crisis
            && self.state.billy.belief.is_none()
            && dist(self.state.billy.x, self.state.vacuum.x) < self.actor.stats.distract_dist
        {
            self.state.billy.last_known_x = self.state.vacuum.x;
            if !matches!(
                self.state.billy.mode,
                BillyMode::Pursue | BillyMode::Secure | BillyMode::Guard | BillyMode::CallBoss
            ) {
                self.set_billy_mode(BillyMode::Investigate);
            }
        }
        if self.state.vacuum.x >= self.state.chute.x - c::VAC_FALL_OFF {
            self.state.vacuum.active = false;
            self.state.vacuum.fallen = true;
            self.reveal_chute(ChuteMethod::Vacuum);
            self.events.push(Event::VacuumFell);
        }
    }

    // 6. CAMERAS -----------------------------------------------------------
    pub(super) fn system_cameras(&mut self) {
        let dt = TICK_DT;
        if self.state.camera_lockout > 0.0 || self.state.lights_flicker > 0.0 {
            self.state.camera_detection =
                (self.state.camera_detection - c::CAM_DECAY_OFF * dt).max(0.0);
            return;
        }
        let ridx = Self::room_index_at(&self.def, self.state.player.x);
        let room_id = self.def.rooms.get(ridx).map(|r| r.id.clone());
        let seen = (0..self.def.cameras.len()).any(|i| self.camera_sees_player(i));
        if seen {
            self.state.camera_detection += dt;
            if self.state.camera_detection >= self.preset.camera_lock {
                self.state.camera_detection = 0.0;
                self.state.camera_lockout = c::CAM_LOCKOUT;
                self.state.camera_seen_count += 1;
                self.state.stats.camera_detections += 1;
                self.state.alert = clamp(
                    self.state.alert + c::CAM_ALERT * self.preset.alert_gain,
                    0.0,
                    100.0,
                );
                self.state.billy.last_known_x = self.state.player.x;
                self.state.billy.player_interest = clamp(
                    self.state.billy.player_interest + c::CAMERA_PLAYER_INT,
                    0.0,
                    100.0,
                );
                if self.state.phase == Phase::Crisis
                    && !matches!(
                        self.state.billy.mode,
                        BillyMode::Secure | BillyMode::Guard | BillyMode::CallBoss
                    )
                {
                    self.set_billy_mode(BillyMode::Investigate);
                }
                let room = room_id.unwrap_or_else(|| RoomId::from(""));
                self.events.push(Event::CameraFlag { room });
            }
        } else {
            self.state.camera_detection =
                (self.state.camera_detection - c::CAM_DECAY_UNSEEN * dt).max(0.0);
        }
    }

    // 7. SUPPORT -----------------------------------------------------------
    pub(super) fn system_support(&mut self) {
        let dt = TICK_DT;
        let ridx = Self::room_index_at(&self.def, self.state.player.x);
        let (base, ping_bonus) = match &self.def.exterior {
            // Outside, the van is in plain sight.
            Some(ext) if self.infiltrator_outside() => (ext.support, 0.0),
            _ => self
                .def
                .rooms
                .get(ridx)
                .map(|r| (r.support, r.ping_support_bonus))
                .unwrap_or((c::SUPPORT_CLAMP_MIN, 0.0)),
        };
        let mut target = base;
        if self.state.camera_ping > 0.0 {
            target += ping_bonus;
        }
        if self.state.bandwidth < c::SUPPORT_LOWBW_X {
            target -= c::SUPPORT_LOWBW_PEN;
        }
        target -= self.state.alert * c::SUPPORT_ALERT_PEN;
        if self.state.player.hidden {
            target += c::SUPPORT_HIDDEN;
        }
        if self.state.lights_flicker > 0.0 {
            target += c::SUPPORT_FLICKER;
        }
        // The room base is the infiltrator's own vantage; this is the hacker's.
        // Every pivot they stand on is another leg the link must carry, so depth
        // costs whatever the room grants. It is charged inside the clamp, so the
        // envelope still cannot leave [SUPPORT_CLAMP_MIN, 1].
        target -= f64::from(self.state.agents.hacker.hops()) * c::SUPPORT_HOP_PEN;
        target = clamp(target, c::SUPPORT_CLAMP_MIN, 1.0);
        self.state.support = approach(self.state.support, target, c::SUPPORT_APPROACH * dt);

        if self.state.phase == Phase::Crisis
            && self.state.support < c::ISO_GATE
            && !self.state.player.hidden
        {
            let pressure = 1.0 + self.state.alert / c::ISO_PRESSURE_DIV;
            self.state.isolation += dt * pressure;
            self.state.stats.support_broken_time += dt;
            let limit = self.preset.support_limit;
            if self.state.isolation > limit * c::FRAY_FRAC {
                let t = self.state.t;
                if t - self.state.throttles.support_fray >= c::THROTTLE_FRAY {
                    self.state.throttles.support_fray = t;
                    self.events.push(Event::SupportFraying);
                }
            }
            if self.state.isolation >= limit {
                self.state.stats.max_isolation =
                    self.state.stats.max_isolation.max(self.state.isolation);
                self.fail_mission(crate::scenario::common::FailReason::Partition);
                return;
            }
        } else {
            let decay = if self.state.player.hidden {
                c::ISO_DECAY_HIDDEN
            } else {
                c::ISO_DECAY
            };
            self.state.isolation = (self.state.isolation - decay * dt).max(0.0);
        }
        self.state.stats.max_isolation = self.state.stats.max_isolation.max(self.state.isolation);
    }

    // 8. BEHAVIOUR (belief) ------------------------------------------------
    //
    // The per-object interest model is the archetype's: each tracked object
    // carries an `InterestProfile` (valueSignal economy) and the arithmetic
    // goes through the shared `actor::belief` engine. With the default Billy
    // archetype the numbers are the ported constants, bit for bit.
    pub(super) fn system_behaviour(&mut self) {
        let dt = TICK_DT;
        if self.state.phase != Phase::Crisis || self.state.billy.mode == BillyMode::Offsite {
            return;
        }
        let stats = self.actor.stats;
        let note_prof = self.actor.interest_or_inert(actor::NOTE_OBJECT);
        let usb_prof = self.actor.interest_or_inert(actor::USB_OBJECT);
        let sees = self.can_billy_see_player();
        let pcx = self.state.player.x + self.def.player.w / 2.0;
        let moving = self.state.player.vx.abs() > c::MOVING_VX;
        let sprinting = self.state.player.sprinting;
        let leakage = if sprinting {
            Leakage::Sprinting
        } else if moving {
            Leakage::Moving
        } else {
            Leakage::Still
        };
        if sees {
            self.state.billy.last_known_x = self.state.player.x;
            self.state.billy.last_seen_ago = 0.0;
            let pi = if sprinting {
                stats.pi_sprint
            } else {
                stats.pi_seen
            };
            self.state.billy.player_interest =
                clamp(self.state.billy.player_interest + pi * dt, 0.0, 100.0);
            if dist(pcx, self.state.note.x) < note_prof.near
                || self.state.player.has_note
                || self.state.note.progress > c::PROG_GATE
            {
                self.state.billy.note_interest = belief::interest_observed(
                    self.state.billy.note_interest,
                    &note_prof,
                    leakage,
                    self.state.player.has_note,
                    dt,
                );
            }
            if dist(pcx, self.state.usb.x) < usb_prof.near || self.state.player.has_usb {
                self.state.billy.usb_interest = belief::interest_observed(
                    self.state.billy.usb_interest,
                    &usb_prof,
                    leakage,
                    self.state.player.has_usb,
                    dt,
                );
            }
        } else {
            self.state.billy.last_seen_ago += dt;
            self.state.billy.player_interest =
                (self.state.billy.player_interest - stats.pi_decay * dt).max(0.0);
            self.state.billy.note_interest = belief::interest_unobserved(
                self.state.billy.note_interest,
                &note_prof,
                self.state.note.exposed || self.state.player.has_note,
                dt,
            );
            self.state.billy.usb_interest = belief::interest_unobserved(
                self.state.billy.usb_interest,
                &usb_prof,
                self.state.usb.thrown || self.state.player.has_usb,
                dt,
            );
        }

        // The note is listed first, so the engine's earlier-entry tie-break
        // reproduces the prototype's `note >= usb` rule exactly.
        let belief = belief::belief_over(
            &[
                (ObjectKind::Note, self.state.billy.note_interest),
                (ObjectKind::Usb, self.state.billy.usb_interest),
            ],
            stats.belief_threshold,
        );
        self.state.billy.belief = belief;
        if let Some(b) = belief
            && self.state.billy.belief_announced != Some(b)
        {
            self.state.billy.belief_announced = Some(b);
            self.events.push(Event::BillyBeliefFormed { belief: b });
        }
    }

    // 9. BILLY (FSM) -------------------------------------------------------
    pub(super) fn system_billy(&mut self) {
        let dt = TICK_DT;
        if self.state.phase != Phase::Crisis {
            return;
        }
        if self.state.billy.stun > 0.0 {
            self.state.billy.stun = (self.state.billy.stun - dt).max(0.0);
            return;
        }
        let sees = self.can_billy_see_player();
        if sees {
            self.state.billy.last_known_x = self.state.player.x;
            self.state.billy.last_seen_ago = 0.0;
        }

        // entering / shock return before any transition
        match self.state.billy.mode {
            BillyMode::Entering => {
                let snack = self.state.billy.snack_x;
                self.move_billy_toward(
                    snack,
                    self.preset.billy_speed * self.actor.stats.enter_speed,
                );
                if dist(self.state.billy.x, snack) < c::SNACK_REACH {
                    self.set_billy_mode(BillyMode::Shock);
                    self.state.billy.state_timer = self.actor.stats.shock_t;
                    self.state.billy.vx = 0.0;
                }
                return;
            }
            BillyMode::Shock => {
                self.state.billy.state_timer -= dt;
                if self.state.billy.state_timer <= 0.0 {
                    self.set_billy_mode(BillyMode::Assess);
                    self.state.billy.patrol_target = 320.0;
                }
                return;
            }
            _ => {}
        }

        // pre-switch fall-through
        if self.state.billy.belief.is_some()
            && !self.state.billy.has_note
            && !self.state.billy.has_usb
            && !matches!(
                self.state.billy.mode,
                BillyMode::Secure | BillyMode::Guard | BillyMode::CallBoss
            )
        {
            self.state.billy.target = self.state.billy.belief;
            self.set_billy_mode(BillyMode::Secure);
        }
        if sees
            && dist(self.state.billy.x, self.state.player.x) < self.actor.stats.pursue_trigger
            && !matches!(
                self.state.billy.mode,
                BillyMode::Guard | BillyMode::CallBoss
            )
        {
            self.set_billy_mode(BillyMode::Pursue);
        }

        // exclusive chain on the MUTATED mode
        match self.state.billy.mode {
            BillyMode::Assess => self.billy_assess(sees),
            BillyMode::Investigate => self.billy_investigate(sees),
            BillyMode::Secure => self.billy_secure(),
            BillyMode::Guard => self.billy_guard(sees),
            BillyMode::CallBoss => self.billy_call_boss(sees),
            BillyMode::Pursue => self.billy_pursue(sees),
            _ => {}
        }
    }

    fn billy_assess(&mut self, sees: bool) {
        if self.state.billy.belief.is_some() {
            self.state.billy.target = self.state.billy.belief;
            self.set_billy_mode(BillyMode::Secure);
            return;
        }
        let noise_invest = self.state.player.noise > self.actor.stats.invest_noise
            && dist(self.state.billy.x, self.state.player.x) < self.actor.stats.invest_dist;
        // Building awareness (ADR-0013): when the guard neither sees the
        // player nor hears their footsteps, the strongest unusual noise he
        // can hear — splashing water, arcing shorts, a sounding alarm, a
        // mole's decoy — pulls his attention to where *it* was heard, never
        // to where the player is. His own ears are the only input.
        let infra_pull = if sees || noise_invest {
            None
        } else {
            self.infra_investigation_target()
        };
        if sees || noise_invest || infra_pull.is_some() {
            self.state.billy.last_known_x = infra_pull.unwrap_or(self.state.player.x);
            self.set_billy_mode(if sees {
                BillyMode::Pursue
            } else {
                BillyMode::Investigate
            });
            return;
        }
        let (patrol_lo, patrol_hi, patrol_pivot) = self.effective_patrol_band();
        let target = self.state.billy.patrol_target;
        self.move_billy_toward(
            target,
            self.preset.billy_speed * self.actor.stats.assess_speed,
        );
        if dist(self.state.billy.x, self.state.billy.patrol_target) < 10.0 {
            self.state.billy.patrol_target = if self.state.billy.patrol_target < patrol_pivot {
                patrol_hi
            } else {
                patrol_lo
            };
        }
    }

    /// The Assess ping-pong band — the archetype's raw band, bent toward the
    /// supervision coverage target in a supervised run. The bend is
    /// attention-proportional and threshold-gated, reads only deterministic
    /// state, and draws no RNG, so it composes with the replay/golden gates.
    fn effective_patrol_band(&self) -> (f64, f64, f64) {
        let lo = self.actor.stats.patrol_lo;
        let hi = self.actor.stats.patrol_hi;
        let pivot = self.actor.stats.patrol_pivot;
        if !self.cfg.supervised {
            return (lo, hi, pivot);
        }
        let sup = &self.state.supervision.supervisor;
        let attention = sup.team.attention;
        if attention < c::SUPERVISION_ATTENTION_MIN {
            return (lo, hi, pivot);
        }
        // The coverage target follows the object, not where it was last
        // reported: a decoy that skitters keeps pulling the sweep after it.
        let target_x = match sup.coverage_target.as_deref() {
            Some("usb") => self.state.usb.x,
            Some("fridge_note") => self.state.note.x,
            _ => return (lo, hi, pivot),
        };
        let a = f64::from(attention) / 100.0;
        let band_lo =
            (target_x - c::SUPERVISION_COVERAGE_BAND).clamp(c::BILLY_CLAMP_LO, c::BILLY_CLAMP_HI);
        let band_hi =
            (target_x + c::SUPERVISION_COVERAGE_BAND).clamp(c::BILLY_CLAMP_LO, c::BILLY_CLAMP_HI);
        let lo_eff = lo + (band_lo - lo) * a;
        let hi_eff = hi + (band_hi - hi) * a;
        let (lo_eff, hi_eff) = if lo_eff <= hi_eff {
            (lo_eff, hi_eff)
        } else {
            (hi_eff, lo_eff)
        };
        (lo_eff, hi_eff, (lo_eff + hi_eff) / 2.0)
    }

    fn billy_investigate(&mut self, sees: bool) {
        let target = self.state.billy.last_known_x;
        self.move_billy_toward(
            target,
            self.preset.billy_speed * self.actor.stats.invest_speed,
        );
        if sees {
            self.set_billy_mode(BillyMode::Pursue);
            return;
        }
        if dist(self.state.billy.x, self.state.billy.last_known_x) < 15.0 {
            self.set_billy_mode(BillyMode::Assess);
            self.state.billy.patrol_target = if self.state.billy.x < 280.0 {
                self.actor.stats.patrol_hi
            } else {
                self.actor.stats.patrol_lo
            };
        }
    }

    fn billy_secure(&mut self) {
        let target = self.state.billy.target.or(self.state.billy.belief);
        let mut tx = self.state.billy.x;
        match target {
            Some(ObjectKind::Note) => {
                if self.state.player.has_note {
                    self.set_billy_mode(BillyMode::Pursue);
                    return;
                }
                tx = self.state.note.x;
            }
            Some(ObjectKind::Usb) => {
                if self.state.player.has_usb {
                    self.set_billy_mode(BillyMode::Pursue);
                    return;
                }
                tx = self.state.usb.x;
            }
            None => {}
        }
        self.move_billy_toward(tx, self.preset.billy_speed * self.actor.stats.secure_speed);
        if dist(self.state.billy.x + self.def.billy.w / 2.0, tx) < self.actor.stats.grab_dist {
            match target {
                Some(ObjectKind::Note) if !self.state.note.held && !self.state.note.billy_has => {
                    self.state.note.billy_has = true;
                    self.state.billy.has_note = true;
                    self.state.billy.guard_timer =
                        self.actor.interest_or_inert(actor::NOTE_OBJECT).guard_t;
                    self.state.billy.reported_target = Some(ReportedTarget::Note);
                    self.set_billy_mode(BillyMode::Guard);
                    self.events.push(Event::BillyTookNote);
                }
                Some(ObjectKind::Usb) if !self.state.usb.held && !self.state.usb.billy_has => {
                    self.state.usb.billy_has = true;
                    self.state.billy.has_usb = true;
                    self.state.usb.thrown = false;
                    self.state.usb.vx = 0.0;
                    self.state.usb.vy = 0.0;
                    self.state.billy.guard_timer =
                        self.actor.interest_or_inert(actor::USB_OBJECT).guard_t;
                    self.state.billy.reported_target = Some(ReportedTarget::Usb);
                    self.set_billy_mode(BillyMode::Guard);
                    self.events.push(Event::BillyTookUsb);
                }
                _ => self.set_billy_mode(BillyMode::Pursue),
            }
        }
    }

    fn billy_guard(&mut self, sees: bool) {
        self.state.billy.guard_timer -= TICK_DT;
        if sees && dist(self.state.billy.x, self.state.player.x) < 95.0 {
            self.set_billy_mode(BillyMode::Pursue);
            return;
        }
        let guard_x = if self.state.billy.has_note {
            self.state.note.x
        } else if self.state.billy.has_usb {
            self.state.usb.x
        } else {
            self.state.billy.x
        };
        if dist(self.state.billy.x, guard_x) > 35.0 {
            self.move_billy_toward(
                guard_x,
                self.preset.billy_speed * self.actor.stats.guard_speed,
            );
        }
        if self.state.billy.guard_timer <= 0.0 && !self.state.billy.called {
            self.set_billy_mode(BillyMode::CallBoss);
            self.state.billy.call_timer = self.actor.stats.call_t;
        } else if self.state.billy.called {
            self.set_billy_mode(BillyMode::Pursue);
        }
    }

    fn billy_call_boss(&mut self, sees: bool) {
        self.state.billy.call_timer -= TICK_DT;
        if sees && dist(self.state.billy.x, self.state.player.x) < 72.0 {
            self.set_billy_mode(BillyMode::Pursue);
            return;
        }
        if self.state.billy.call_timer <= 0.0 {
            self.state.billy.called = true;
            self.state.stats.boss_called = true;
            if self.state.billy.reported_target.is_none() {
                self.state.billy.reported_target = Some(match self.state.billy.belief {
                    Some(ObjectKind::Note) => ReportedTarget::Note,
                    Some(ObjectKind::Usb) => ReportedTarget::Usb,
                    None => ReportedTarget::Intruder,
                });
            }
            let reported = self
                .state
                .billy
                .reported_target
                .unwrap_or(ReportedTarget::Intruder);
            self.state.billy.last_known_x = self.state.player.x;
            self.state.alert = clamp(
                self.state.alert + c::CALL_ALERT * self.preset.alert_gain,
                0.0,
                100.0,
            );
            self.events.push(Event::BossCalled { reported });
            self.set_billy_mode(BillyMode::Pursue);
        }
    }

    fn billy_pursue(&mut self, sees: bool) {
        let target_x = if sees {
            self.state.player.x
        } else {
            self.state.billy.last_known_x
        };
        let alert_boost = 1.0
            + self.state.alert / self.actor.stats.alert_boost_div
            + (f64::from(self.state.lights_uses) - 2.0).max(0.0) * self.actor.stats.lights_boost_k;
        self.move_billy_toward(
            target_x,
            self.preset.billy_speed * self.actor.stats.pursue_speed * alert_boost,
        );
        if !sees
            && self.state.billy.last_seen_ago > self.actor.stats.pursue_giveup_ago
            && dist(self.state.billy.x, target_x) < self.actor.stats.pursue_giveup_dist
        {
            let next = if self.state.billy.belief.is_some() {
                BillyMode::Secure
            } else {
                BillyMode::Assess
            };
            self.set_billy_mode(next);
        }
    }

    // 10. COLLISIONS -------------------------------------------------------
    pub(super) fn system_collisions(&mut self) {
        if self.state.phase != Phase::Crisis || self.state.ended {
            return;
        }
        if self.state.player.hidden
            || self.state.lights_flicker > 0.0
            || self.state.billy.stun > 0.0
            || self.state.player.caught_grace > 0.0
        {
            return;
        }
        // Nobody grabs you on the roof, nor through the building's wall.
        if self.state.player.place != Place::Floor
            || self.infiltrator_outside() != self.billy_outside()
        {
            return;
        }
        let pc = self.state.player.x + self.def.player.w / 2.0;
        let bc = self.state.billy.x + self.def.billy.w / 2.0;
        let same_room = Self::room_index_at(&self.def, self.state.player.x)
            == Self::room_index_at(&self.def, self.state.billy.x);
        if !((pc - bc).abs() < c::COLLISION_RADIUS && same_room) {
            return;
        }
        if self.preset.rescue
            && !self.state.stats.rescue_used
            && self.state.support >= c::RESCUE_SUPPORT_MIN
            && self.state.bandwidth >= c::RESCUE_BW_MIN
        {
            self.state.stats.rescue_used = true;
            self.state.bandwidth -= c::RESCUE_BW_COST;
            self.state.billy.stun = c::RESCUE_STUN;
            self.state.player.caught_grace = c::RESCUE_GRACE;
            self.state.player.x += if self.state.player.x < self.state.billy.x {
                -c::RESCUE_DISPLACE
            } else {
                c::RESCUE_DISPLACE
            };
            self.state.player.x = clamp(self.state.player.x, 28.0, 1230.0);
            self.events.push(Event::RescueUsed);
        } else {
            self.fail_mission(crate::scenario::common::FailReason::Caught);
        }
    }

    // 11. OBJECTIVES -------------------------------------------------------
    pub(super) fn system_objectives(&mut self) {
        let note = if self.state.player.has_note {
            ObjectiveStatus::Done
        } else if self.state.note.billy_has {
            ObjectiveStatus::Failed
        } else {
            ObjectiveStatus::Open
        };
        let misled = self.state.billy.has_usb
            || self.state.billy.reported_target == Some(ReportedTarget::Usb)
            || self.state.billy.usb_interest >= c::MISDIRECT_THRESHOLD;
        let misdirect = if misled {
            ObjectiveStatus::Done
        } else if self.state.billy.reported_target == Some(ReportedTarget::Note) {
            ObjectiveStatus::Failed
        } else {
            ObjectiveStatus::Open
        };
        let exit = if self.state.ended && self.state.stats.extraction.is_some() {
            ObjectiveStatus::Done
        } else if self.state.phase == Phase::Crisis {
            ObjectiveStatus::Available
        } else {
            ObjectiveStatus::Locked
        };
        let ledger = (note, misdirect, exit);
        if ledger != self.state.objective_ledger {
            self.state.objective_ledger = ledger;
            self.events.push(Event::ObjectivesUpdated {
                note,
                misdirect,
                exit,
            });
        }
    }
}

impl GhostLobbySim {
    /// Whether camera `i` has the player in frame right now: same room, not
    /// stale, not looped, powered, not switched off or left in the dark by a
    /// wire, and the player inside its sweep and not hiding from it. Excludes
    /// the floor-wide lockout and flicker, which the camera system checks first.
    pub(crate) fn camera_sees_player(&self, i: usize) -> bool {
        let Some(cam) = self.def.cameras.get(i) else {
            return false;
        };
        let s = &self.state;
        // The cameras watch the rooms, not the roof or the street.
        if self.infiltrator_outside() {
            return false;
        }
        let ridx = Self::room_index_at(&self.def, s.player.x);
        if self.def.rooms.get(ridx).map(|r| &r.id) != Some(&cam.room) || cam.stale {
            return false;
        }
        // A looped camera is showing the operator yesterday's corridor, and an
        // unpowered one is showing nothing at all. Neither can flag anybody, so
        // the hack the hacker paid for is worth exactly what it promised.
        if s.camera_looped.get(i).is_some_and(|left| *left > 0.0)
            || s.dead_nodes.contains(&camera_node_id(i))
        {
            return false;
        }
        if self.signal_blinds_camera(i) {
            return false;
        }
        let cx = s.player.x + self.def.player.w / 2.0;
        let sweep = sin(s.t * c::CAM_SWEEP_W + cam.phase) * c::CAM_SWEEP_A;
        let lo = cam.range.0 + sweep.min(0.0);
        let hi = cam.range.1 + sweep.max(0.0);
        cx >= lo
            && cx <= hi
            && !s.player.hidden
            && !(s.player.crouching && s.player.vx.abs() < c::CROUCH_CAM_VX)
    }
}
