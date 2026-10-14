//! The signal network: devices wired output-to-input, re-wired in play.
//!
//! The grounded network ([`crate::netsim`]) answers *who can reach what*; this
//! module answers *what happens when they get there*. Every device exposes
//! output ports (signals it emits — a door is open, a camera has tripped, a
//! terminal has been compromised) and input ports (things it can be made to do —
//! open, lock, disable). Wires carry booleans from an output to an input,
//! optionally inverted. The installer's wires are hardwired; the players lay
//! their own, up to a budget, and can re-program free-standing relays (logic
//! gates).
//!
//! **Both seats can wire.** The difference between them is *access* and
//! *skill*, not role:
//!
//! * the infiltrator touches a device by standing at it;
//! * the hacker touches it by reaching it over the network (pivoting, or
//!   through a foothold the infiltrator jacked in);
//! * a [`SeatProfile`] prices each seat's work (the hacker spends bandwidth, the
//!   infiltrator spends time at the panel) and gates the skilled work —
//!   re-programming relays, wiring into hacker-only devices.
//!
//! Who can touch what at a given tick is the host's call (vantage, room,
//! pivot depth), handed in as an [`sim::Access`]. This module owns the wiring
//! rules and the deterministic per-tick resolution; [`sim::SignalSim`] is the
//! runtime. See ADR-0012.

pub mod ghost_lobby_wiring;
pub mod sim;

use crate::scenario::constants::ROUTE_DURATION;
use crate::scenario::ids::SignalId;
use crate::scenario::mathf::TICK_DT;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The signal-network payload format this build reads and writes.
pub const SIGNAL_FORMAT: &str = "idaptik-signal/1";

/// What a device is: the physical-function devices the grounded network
/// already knows ([`crate::device::DeviceKind`]), plus the relay. The port
/// table ([`SignalKind::outputs`] / [`SignalKind::inputs`]) is fixed per kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    /// A smart door between two rooms.
    Door,
    /// A security camera.
    Camera,
    /// A room's lights.
    Light,
    /// A motion or presence sensor.
    Sensor,
    /// The robot vacuum.
    Vacuum,
    /// A guard (Billy). Outputs only: what he is doing is the host's to decide.
    Guard,
    /// A terminal or controller that can be compromised.
    Terminal,
    /// A free-standing logic gate.
    Relay,
}

/// A signal a device emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutPort {
    Active,
    Tripped,
    Open,
    Closed,
    Compromised,
    Q,
}

/// Something a device can be made to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InPort {
    Toggle,
    Open,
    Close,
    Lock,
    Unlock,
    Enable,
    Disable,
    Pulse,
    A,
    B,
}

impl InPort {
    /// This port's bit in a device's edge memory.
    pub fn bit(self) -> u16 {
        1 << (self as u16)
    }
}

/// How an input reacts to its wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortMode {
    /// Fires once when the wire goes high (open, toggle, lock, …).
    Edge,
    /// Holds while the wire is high (relay inputs).
    Level,
}

impl SignalKind {
    /// The output ports this kind exposes, in display order.
    pub fn outputs(self) -> &'static [OutPort] {
        use OutPort::*;
        match self {
            SignalKind::Door => &[Open, Closed],
            SignalKind::Light | SignalKind::Vacuum => &[Active],
            SignalKind::Camera | SignalKind::Sensor | SignalKind::Guard => &[Tripped],
            SignalKind::Terminal => &[Compromised],
            SignalKind::Relay => &[Q],
        }
    }

    /// The input ports this kind exposes, in display order.
    pub fn inputs(self) -> &'static [InPort] {
        use InPort::*;
        match self {
            SignalKind::Door => &[Open, Close, Toggle, Lock, Unlock, Pulse],
            SignalKind::Camera | SignalKind::Sensor => &[Enable, Disable],
            SignalKind::Light => &[Enable, Disable, Toggle],
            SignalKind::Vacuum => &[Pulse],
            SignalKind::Guard | SignalKind::Terminal => &[],
            SignalKind::Relay => &[A, B],
        }
    }

    /// Output ports the *host* drives from world state (a camera sees someone,
    /// Billy is pursuing, a terminal has been compromised, the vacuum is
    /// running, a door is open on its own route or badge). Everything else the
    /// network owns.
    pub fn host_outputs(self) -> &'static [OutPort] {
        use OutPort::*;
        match self {
            SignalKind::Door => &[Open],
            SignalKind::Camera | SignalKind::Sensor | SignalKind::Guard => &[Tripped],
            SignalKind::Terminal => &[Compromised],
            SignalKind::Vacuum => &[Active],
            _ => &[],
        }
    }

    /// Edge- or level-triggered, for an input this kind exposes.
    pub fn mode(self, _port: InPort) -> PortMode {
        match self {
            SignalKind::Relay => PortMode::Level,
            _ => PortMode::Edge,
        }
    }
}

/// A relay's logic function. `Latch` flips on each rising edge of `a`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gate {
    And,
    Or,
    Xor,
    Nand,
    Nor,
    Latch,
}

impl Gate {
    /// The combinational output; `latched` is only read by [`Gate::Latch`].
    pub fn eval(self, a: bool, b: bool, latched: bool) -> bool {
        match self {
            Gate::And => a && b,
            Gate::Or => a || b,
            Gate::Xor => a != b,
            Gate::Nand => !(a && b),
            Gate::Nor => !(a || b),
            Gate::Latch => latched,
        }
    }
}

/// One device in the network.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalDeviceDef {
    pub id: SignalId,
    pub kind: SignalKind,
    pub label: String,
    /// The grounded-network node this device is, if any — how the host
    /// decides whether the hacker can reach it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    /// The room it stands in, if any — how the host decides whether the
    /// infiltrator can touch it in person.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    /// Relays only: the gate it starts programmed as.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<Gate>,
    /// Doors only: starts locked (`open` is refused until `unlock`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub start_locked: bool,
    /// Wiring into this device needs [`SeatProfile::may_wire_hacker_only`].
    #[serde(default, skip_serializing_if = "is_false")]
    pub hacker_only: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// A wire from one device's output to another's input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkDef {
    pub from: SignalId,
    pub from_port: OutPort,
    pub to: SignalId,
    pub to_port: InPort,
    /// The wire carries the negation of its source.
    #[serde(default, skip_serializing_if = "is_false")]
    pub invert: bool,
    /// Laid by the installer: cannot be cut or inverted, and does not count
    /// against the budget.
    #[serde(default, skip_serializing_if = "is_false")]
    pub builtin: bool,
}

impl LinkDef {
    /// Whether this wire joins the same two ports as `other`.
    pub fn same_ends(&self, other: &LinkDef) -> bool {
        self.from == other.from
            && self.from_port == other.from_port
            && self.to == other.to
            && self.to_port == other.to_port
    }
}

/// What a seat's wiring costs, and what it is allowed to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatProfile {
    /// Bandwidth spent per wire laid, cut or inverted.
    pub wire_bandwidth: u32,
    /// Ticks spent working the panel per wire laid, cut or inverted.
    pub wire_busy_ticks: u32,
    /// May change a relay's gate.
    pub may_reprogram: bool,
    /// May wire into a hacker-only device.
    pub may_wire_hacker_only: bool,
}

/// Per-network tuning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalTuning {
    pub infiltrator: SeatProfile,
    pub hacker: SeatProfile,
    /// How long a door's `pulse` holds it open.
    pub door_pulse_ticks: u32,
}

impl Default for SignalTuning {
    /// The hacker is the better engineer — instant, skilled, paid in
    /// bandwidth. The infiltrator can do the plain work by hand, slowly. A
    /// pulsed door holds for the same `ROUTE_DURATION` as a routed hold.
    fn default() -> Self {
        Self {
            infiltrator: SeatProfile {
                wire_bandwidth: 0,
                wire_busy_ticks: 90,
                may_reprogram: false,
                may_wire_hacker_only: false,
            },
            hacker: SeatProfile {
                wire_bandwidth: 10,
                wire_busy_ticks: 0,
                may_reprogram: true,
                may_wire_hacker_only: true,
            },
            door_pulse_ticks: libm::round(ROUTE_DURATION / TICK_DT) as u32,
        }
    }
}

/// A complete signal network: devices, the installer's wiring, the players'
/// wire budget and the seat tuning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalNetworkDef {
    pub format: String,
    pub network_id: String,
    pub name: String,
    pub devices: Vec<SignalDeviceDef>,
    pub links: Vec<LinkDef>,
    /// How many player-laid (non-builtin) wires may exist at once.
    pub budget: u32,
    #[serde(default)]
    pub tuning: SignalTuning,
}

/// Why a signal network definition is unusable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum SignalValidationError {
    UnsupportedFormat { found: String },
    UnsupportedSnapshotFormat { found: String },
    DuplicateDevice { id: SignalId },
    UnknownDevice { id: SignalId },
    NoSuchOutput { device: SignalId, port: OutPort },
    NoSuchInput { device: SignalId, port: InPort },
    SelfLink { device: SignalId },
    DuplicateLink { from: SignalId, to: SignalId },
    RelayWithoutGate { device: SignalId },
    GateOnNonRelay { device: SignalId },
    LockOnNonDoor { device: SignalId },
    OverBudget { laid: u32, budget: u32 },
}

impl SignalNetworkDef {
    /// The device with this id.
    pub fn device(&self, id: &SignalId) -> Option<&SignalDeviceDef> {
        self.devices.iter().find(|d| &d.id == id)
    }

    /// The index of the device with this id.
    pub fn index_of(&self, id: &SignalId) -> Option<usize> {
        self.devices.iter().position(|d| &d.id == id)
    }

    /// Every problem with the definition; empty means it is playable.
    pub fn validate(&self) -> Vec<SignalValidationError> {
        use SignalValidationError as E;
        let mut errs = Vec::new();
        if self.format != SIGNAL_FORMAT {
            errs.push(E::UnsupportedFormat {
                found: self.format.clone(),
            });
        }
        let mut seen = BTreeSet::new();
        for d in &self.devices {
            if !seen.insert(&d.id) {
                errs.push(E::DuplicateDevice { id: d.id.clone() });
            }
            let id = || d.id.clone();
            match (d.kind, &d.gate) {
                (SignalKind::Relay, None) => errs.push(E::RelayWithoutGate { device: id() }),
                (k, Some(_)) if k != SignalKind::Relay => {
                    errs.push(E::GateOnNonRelay { device: id() })
                }
                _ => {}
            }
            if d.start_locked && d.kind != SignalKind::Door {
                errs.push(E::LockOnNonDoor { device: id() });
            }
        }
        for (i, l) in self.links.iter().enumerate() {
            errs.extend(self.check_link(l));
            if self.links[..i].iter().any(|o| o.same_ends(l)) {
                errs.push(E::DuplicateLink {
                    from: l.from.clone(),
                    to: l.to.clone(),
                });
            }
        }
        let laid = self.links.iter().filter(|l| !l.builtin).count() as u32;
        if laid > self.budget {
            errs.push(E::OverBudget {
                laid,
                budget: self.budget,
            });
        }
        errs
    }

    /// Problems with one wire against this network's devices.
    pub fn check_link(&self, l: &LinkDef) -> Vec<SignalValidationError> {
        use SignalValidationError as E;
        let mut errs = Vec::new();
        match self.device(&l.from) {
            None => errs.push(E::UnknownDevice { id: l.from.clone() }),
            Some(d) if !d.kind.outputs().contains(&l.from_port) => errs.push(E::NoSuchOutput {
                device: l.from.clone(),
                port: l.from_port,
            }),
            _ => {}
        }
        match self.device(&l.to) {
            None => errs.push(E::UnknownDevice { id: l.to.clone() }),
            Some(d) if !d.kind.inputs().contains(&l.to_port) => errs.push(E::NoSuchInput {
                device: l.to.clone(),
                port: l.to_port,
            }),
            _ => {}
        }
        if l.from == l.to {
            errs.push(E::SelfLink {
                device: l.from.clone(),
            });
        }
        errs
    }

    /// `Ok` iff [`Self::validate`] found nothing.
    pub fn ok(&self) -> Result<(), Vec<SignalValidationError>> {
        let errs = self.validate();
        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }
}

/// Why a signal payload could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalPackError {
    /// The JSON did not parse as a signal network.
    Parse(String),
    /// It parsed but is not playable.
    Invalid(Vec<SignalValidationError>),
}

/// The UMS seam: parse a signal-network payload, gate on its format tag and
/// validate it before the runtime trusts it — the same contract as
/// [`crate::scenario::building::load_building`].
pub fn load_signal_network(json: &str) -> Result<SignalNetworkDef, SignalPackError> {
    let def: SignalNetworkDef =
        serde_json::from_str(json).map_err(|e| SignalPackError::Parse(e.to_string()))?;
    def.ok().map_err(SignalPackError::Invalid)?;
    Ok(def)
}
