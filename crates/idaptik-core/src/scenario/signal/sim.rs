//! The deterministic signal-network runtime.
//!
//! [`SignalSim::apply`] takes one [`SignalCommand`] (lay, cut or invert a wire,
//! re-program a relay) and returns the events it produced; [`SignalSim::step`]
//! advances one fixed tick against the host's [`Readout`] of the world and
//! returns the events that tick produced.
//!
//! # Tick semantics
//!
//! Every output is read from the state at the *start* of the tick, every input
//! then changes state, so a signal crosses exactly one wire per tick. That gives
//! a uniform one-tick latency and makes feedback loops through relays
//! well-defined rather than order-dependent. Edge inputs fire once on the rising
//! edge of their wire, however long it is held; level inputs hold while it is
//! high ([`super::SignalKind::mode`]).

use super::{
    Gate, InPort, LinkDef, OutPort, PortMode, SeatProfile, SignalKind, SignalNetworkDef,
    SignalValidationError,
};
use crate::scenario::ids::SignalId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The snapshot format this build reads and writes.
pub const SIGNAL_SNAPSHOT_FORMAT: &str = "idaptik-signal-snapshot/1";

/// Who is doing the work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Seat {
    Infiltrator,
    Hacker,
}

/// Which devices each seat can touch right now. The host computes this —
/// the infiltrator's room and what they are jacked into, the hacker's pivot
/// and reach — and hands it in with every command.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Access {
    pub infiltrator: BTreeSet<SignalId>,
    pub hacker: BTreeSet<SignalId>,
}

impl Access {
    /// Both seats can touch every device (tests, and a sandbox view).
    pub fn everyone(def: &SignalNetworkDef) -> Self {
        let all: BTreeSet<SignalId> = def.devices.iter().map(|d| d.id.clone()).collect();
        Self {
            infiltrator: all.clone(),
            hacker: all,
        }
    }

    /// Whether `seat` can touch `device`.
    pub fn can(&self, seat: Seat, device: &SignalId) -> bool {
        match seat {
            Seat::Infiltrator => self.infiltrator.contains(device),
            Seat::Hacker => self.hacker.contains(device),
        }
    }
}

/// The host's view of the world for one tick: which host-owned outputs
/// ([`SignalKind::host_outputs`]) are high. Anything else listed is ignored.
///
/// It also says which devices **cannot operate at all** this tick — no supply,
/// damaged, isolated, or still warming up (ADR-0013). An inert device emits
/// nothing and ignores its inputs, so no wire and no command can make it see,
/// open or speak. The signal network stays the only owner of *what* a device
/// does; power only decides whether it can.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Readout {
    pub high: BTreeSet<(SignalId, OutPort)>,
    /// Devices that cannot operate this tick.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub unpowered: BTreeSet<SignalId>,
}

impl Readout {
    /// Assert one host-owned output for this tick.
    pub fn set(&mut self, device: &str, port: OutPort) -> &mut Self {
        self.high.insert((device.into(), port));
        self
    }

    /// Mark a device inert: it has no supply, is damaged or is still warming up.
    pub fn unpower(&mut self, device: &str) -> &mut Self {
        self.unpowered.insert(device.into());
        self
    }
}

/// The two ends of a wire, as a command names them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireEnds {
    pub from: SignalId,
    pub from_port: OutPort,
    pub to: SignalId,
    pub to_port: InPort,
}

impl WireEnds {
    /// Build from raw ids.
    pub fn new(from: &str, from_port: OutPort, to: &str, to_port: InPort) -> Self {
        Self {
            from: from.into(),
            from_port,
            to: to.into(),
            to_port,
        }
    }

    fn link(&self) -> LinkDef {
        LinkDef {
            from: self.from.clone(),
            from_port: self.from_port,
            to: self.to.clone(),
            to_port: self.to_port,
            invert: false,
            builtin: false,
        }
    }
}

/// A typed command the signal runtime consumes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum SignalCommand {
    /// Lay a wire. Configured at its receiving end, so `by` must be able to
    /// touch `to`.
    Wire { by: Seat, ends: WireEnds },
    /// Cut a player-laid wire.
    Cut { by: Seat, ends: WireEnds },
    /// Invert a player-laid wire.
    Invert { by: Seat, ends: WireEnds },
    /// Re-program a relay's gate.
    Reprogram {
        by: Seat,
        relay: SignalId,
        gate: Gate,
    },
}

impl SignalCommand {
    /// The seat issuing the command.
    pub fn seat(&self) -> Seat {
        match self {
            SignalCommand::Wire { by, .. }
            | SignalCommand::Cut { by, .. }
            | SignalCommand::Invert { by, .. }
            | SignalCommand::Reprogram { by, .. } => *by,
        }
    }
}

/// What a piece of work cost the seat that did it. The host charges it:
/// bandwidth against the hacker's link, busy ticks against the infiltrator's
/// hands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkCost {
    pub bandwidth: u32,
    pub busy_ticks: u32,
}

/// Why a command was refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "why", rename_all = "snake_case")]
pub enum SignalDenyReason {
    /// The seat cannot touch this device from where it is.
    NoAccess { device: SignalId },
    /// The wire is not valid for these devices.
    BadWire {
        problems: Vec<SignalValidationError>,
    },
    /// The installer laid this wire; it cannot be cut or inverted.
    Hardwired,
    /// No wire joins these ports.
    NoSuchWire,
    /// A wire already joins these ports.
    AlreadyWired,
    /// The wire budget is spent.
    BudgetFull { budget: u32 },
    /// Not a relay.
    NotARelay { device: SignalId },
    /// This seat lacks the skill (re-programming, say).
    Unskilled,
    /// The receiving device is hacker-only and this seat cannot wire into it.
    HackerOnly { device: SignalId },
    /// A command named a device index the network does not have.
    NoSuchDevice { index: u16 },
    /// The hacker cannot afford the bandwidth (host-checked).
    NoBandwidth,
    /// The infiltrator's hands are still busy at a panel (host-checked).
    Busy,
}

/// A visible state a device switched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Switch {
    /// A door opened or closed.
    Open,
    /// A door locked or unlocked.
    Locked,
    /// A camera, sensor or light went on or off.
    Active,
}

/// A typed event the signal runtime emits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum SignalEvent {
    Wired {
        link: LinkDef,
        by: Seat,
        cost: WorkCost,
    },
    Cut {
        link: LinkDef,
        by: Seat,
        cost: WorkCost,
    },
    Inverted {
        link: LinkDef,
        by: Seat,
        cost: WorkCost,
    },
    Reprogrammed {
        relay: SignalId,
        gate: Gate,
        by: Seat,
        cost: WorkCost,
    },
    Denied {
        by: Seat,
        reason: SignalDenyReason,
    },
    Switched {
        device: SignalId,
        what: Switch,
        on: bool,
    },
    /// A wire told the vacuum to run. The host routes it.
    VacuumRouted {
        vacuum: SignalId,
    },
}

/// One device's runtime state. Fields a kind does not use stay at their
/// defaults.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceState {
    /// Doors: open.
    pub open: bool,
    /// Doors: locked.
    pub locked: bool,
    /// Cameras, sensors, lights: on.
    pub active: bool,
    /// Relays: current gate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<Gate>,
    /// Relays: sampled inputs.
    pub a: bool,
    pub b: bool,
    /// Relays: latch memory.
    pub latched: bool,
    /// Doors: a running `pulse` — ticks left, and the state to restore after.
    pub pulse_ticks: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_pulse: Option<bool>,
    /// Last tick's input levels, one bit per [`InPort`].
    pub prev_in: u16,
}

/// The complete runtime state — pure serde, so a snapshot restores exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalState {
    pub tick: u64,
    pub devices: Vec<DeviceState>,
    /// Every wire, builtin first then player-laid, in the order laid.
    pub links: Vec<LinkDef>,
}

impl SignalState {
    /// The state a fresh run starts in.
    pub fn initial(def: &SignalNetworkDef) -> Self {
        let devices = def
            .devices
            .iter()
            .map(|d| DeviceState {
                locked: d.start_locked,
                active: matches!(
                    d.kind,
                    SignalKind::Camera | SignalKind::Sensor | SignalKind::Light
                ),
                gate: d.gate,
                ..DeviceState::default()
            })
            .collect();
        Self {
            tick: 0,
            devices,
            links: def.links.clone(),
        }
    }
}

/// A restorable snapshot: the definition plus the runtime state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalSnapshot {
    pub format: String,
    pub definition: SignalNetworkDef,
    pub state: SignalState,
}

/// The deterministic signal-network runtime.
#[derive(Debug, Clone)]
pub struct SignalSim {
    def: SignalNetworkDef,
    state: SignalState,
}

fn out_bit(p: OutPort) -> u8 {
    1 << (p as u8)
}

impl SignalSim {
    /// Start a run. Validates the definition first.
    pub fn new(def: SignalNetworkDef) -> Result<Self, Vec<SignalValidationError>> {
        def.ok()?;
        let state = SignalState::initial(&def);
        Ok(Self { def, state })
    }

    /// Restore a run from a snapshot, gating on its format tag.
    pub fn restore(snapshot: SignalSnapshot) -> Result<Self, Vec<SignalValidationError>> {
        if snapshot.format != SIGNAL_SNAPSHOT_FORMAT {
            return Err(vec![SignalValidationError::UnsupportedSnapshotFormat {
                found: snapshot.format,
            }]);
        }
        snapshot.definition.ok()?;
        if snapshot.state.devices.len() != snapshot.definition.devices.len() {
            return Err(vec![SignalValidationError::UnsupportedSnapshotFormat {
                found: "state does not match its definition".into(),
            }]);
        }
        Ok(Self {
            def: snapshot.definition,
            state: snapshot.state,
        })
    }

    /// A snapshot of the run.
    pub fn snapshot(&self) -> SignalSnapshot {
        SignalSnapshot {
            format: SIGNAL_SNAPSHOT_FORMAT.to_owned(),
            definition: self.def.clone(),
            state: self.state.clone(),
        }
    }

    /// The definition being played.
    pub fn definition(&self) -> &SignalNetworkDef {
        &self.def
    }

    /// The current state.
    pub fn state(&self) -> &SignalState {
        &self.state
    }

    /// One device's state, by id.
    pub fn device(&self, id: &str) -> Option<&DeviceState> {
        let i = self.def.index_of(&id.into())?;
        self.state.devices.get(i)
    }

    /// How many player-laid wires exist.
    pub fn wires_laid(&self) -> u32 {
        self.state.wires_laid()
    }

    /// Apply one command under `access`, returning what it produced.
    pub fn apply(&mut self, cmd: &SignalCommand, access: &Access) -> Vec<SignalEvent> {
        self.state.apply(&self.def, cmd, access)
    }

    /// Advance one tick against the host's readout.
    pub fn step(&mut self, readout: &Readout) -> Vec<SignalEvent> {
        self.state.step(&self.def, readout)
    }
}

impl SignalState {
    /// How many player-laid wires exist.
    pub fn wires_laid(&self) -> u32 {
        self.links.iter().filter(|l| !l.builtin).count() as u32
    }

    /// Apply one command to this state under `def` — the entry point a host
    /// that embeds the state (Ghost Lobby's `RuntimeState`) uses.
    pub fn apply(
        &mut self,
        def: &SignalNetworkDef,
        cmd: &SignalCommand,
        access: &Access,
    ) -> Vec<SignalEvent> {
        Run { def, state: self }.apply(cmd, access)
    }

    /// Advance this state one tick under `def`.
    pub fn step(&mut self, def: &SignalNetworkDef, readout: &Readout) -> Vec<SignalEvent> {
        Run { def, state: self }.step(readout)
    }

    /// The output bits of every device right now, for frontends that draw
    /// live wires: bit `p as u8` of entry `i` is device `i`'s port `p`.
    pub fn outputs(&self, def: &SignalNetworkDef, readout: &Readout) -> Vec<u8> {
        outputs_of(def, self, readout)
    }
}

/// One definition and the state it drives, borrowed for one command or tick.
struct Run<'a> {
    def: &'a SignalNetworkDef,
    state: &'a mut SignalState,
}

impl Run<'_> {
    fn wires_laid(&self) -> u32 {
        self.state.wires_laid()
    }

    // --- commands ------------------------------------------------------------

    fn apply(&mut self, cmd: &SignalCommand, access: &Access) -> Vec<SignalEvent> {
        let by = cmd.seat();
        let result = match cmd {
            SignalCommand::Wire { ends, .. } => self.wire(by, ends, access),
            SignalCommand::Cut { ends, .. } => self.cut(by, ends, access, false),
            SignalCommand::Invert { ends, .. } => self.cut(by, ends, access, true),
            SignalCommand::Reprogram { relay, gate, .. } => {
                self.reprogram(by, relay, *gate, access)
            }
        };
        result.unwrap_or_else(|reason| vec![SignalEvent::Denied { by, reason }])
    }

    fn profile(&self, by: Seat) -> &SeatProfile {
        match by {
            Seat::Infiltrator => &self.def.tuning.infiltrator,
            Seat::Hacker => &self.def.tuning.hacker,
        }
    }

    fn wire_cost(&self, by: Seat) -> WorkCost {
        let p = self.profile(by);
        WorkCost {
            bandwidth: p.wire_bandwidth,
            busy_ticks: p.wire_busy_ticks,
        }
    }

    /// The access and hacker-only checks every wire operation shares.
    fn may_wire_into(
        &self,
        by: Seat,
        ends: &WireEnds,
        access: &Access,
    ) -> Result<(), SignalDenyReason> {
        let problems = self.def.check_link(&ends.link());
        if !problems.is_empty() {
            return Err(SignalDenyReason::BadWire { problems });
        }
        if !access.can(by, &ends.to) {
            return Err(SignalDenyReason::NoAccess {
                device: ends.to.clone(),
            });
        }
        let hacker_only = self.def.device(&ends.to).is_some_and(|d| d.hacker_only);
        if hacker_only && !self.profile(by).may_wire_hacker_only {
            return Err(SignalDenyReason::HackerOnly {
                device: ends.to.clone(),
            });
        }
        Ok(())
    }

    fn wire(
        &mut self,
        by: Seat,
        ends: &WireEnds,
        access: &Access,
    ) -> Result<Vec<SignalEvent>, SignalDenyReason> {
        self.may_wire_into(by, ends, access)?;
        let link = ends.link();
        if self.state.links.iter().any(|l| l.same_ends(&link)) {
            return Err(SignalDenyReason::AlreadyWired);
        }
        if self.wires_laid() >= self.def.budget {
            return Err(SignalDenyReason::BudgetFull {
                budget: self.def.budget,
            });
        }
        self.state.links.push(link.clone());
        Ok(vec![SignalEvent::Wired {
            link,
            by,
            cost: self.wire_cost(by),
        }])
    }

    fn cut(
        &mut self,
        by: Seat,
        ends: &WireEnds,
        access: &Access,
        invert_only: bool,
    ) -> Result<Vec<SignalEvent>, SignalDenyReason> {
        self.may_wire_into(by, ends, access)?;
        let probe = ends.link();
        let i = self
            .state
            .links
            .iter()
            .position(|l| l.same_ends(&probe))
            .ok_or(SignalDenyReason::NoSuchWire)?;
        if self.state.links[i].builtin {
            return Err(SignalDenyReason::Hardwired);
        }
        let cost = self.wire_cost(by);
        if invert_only {
            self.state.links[i].invert = !self.state.links[i].invert;
            let link = self.state.links[i].clone();
            Ok(vec![SignalEvent::Inverted { link, by, cost }])
        } else {
            let link = self.state.links.remove(i);
            Ok(vec![SignalEvent::Cut { link, by, cost }])
        }
    }

    fn reprogram(
        &mut self,
        by: Seat,
        relay: &SignalId,
        gate: Gate,
        access: &Access,
    ) -> Result<Vec<SignalEvent>, SignalDenyReason> {
        let i = self
            .def
            .index_of(relay)
            .filter(|&i| self.def.devices[i].kind == SignalKind::Relay)
            .ok_or_else(|| SignalDenyReason::NotARelay {
                device: relay.clone(),
            })?;
        if !access.can(by, relay) {
            return Err(SignalDenyReason::NoAccess {
                device: relay.clone(),
            });
        }
        if !self.profile(by).may_reprogram {
            return Err(SignalDenyReason::Unskilled);
        }
        let d = &mut self.state.devices[i];
        d.gate = Some(gate);
        d.latched = false;
        Ok(vec![SignalEvent::Reprogrammed {
            relay: relay.clone(),
            gate,
            by,
            cost: self.wire_cost(by),
        }])
    }

    // --- the tick ------------------------------------------------------------

    fn outputs(&self, readout: &Readout) -> Vec<u8> {
        outputs_of(self.def, self.state, readout)
    }

    /// Advance one tick against the host's readout.
    fn step(&mut self, readout: &Readout) -> Vec<SignalEvent> {
        self.step_body(readout)
    }
}

/// The output bits of every device, read from current state and the host. A
/// door reads open if the network holds it open *or* the host says it is
/// (a routed hold, Billy badging through).
fn outputs_of(def: &SignalNetworkDef, state: &SignalState, readout: &Readout) -> Vec<u8> {
    def.devices
        .iter()
        .zip(&state.devices)
        .map(|(def, s)| {
            // A device without power emits nothing: a blind camera does not trip,
            // a dead door is not open, a held relay has no coil current.
            if readout.unpowered.contains(&def.id) {
                return 0u8;
            }
            let host = |p: OutPort| readout.high.contains(&(def.id.clone(), p));
            let mut bits = 0u8;
            let mut set = |p: OutPort, on: bool| {
                if on {
                    bits |= out_bit(p);
                }
            };
            match def.kind {
                SignalKind::Door => {
                    let open = s.open || host(OutPort::Open);
                    set(OutPort::Open, open);
                    set(OutPort::Closed, !open);
                }
                SignalKind::Light => set(OutPort::Active, s.active),
                // A disabled camera or sensor sees nothing.
                SignalKind::Camera | SignalKind::Sensor => {
                    set(OutPort::Tripped, s.active && host(OutPort::Tripped))
                }
                SignalKind::Guard => set(OutPort::Tripped, host(OutPort::Tripped)),
                SignalKind::Vacuum => set(OutPort::Active, host(OutPort::Active)),
                SignalKind::Terminal => set(OutPort::Compromised, host(OutPort::Compromised)),
                SignalKind::Relay => set(
                    OutPort::Q,
                    s.gate.unwrap_or(Gate::And).eval(s.a, s.b, s.latched),
                ),
            }
            bits
        })
        .collect()
}

impl Run<'_> {
    fn step_body(&mut self, readout: &Readout) -> Vec<SignalEvent> {
        let door_pulse_ticks = self.def.tuning.door_pulse_ticks;
        let mut events = Vec::new();

        for s in &mut self.state.devices {
            s.pulse_ticks = s.pulse_ticks.saturating_sub(1);
        }

        // 1. Every output, from state at the start of the tick.
        let outs = self.outputs(readout);

        // 2. Every input level, OR-ed over the wires into it.
        let mut levels = vec![0u16; self.def.devices.len()];
        for l in &self.state.links {
            let (Some(f), Some(t)) = (self.def.index_of(&l.from), self.def.index_of(&l.to)) else {
                continue;
            };
            let high = (outs[f] & out_bit(l.from_port) != 0) != l.invert;
            if high {
                levels[t] |= l.to_port.bit();
            }
        }

        // 3. Inputs change state.
        for (i, def) in self.def.devices.iter().enumerate() {
            if readout.unpowered.contains(&def.id) {
                // Inert: nothing is asked of it and nothing happens — but it
                // must be handed back in the state it was interrupted in.
                //
                // * A pulse that was running is *not* cancelled: the mechanism
                //   simply cannot move until it has current again. Dropping the
                //   timer and the saved state together would leave a door open
                //   for ever, because nothing is left to close it.
                // * The edge history is synchronised with the inputs, so a wire
                //   held high through the outage does not fire on restore. That
                //   history is `prev_in` *and* the relay's cached edge flags:
                //   `Q` is computed from `s.a`/`s.b`, so a relay that went dark
                //   on the same tick it received an edge would otherwise emit a
                //   pulse on power-up that nobody asked for. No edge was
                //   delivered while it was inert, so none is remembered.
                let s = &mut self.state.devices[i];
                s.a = false;
                s.b = false;
                s.prev_in = levels[i];
                continue;
            }
            let s = &mut self.state.devices[i];
            let before = (s.open, s.locked, s.active);
            let level = levels[i];
            let rising = level & !s.prev_in;
            let fired = |p: InPort| match def.kind.mode(p) {
                PortMode::Edge => rising & p.bit() != 0,
                PortMode::Level => level & p.bit() != 0,
            };

            match def.kind {
                SignalKind::Door => {
                    // A pulse that ran out restores what it interrupted.
                    if s.pulse_ticks == 0
                        && let Some(was) = s.pre_pulse.take()
                    {
                        s.open = was;
                    }
                    if fired(InPort::Toggle) {
                        s.open = !s.open;
                    }
                    if fired(InPort::Open) {
                        s.open = true;
                    }
                    if fired(InPort::Close) {
                        s.open = false;
                    }
                    if fired(InPort::Lock) {
                        s.locked = true;
                    }
                    if fired(InPort::Unlock) {
                        s.locked = false;
                        s.open = true;
                    }
                    if fired(InPort::Pulse) {
                        s.pre_pulse.get_or_insert(s.open);
                        s.open = true;
                        s.pulse_ticks = door_pulse_ticks;
                    }
                    if s.locked {
                        s.open = false;
                    }
                }
                SignalKind::Camera | SignalKind::Sensor => {
                    if fired(InPort::Enable) {
                        s.active = true;
                    }
                    if fired(InPort::Disable) {
                        s.active = false;
                    }
                }
                SignalKind::Light => {
                    if fired(InPort::Toggle) {
                        s.active = !s.active;
                    }
                    if fired(InPort::Enable) {
                        s.active = true;
                    }
                    if fired(InPort::Disable) {
                        s.active = false;
                    }
                }
                SignalKind::Vacuum => {
                    if fired(InPort::Pulse) {
                        events.push(SignalEvent::VacuumRouted {
                            vacuum: def.id.clone(),
                        });
                    }
                }
                SignalKind::Relay => {
                    let a = fired(InPort::A);
                    if s.gate == Some(Gate::Latch) && a && !s.a {
                        s.latched = !s.latched;
                    }
                    s.a = a;
                    s.b = fired(InPort::B);
                }
                SignalKind::Guard | SignalKind::Terminal => {}
            }
            s.prev_in = level;

            let after = (s.open, s.locked, s.active);
            for (what, was, now) in [
                (Switch::Open, before.0, after.0),
                (Switch::Locked, before.1, after.1),
                (Switch::Active, before.2, after.2),
            ] {
                if was != now {
                    events.push(SignalEvent::Switched {
                        device: def.id.clone(),
                        what,
                        on: now,
                    });
                }
            }
        }

        self.state.tick += 1;
        events
    }
}

/// A device addressed by its position in [`SignalNetworkDef::devices`] — how
/// a wire [`crate::scenario::Command`] names one, the way `NetNodeIndex`
/// names a graph node, so the command stays `Copy` and fixed-size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SignalIndex(pub u16);

/// The wire form of a [`SignalCommand`]: the same verbs with devices by index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum SignalOp {
    Wire {
        by: Seat,
        from: SignalIndex,
        from_port: OutPort,
        to: SignalIndex,
        to_port: InPort,
    },
    Cut {
        by: Seat,
        from: SignalIndex,
        from_port: OutPort,
        to: SignalIndex,
        to_port: InPort,
    },
    Invert {
        by: Seat,
        from: SignalIndex,
        from_port: OutPort,
        to: SignalIndex,
        to_port: InPort,
    },
    Reprogram {
        by: Seat,
        relay: SignalIndex,
        gate: Gate,
    },
}

impl SignalOp {
    /// The seat issuing the op.
    pub fn seat(&self) -> Seat {
        match self {
            SignalOp::Wire { by, .. }
            | SignalOp::Cut { by, .. }
            | SignalOp::Invert { by, .. }
            | SignalOp::Reprogram { by, .. } => *by,
        }
    }

    /// Resolve device indices against `def`, or name the first bad index.
    pub fn resolve(&self, def: &SignalNetworkDef) -> Result<SignalCommand, SignalDenyReason> {
        let id = |i: SignalIndex| {
            def.devices
                .get(usize::from(i.0))
                .map(|d| d.id.clone())
                .ok_or(SignalDenyReason::NoSuchDevice { index: i.0 })
        };
        let ends = |from, from_port, to, to_port| -> Result<WireEnds, SignalDenyReason> {
            Ok(WireEnds {
                from: id(from)?,
                from_port,
                to: id(to)?,
                to_port,
            })
        };
        Ok(match *self {
            SignalOp::Wire {
                by,
                from,
                from_port,
                to,
                to_port,
            } => SignalCommand::Wire {
                by,
                ends: ends(from, from_port, to, to_port)?,
            },
            SignalOp::Cut {
                by,
                from,
                from_port,
                to,
                to_port,
            } => SignalCommand::Cut {
                by,
                ends: ends(from, from_port, to, to_port)?,
            },
            SignalOp::Invert {
                by,
                from,
                from_port,
                to,
                to_port,
            } => SignalCommand::Invert {
                by,
                ends: ends(from, from_port, to, to_port)?,
            },
            SignalOp::Reprogram { by, relay, gate } => SignalCommand::Reprogram {
                by,
                relay: id(relay)?,
                gate,
            },
        })
    }
}
