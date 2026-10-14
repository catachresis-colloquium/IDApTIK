//! Consoles: equipment with a screen, and what a screen shows.
//!
//! The hacker has total system knowledge and zero eyes (storyboard §12.1):
//! nobody on the network sees the building, only its systems. A console is a
//! piece of equipment — the deck in the van, a PC on an office desk — and what
//! its screen shows is a pure function of three things:
//!
//! * **reach** — the network session of whoever is using it (the hacker from
//!   wherever they have pivoted to; the infiltrator from the room they stand in);
//! * **capability** — what the equipment can display ([`ConsoleCap`]);
//! * **intel** — what the building's own systems have observed and logged
//!   ([`Intel`]): camera sightings and door passages, each with a time.
//!
//! People are never shown directly. A live camera that can see someone gives
//! an exact fix; otherwise the last fix is shown as an [`Estimate`] whose
//! radius grows with how long ago it was taken, and only if the device that
//! took it is still within reach.
//!
//! [`ConsoleCap`]: crate::scenario::definition::ConsoleCap

use crate::scenario::ids::{DoorId, RoomId};
use crate::scenario::signal::sim::Seat;
use serde::{Deserialize, Serialize};

/// What produced a fix.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FixSource {
    /// Camera `index` (definition order) had them in frame.
    Camera { index: usize },
    /// A door controller logged them passing.
    Door { door: DoorId },
}

/// One observation of someone's position, logged by the building.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fix {
    pub x: f64,
    pub t: f64,
    pub room: RoomId,
    pub source: FixSource,
}

/// What the building's systems have observed. Recorded only on a floor that
/// has consoles, so the canonical floor's state is unchanged.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Intel {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub infiltrator: Option<Fix>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub billy: Option<Fix>,
    /// The room the infiltrator was last in, to notice door passages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_room: Option<RoomId>,
}

impl Intel {
    /// Nothing observed yet.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// A room as the screen draws it: a labelled box, no picture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenRoom {
    pub id: RoomId,
    pub name: String,
    pub x: f64,
    pub w: f64,
}

/// The kind of device a screen lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenKind {
    Door,
    Camera,
    Light,
    Vacuum,
}

/// One device the screen can see, with its state as the system reports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenDevice {
    /// Grounded-network node id.
    pub node: String,
    pub kind: ScreenKind,
    pub label: String,
    /// Canvas x on the floor.
    pub x: f64,
    pub room: Option<RoomId>,
    /// What the system reports, e.g. `ROUTING 1.2s`, `LOOPED 18s`.
    pub status: String,
    /// Position in the grounded graph, for `Command::NetHack`.
    pub net_index: u32,
    /// Whether this seat can act on it from here (the hacker's deck can;
    /// a screen used by the infiltrator is look-only for now).
    pub actionable: bool,
}

/// Where someone probably is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Estimate {
    pub x: f64,
    pub room: RoomId,
    /// Uncertainty, canvas units.
    pub radius: f64,
    /// Seconds since the fix.
    pub age: f64,
    /// A live camera has them in frame right now.
    pub live: bool,
}

/// Everything one console shows one seat, right now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenView {
    pub console: String,
    pub label: String,
    pub seat: Seat,
    pub rooms: Vec<ScreenRoom>,
    pub devices: Vec<ScreenDevice>,
    pub infiltrator: Option<Estimate>,
    pub billy: Option<Estimate>,
    /// Whether the signal network is drawn and wireable from here.
    pub wiring: bool,
    pub trace: f32,
    pub hops: u32,
    pub bandwidth: f64,
}
