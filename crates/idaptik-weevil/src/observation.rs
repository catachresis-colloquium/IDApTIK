//! Host → kernel observations.
//!
//! The game owns perception; the kernel owns decision state. An [`Observation`]
//! is the game's honest report of what one guard can perceive this tick: its own
//! pose, its health and ammunition, and the contacts it has grounds to believe
//! are there. The adapter converts it into the ABI's fixed `ee_snapshot`.
//!
//! # What crosses the ABI, and what does not
//!
//! The ABI's contact carries an id, a bearing, a distance and a threat. It does
//! **not** carry a source, an age or a confidence. Those are part of the
//! adapter's deliberate schema ([`ContactSource`], [`Contact::confidence`],
//! [`Contact::age_ticks`]) and are recorded in the replay trace, but they never
//! cross the C boundary — the kernel has nowhere to put them. A host that needs
//! to reason about "is this contact fresh?" does so here, in Rust, beside the
//! observation it is annotating.
//!
//! # Bearing encoding — a recorded ABI limitation
//!
//! `ee_contact.bearing_cos`/`bearing_sin` are `u16`, and the kernel reads them
//! with a zero-extending cast. A signed Q16.16 component does not fit, so the
//! kernel cannot represent a bearing that points left or down: the returned
//! `move_x`/`move_y` are therefore only meaningful as a *magnitude hint*. The
//! adapter keeps the full signed [`Fx2`] bearing on the Rust side (so the host
//! and the replay can use it) and truncates to the low 16 bits only at the ABI
//! boundary. Consequence, stated plainly: **the host must apply its own
//! navigation**, which the architecture assigns to the game anyway; the kernel's
//! move vector is advisory and is validated but not trusted for steering.

use crate::abi::{EeContact, EeSnapshot, MAX_CONTACTS};
use crate::error::{Result, WeevilError};
use crate::fx::Fx2;

/// Where a contact's evidence came from. Deliberate, closed vocabulary: a host
/// that reports a contact must say what kind of thing it believes it saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContactSource {
    /// The infiltrator, perceived directly (sight or sound).
    Player,
    /// A tracked world object (a note, a drive, a doorway, …).
    Object(u32),
    /// A decoy or spoofed sensor reading: real in the world, not a person.
    Decoy,
    /// A sound event with no visible source.
    Sound,
}

impl ContactSource {
    /// A short stable label for logs and the developer view.
    pub fn label(self) -> &'static str {
        match self {
            Self::Player => "player",
            Self::Object(_) => "object",
            Self::Decoy => "decoy",
            Self::Sound => "sound",
        }
    }
}

/// One perceived contact.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact {
    /// Host-assigned contact id. Must be stable for the life of the entity so
    /// the kernel's remembered target can be matched back by the host.
    pub id: u32,
    /// What kind of thing this is.
    pub source: ContactSource,
    /// Direction from the guard to the contact, Q16.16, signed.
    pub bearing: Fx2,
    /// Range in ABI fx units (`65536 == 1.0`). The kernel compares this against
    /// its own attack/flee thresholds, so the host must scale world distance
    /// consistently (see [`crate::profile::HostTuning::fx_per_world_unit`]).
    pub distance: i32,
    /// Perceived threat in fx units.
    pub threat: i32,
    /// The host's confidence in this contact, `0.0..=1.0`.
    pub confidence: f64,
    /// Ticks since the contact was first perceived (0 = this tick). Recorded for
    /// the replay; the ABI carries no age, so the kernel's own memory age is the
    /// only age it knows.
    pub age_ticks: u32,
}

impl Contact {
    /// A minimal contact with a unit bearing and full confidence.
    pub fn new(id: u32, source: ContactSource, bearing: Fx2, distance: i32) -> Self {
        Self {
            id,
            source,
            bearing,
            distance,
            threat: 0,
            confidence: 1.0,
            age_ticks: 0,
        }
    }
}

/// One guard's observation for one tick.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    /// The guard this observation belongs to. Must match a host agent slot.
    pub agent_id: u32,
    /// Monotonic host tick. Must be exactly one more than the agent's last.
    pub tick: u64,
    /// The guard's own position, Q16.16.
    pub self_pos: Fx2,
    /// The guard's own velocity, Q16.16.
    pub self_vel: Fx2,
    /// Health in fx units (`65536 == full`).
    pub health: i32,
    /// Ammunition in fx units (`0 == empty`).
    pub ammo: i32,
    /// Capability mask the host declares for this agent this tick.
    pub capability_mask: u32,
    /// Contacts the guard perceives, in the host's chosen order. The kernel
    /// selects the nearest by distance, breaking ties by lowest index.
    pub contacts: Vec<Contact>,
    /// Host-side action cooldown in ticks (recorded, not interpreted by v0).
    pub cooldown: u32,
}

impl Observation {
    /// An empty observation for a guard that perceives nothing.
    pub fn lonely(agent_id: u32, tick: u64, self_pos: Fx2) -> Self {
        Self {
            agent_id,
            tick,
            self_pos,
            self_vel: Fx2::ZERO,
            health: crate::abi::FX_ONE,
            ammo: 3 * crate::abi::FX_ONE,
            capability_mask: 0,
            contacts: Vec::new(),
            cooldown: 0,
        }
    }

    /// Convert into the ABI snapshot, validating everything the kernel cannot.
    pub fn to_snapshot(&self) -> Result<EeSnapshot> {
        if self.contacts.len() > MAX_CONTACTS {
            return Err(WeevilError::TooManyContacts {
                count: self.contacts.len(),
                max: MAX_CONTACTS,
            });
        }
        let agent_id = checked_u16("agent", self.agent_id)?;
        let mut snap = EeSnapshot {
            tick: self.tick,
            capability_mask: self.capability_mask,
            agent_id,
            contact_count: self.contacts.len() as u16,
            self_x: self.self_pos.x,
            self_y: self.self_pos.y,
            self_z: 0,
            self_vx: self.self_vel.x,
            self_vy: self.self_vel.y,
            health: self.health,
            ammo: self.ammo,
            flags: 0,
            contact0: EeContact::default(),
            contact1: EeContact::default(),
            contact2: EeContact::default(),
            contact3: EeContact::default(),
            contact4: EeContact::default(),
            contact5: EeContact::default(),
            contact6: EeContact::default(),
            contact7: EeContact::default(),
            cooldown: self.cooldown,
            reserved0: 0,
        };
        for (i, c) in self.contacts.iter().enumerate() {
            let slot = match i {
                0 => &mut snap.contact0,
                1 => &mut snap.contact1,
                2 => &mut snap.contact2,
                3 => &mut snap.contact3,
                4 => &mut snap.contact4,
                5 => &mut snap.contact5,
                6 => &mut snap.contact6,
                _ => &mut snap.contact7,
            };
            *slot = EeContact {
                id: checked_u16("contact", c.id)?,
                // The ABI has no field for source/age/confidence; they live in
                // the Rust schema and the replay. `flags` is unused by the v0
                // kernel, so it stays zero rather than smuggling a meaning the
                // kernel would ignore.
                flags: 0,
                bearing_cos: c.bearing.x as u16,
                bearing_sin: c.bearing.y as u16,
                distance: c.distance,
                threat: c.threat,
            };
        }
        Ok(snap)
    }
}

/// Convert a `u32` id to the ABI's `u16`, refusing to silently alias two
/// distinct ids onto the same value.
fn checked_u16(kind: &'static str, v: u32) -> Result<u16> {
    if v > u16::MAX as u32 {
        return Err(WeevilError::IdOutOfRange { kind, id: v });
    }
    Ok(v as u16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::{FX_ONE, to_fx};

    #[test]
    fn snapshot_carries_contacts_in_order() {
        let obs = Observation {
            agent_id: 3,
            tick: 41,
            self_pos: Fx2 {
                x: to_fx(10.0),
                y: 0,
            },
            self_vel: Fx2::ZERO,
            health: FX_ONE,
            ammo: FX_ONE,
            capability_mask: 3,
            contacts: vec![
                Contact::new(
                    100,
                    ContactSource::Player,
                    Fx2 {
                        x: to_fx(1.0),
                        y: 0,
                    },
                    to_fx(3.0),
                ),
                Contact::new(
                    101,
                    ContactSource::Object(7),
                    Fx2 {
                        x: 0,
                        y: to_fx(1.0),
                    },
                    to_fx(6.0),
                ),
            ],
            cooldown: 0,
        };
        let snap = obs.to_snapshot().expect("ok");
        assert_eq!(snap.agent_id, 3);
        assert_eq!(snap.contact_count, 2);
        assert_eq!(snap.contact0.id, 100);
        assert_eq!(snap.contact1.id, 101);
        assert_eq!(snap.contact0.distance, to_fx(3.0));
    }

    #[test]
    fn more_than_eight_contacts_is_refused() {
        let contacts = (0..9)
            .map(|i| Contact::new(i, ContactSource::Sound, Fx2::ZERO, to_fx(1.0)))
            .collect();
        let obs = Observation {
            agent_id: 0,
            tick: 1,
            self_pos: Fx2::ZERO,
            self_vel: Fx2::ZERO,
            health: FX_ONE,
            ammo: FX_ONE,
            capability_mask: 0,
            contacts,
            cooldown: 0,
        };
        assert_eq!(
            obs.to_snapshot().unwrap_err(),
            WeevilError::TooManyContacts { count: 9, max: 8 }
        );
    }
}
