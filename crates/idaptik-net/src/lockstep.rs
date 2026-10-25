//! Delay-based lockstep for live seats (issue #7, interactive-client slice).
//!
//! The batch machine in [`crate::session`] streams a whole script, then runs.
//! A live seat cannot: input arrives in real time, so each seat samples its
//! input at local step `T` and schedules it for execution tick `T +
//! input_delay`, giving every command `input_delay` ticks of wire time to
//! reach the peer before either side needs it. Pacing hides latency; nobody
//! rolls back.
//!
//! Two protocol facts carry the whole design:
//!
//! - **Commands are sparse, so completeness needs a watermark.** Silence at
//!   tick `T` is indistinguishable from a command still in flight. Each seat
//!   therefore follows its sends with a `net:commit { through }` — over the
//!   ordered transport, receiving the commit proves every command `at <=
//!   through` already arrived. A seat executes tick `T` only when both its own
//!   and its peer's watermarks cover `T`.
//! - **Uncommitted commands did not happen.** A seat that dies between a
//!   command and its commit leaves an orphan in the survivor's buffer that the
//!   rejoining seat (a fresh process) knows nothing about; if the survivor
//!   kept it, the rejoiner would resample that tick and the two sims would
//!   silently diverge. [`LockstepCore::on_peer_lost`] prunes everything at or
//!   above the peer's watermark, and the resync payload carries only committed
//!   history — both sides then agree exactly on which inputs exist.
//!
//! This module is sans-IO on purpose: it consumes facts (`on_peer_*`) and
//! emits [`Outgoing`] values, and never touches a socket. The unit tests below
//! prove determinism by running two cores over an in-memory wire — including
//! a mid-run death and [`Resync`] — and asserting both event logs equal the
//! reference `headless::simulate` run byte-for-byte. The async driver in
//! [`crate::live`] then carries the same machine over the real relay.

use crate::envelope::Role;
use crate::error::NetError;
use crate::seat::seat_schedule;
use idaptik_core::scenario::command::{Button, Buttons, Command, fold};
use idaptik_core::scenario::event::Event;
use idaptik_core::scenario::{GhostLobbySim, RuntimeSnapshot};
use idaptik_tui::headless;
use idaptik_tui::script::{ScriptFile, button_from};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A source of local input: the commands this seat schedules for execution
/// tick `at`, in send order. Implementations are role-aware — they must only
/// emit commands their seat may send (the relay enforces it; the feed decides
/// it).
pub trait InputFeed {
    fn commands_for(&mut self, at: u64) -> Vec<Command>;
}

/// The scripted feed: one seat's share of a headless script, served by
/// execution tick. Stateless per tick (a `BTreeMap` lookup), which is what
/// makes a rejoined seat resample identical commands for a pruned tick.
pub struct ScriptFeed {
    by_tick: BTreeMap<u64, Vec<Command>>,
}

impl ScriptFeed {
    pub fn new(script: &ScriptFile, role: Role) -> Self {
        let mut by_tick: BTreeMap<u64, Vec<Command>> = BTreeMap::new();
        for sc in seat_schedule(script, role) {
            by_tick.entry(sc.at).or_default().push(sc.cmd);
        }
        Self { by_tick }
    }
}

impl InputFeed for ScriptFeed {
    fn commands_for(&mut self, at: u64) -> Vec<Command> {
        self.by_tick.get(&at).cloned().unwrap_or_default()
    }
}

/// Something the driver must put on the wire, in order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Outgoing {
    /// Push on the `"command"` relay with this schedule tick.
    Command { at: u64, cmd: Command },
    /// Push a `net:commit` watermark on the `"event"` relay.
    Commit { through: u64 },
}

/// One command pending execution, as carried inside [`Resync`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingCommand {
    pub at: u64,
    pub cmd: Command,
}

/// The snapshot hand-off a surviving seat ships to a rejoining peer. Carries
/// everything a fresh process needs to continue the run exactly: the
/// restorable sim state, the full event log so far (so the rejoiner's final
/// artifact covers the whole run), the client-side fold state, and both
/// seats' committed-but-unexecuted command history with their watermarks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resync {
    /// Always [`crate::envelope::RESYNC_TAG`].
    pub event: String,
    /// The surviving (sending) seat.
    pub role: Role,
    /// Executed lockstep steps. Distinct from `snapshot.tick`: a paused tick
    /// consumes a step without advancing the sim's counter, so the step index
    /// must travel alongside the snapshot.
    pub step: u64,
    /// The run's step budget (the script's `max_ticks`), so the payload is
    /// self-contained — `RunConfig` inside the snapshot does not carry it.
    pub max_steps: u64,
    /// The fold-state held-button set at `step`, as canonical script tokens.
    /// Client-side lockstep state, not sim state — the snapshot cannot carry
    /// it.
    pub held: Vec<String>,
    pub snapshot: RuntimeSnapshot,
    pub event_log: Vec<Event>,
    /// The survivor's sample head: it has committed through `survivor_next -
    /// 1` and will next sample `survivor_next`.
    pub survivor_next: u64,
    /// The survivor's committed-but-unexecuted commands (`step <= at`), which
    /// the rejoiner missed while away (the relay has no history).
    pub survivor_pending: Vec<PendingCommand>,
    /// The rejoiner's former self: committed through `your_next - 1`. The
    /// rejoiner resumes sampling at `your_next`.
    pub your_next: u64,
    /// The rejoiner's former committed-but-unexecuted commands, which the
    /// fresh process never knew it sent.
    pub your_pending: Vec<PendingCommand>,
}

/// The delay-lockstep state machine for one live seat.
pub struct LockstepCore {
    role: Role,
    input_delay: u64,
    max_steps: u64,
    sim: GhostLobbySim,
    log: Vec<Event>,
    /// Client-side fold state, carried across steps (SetButton diffs
    /// accumulate here exactly as in the batch runner).
    held: Buttons,
    /// Executed steps == `sim.tick` calls. Not `sim.current_tick()`: paused
    /// ticks consume a step without advancing the sim.
    step: u64,
    /// The next execution tick to sample from the feed. Everything below is
    /// sampled, sent and committed.
    next_sample: u64,
    /// The peer has committed through `peer_next - 1`.
    peer_next: u64,
    own: BTreeMap<u64, Vec<Command>>,
    peer: BTreeMap<u64, Vec<Command>>,
    /// The sim reported an end before `max_steps` ran out.
    ended: bool,
}

impl LockstepCore {
    /// A fresh seat at tick 0. Drains the startup events into the log, exactly
    /// as the reference runner does before its first tick.
    pub fn new(role: Role, input_delay: u64, script: &ScriptFile) -> Result<Self, String> {
        let mut sim = headless::build(script)?;
        let log = sim.drain_events();
        Ok(Self {
            role,
            input_delay,
            max_steps: script.max_ticks,
            sim,
            log,
            held: Buttons::default(),
            step: 0,
            next_sample: 0,
            peer_next: 0,
            own: BTreeMap::new(),
            peer: BTreeMap::new(),
            ended: false,
        })
    }

    pub fn role(&self) -> Role {
        self.role
    }

    pub fn step(&self) -> u64 {
        self.step
    }

    /// The peer's committed watermark: the first tick the peer has not
    /// committed through. Read-only observability for the adversarial test
    /// adapter (ADR-0013), which must target the exact post-loss frontier
    /// when proving obsolete frames are rejected. Production drivers derive
    /// the same value implicitly through [`Self::blocked_on_peer`].
    pub fn peer_watermark(&self) -> u64 {
        self.peer_next
    }

    /// The run's step budget, exposed read-only for bounded test accounting.
    pub fn max_steps(&self) -> u64 {
        self.max_steps
    }

    pub fn sim(&self) -> &GhostLobbySim {
        &self.sim
    }

    pub fn log(&self) -> &[Event] {
        &self.log
    }

    /// The run is over: the sim ended, or the step budget ran out.
    pub fn finished(&self) -> bool {
        self.ended || self.step >= self.max_steps
    }

    /// Blocked waiting for the peer's watermark (the loss-detection predicate:
    /// this plus silence is what grace periods measure).
    pub fn blocked_on_peer(&self) -> bool {
        !self.finished() && self.step >= self.peer_next && self.step < self.next_sample
    }

    /// Sample the feed up to the delay horizon, recording and returning what
    /// must go on the wire: every newly sampled tick's commands, then one
    /// cumulative commit. Send order is the merge order, so the wire preserves
    /// it.
    pub fn pump_outgoing(&mut self, feed: &mut dyn InputFeed) -> Vec<Outgoing> {
        let mut out = Vec::new();
        if self.finished() || self.max_steps == 0 {
            return out;
        }
        let horizon = (self.step + self.input_delay).min(self.max_steps - 1);
        let mut sampled = None;
        while self.next_sample <= horizon {
            let at = self.next_sample;
            let cmds = feed.commands_for(at);
            for cmd in &cmds {
                out.push(Outgoing::Command { at, cmd: *cmd });
            }
            if !cmds.is_empty() {
                self.own.insert(at, cmds);
            }
            sampled = Some(at);
            self.next_sample = at + 1;
        }
        if let Some(through) = sampled {
            out.push(Outgoing::Commit { through });
        }
        out
    }

    /// A relayed peer command. Arriving at a tick the peer already committed
    /// (or this side already executed) cannot be applied consistently, so it
    /// is a protocol error, not a merge.
    pub fn on_peer_command(&mut self, at: u64, cmd: Command) -> Result<(), NetError> {
        if at < self.peer_next {
            return Err(NetError::Protocol(format!(
                "peer command for tick {at} arrived after its commit (watermark {})",
                self.peer_next
            )));
        }
        if at >= self.max_steps {
            return Err(NetError::Protocol(format!(
                "peer command scheduled past the run: {at} >= {}",
                self.max_steps
            )));
        }
        self.peer.entry(at).or_default().push(cmd);
        Ok(())
    }

    /// A peer `net:commit` watermark.
    pub fn on_peer_commit(&mut self, through: u64) -> Result<(), NetError> {
        if through >= self.max_steps {
            return Err(NetError::Protocol(format!(
                "peer committed past the run: {through} >= {}",
                self.max_steps
            )));
        }
        self.peer_next = self.peer_next.max(through + 1);
        Ok(())
    }

    /// The peer is gone. Prune its uncommitted commands: without a commit they
    /// never happened, and the rejoining process will resample those ticks.
    pub fn on_peer_lost(&mut self) {
        let watermark = self.peer_next;
        self.peer.retain(|at, _| *at < watermark);
    }

    /// Execute every ready step, calling `on_step` after each tick with the
    /// sim and that tick's fresh events (the render hook).
    pub fn advance_with(&mut self, mut on_step: impl FnMut(&GhostLobbySim, &[Event])) {
        while !self.finished() && self.step < self.peer_next && self.step < self.next_sample {
            if self.sim.is_ended() {
                // Mirror the reference runner: never tick an ended sim.
                self.ended = true;
                break;
            }
            let mut cmds: Vec<Command> = Vec::new();
            let (first, second) = match self.role {
                Role::Infiltrator => (&self.own, &self.peer),
                Role::Hacker => (&self.peer, &self.own),
            };
            // Merged order: the infiltrator's commands, then the hacker's,
            // each in send order — the same rule the batch pipeline proved
            // equal to `expand()`.
            if let Some(c) = first.get(&self.step) {
                cmds.extend_from_slice(c);
            }
            if let Some(c) = second.get(&self.step) {
                cmds.extend_from_slice(c);
            }
            let input = fold(&cmds, &mut self.held);
            let events = self.sim.tick(&input);
            self.log.extend(events.iter().cloned());
            self.step += 1;
            self.own.remove(&(self.step - 1));
            self.peer.remove(&(self.step - 1));
            on_step(&self.sim, &events);
        }
    }

    /// Build the snapshot hand-off for a rejoining peer. Call after
    /// [`Self::on_peer_lost`] so the peer's side of the payload holds only
    /// committed history.
    pub fn make_resync(&self) -> Resync {
        let flatten = |map: &BTreeMap<u64, Vec<Command>>| {
            map.iter()
                .flat_map(|(at, cmds)| cmds.iter().map(|cmd| PendingCommand { at: *at, cmd: *cmd }))
                .collect()
        };
        Resync {
            event: crate::envelope::RESYNC_TAG.to_owned(),
            role: self.role,
            step: self.step,
            max_steps: self.max_steps,
            held: held_tokens(self.held),
            snapshot: self.sim.snapshot(),
            event_log: self.log.clone(),
            survivor_next: self.next_sample,
            survivor_pending: flatten(&self.own),
            your_next: self.peer_next,
            your_pending: flatten(&self.peer),
        }
    }

    /// Rebuild a seat mid-run from a survivor's [`Resync`]: restore the sim
    /// (RNG and pause state included), adopt the full log, the fold state,
    /// both pending sets and both watermarks, then continue sampling at
    /// `your_next` as if the death never happened.
    pub fn adopt_resync(role: Role, input_delay: u64, resync: Resync) -> Result<Self, NetError> {
        if resync.role == role {
            return Err(NetError::Protocol(format!(
                "resync from {:?} adopted by {:?}: same seat on both sides",
                resync.role, role
            )));
        }
        let def = resync.snapshot.definition.clone();
        let sim = GhostLobbySim::restore(def, resync.snapshot)
            .map_err(|e| NetError::Session(format!("restore snapshot: {e:?}")))?;
        let mut held = Buttons::default();
        for name in &resync.held {
            let b = button_from(name)
                .ok_or_else(|| NetError::Protocol(format!("unknown held button {name:?}")))?;
            held.set(b, true);
        }
        let gather = |pending: &[PendingCommand]| {
            let mut map: BTreeMap<u64, Vec<Command>> = BTreeMap::new();
            for p in pending {
                map.entry(p.at).or_default().push(p.cmd);
            }
            map
        };
        Ok(Self {
            role,
            input_delay,
            max_steps: resync.max_steps,
            sim,
            log: resync.event_log,
            held,
            step: resync.step,
            next_sample: resync.your_next,
            peer_next: resync.survivor_next,
            own: gather(&resync.your_pending),
            peer: gather(&resync.survivor_pending),
            ended: false,
        })
    }

    /// Consume the finished core into its artifacts:
    /// `(event_log, debrief, final_snapshot)`.
    pub fn into_artifacts(self) -> (Vec<Event>, Option<idaptik_core::Debrief>, RuntimeSnapshot) {
        (self.log, self.sim.debrief().cloned(), self.sim.snapshot())
    }
}

/// Encode a held set as canonical script tokens (the inverse of
/// [`button_from`]).
fn held_tokens(held: Buttons) -> Vec<String> {
    const ALL: [(Button, &str); 5] = [
        (Button::Left, "left"),
        (Button::Right, "right"),
        (Button::Crouch, "crouch"),
        (Button::Sprint, "sprint"),
        (Button::Interact, "interact"),
    ];
    ALL.iter()
        .filter(|(b, _)| held.has(*b))
        .map(|(_, name)| (*name).to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::{decode_command, encode_command};
    use idaptik_core::Debrief;
    use std::path::Path;

    fn live_fixture() -> ScriptFile {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/session_relay/live_script.json");
        headless::load(&path).expect("live fixture script loads")
    }

    fn reference() -> (Vec<Event>, Option<Debrief>) {
        let script = live_fixture();
        let (sim, log) = headless::simulate(&script).expect("reference run");
        (log, sim.debrief().cloned())
    }

    /// Deliver one seat's outgoing into the other core, round-tripping every
    /// command through the wire envelope (encode → relay strips `"seq"` →
    /// decode) so the codec is under test too.
    fn deliver(out: &[Outgoing], seq: &mut u64, to: &mut LockstepCore) {
        for o in out {
            match o {
                Outgoing::Command { at, cmd } => {
                    *seq += 1;
                    let mut payload = encode_command(cmd, *seq, *at).unwrap();
                    payload.as_object_mut().unwrap().remove("seq");
                    let (at, cmd) = decode_command(&payload).unwrap();
                    to.on_peer_command(at, cmd).unwrap();
                }
                Outgoing::Commit { through } => to.on_peer_commit(*through).unwrap(),
            }
        }
    }

    fn assert_matches_reference(core: LockstepCore) {
        let (ref_log, ref_debrief) = reference();
        let (log, debrief, _snapshot) = core.into_artifacts();
        assert_eq!(
            serde_json::to_value(&log).unwrap(),
            serde_json::to_value(&ref_log).unwrap(),
            "event log diverged from the reference run"
        );
        assert_eq!(
            serde_json::to_value(&debrief).unwrap(),
            serde_json::to_value(&ref_debrief).unwrap(),
            "debrief diverged from the reference run"
        );
    }

    /// Two peers run with no relay, database, GSA, or other central service:
    /// only the two sans-IO cores and an in-memory direct wire. Different
    /// input delays interoperate because every command carries its own `at`
    /// and every commit its own watermark, reproducing the reference run
    /// exactly (pause window included). This is a simulation/core test, not a
    /// production direct-network transport test.
    #[test]
    fn peer_lockstep_runs_without_central_services() {
        let script = live_fixture();
        let mut a = LockstepCore::new(Role::Infiltrator, 2, &script).unwrap();
        let mut b = LockstepCore::new(Role::Hacker, 5, &script).unwrap();
        let mut fa = ScriptFeed::new(&script, Role::Infiltrator);
        let mut fb = ScriptFeed::new(&script, Role::Hacker);
        let (mut sa, mut sb) = (0u64, 0u64);

        let mut guard = 0;
        while !(a.finished() && b.finished()) {
            guard += 1;
            assert!(guard < 10_000, "lockstep deadlocked");
            let oa = a.pump_outgoing(&mut fa);
            let ob = b.pump_outgoing(&mut fb);
            deliver(&oa, &mut sa, &mut b);
            deliver(&ob, &mut sb, &mut a);
            a.advance_with(|_, _| {});
            b.advance_with(|_, _| {});
        }

        let paused = serde_json::to_string(a.log()).unwrap();
        assert!(
            paused.contains("Paused") && paused.contains("Resumed"),
            "fixture must exercise the pause window"
        );
        assert_matches_reference(a);
        assert_matches_reference(b);
    }

    /// Laggy wire: at most one queued message is delivered per turn, so seats
    /// regularly stall on the watermark. Stalls must change pacing only, never
    /// the result.
    #[test]
    fn staggered_delivery_still_matches_the_reference() {
        use std::collections::VecDeque;
        let script = live_fixture();
        let mut a = LockstepCore::new(Role::Infiltrator, 3, &script).unwrap();
        let mut b = LockstepCore::new(Role::Hacker, 3, &script).unwrap();
        let mut fa = ScriptFeed::new(&script, Role::Infiltrator);
        let mut fb = ScriptFeed::new(&script, Role::Hacker);
        let mut qa: VecDeque<Outgoing> = VecDeque::new(); // a → b, in flight
        let mut qb: VecDeque<Outgoing> = VecDeque::new(); // b → a, in flight
        let (mut sa, mut sb) = (0u64, 0u64);

        let mut guard = 0;
        while !(a.finished() && b.finished()) {
            guard += 1;
            assert!(guard < 100_000, "lockstep deadlocked");
            qa.extend(a.pump_outgoing(&mut fa));
            qb.extend(b.pump_outgoing(&mut fb));
            if let Some(o) = qa.pop_front() {
                deliver(&[o], &mut sa, &mut b);
            }
            if let Some(o) = qb.pop_front() {
                deliver(&[o], &mut sb, &mut a);
            }
            a.advance_with(|_, _| {});
            b.advance_with(|_, _| {});
        }

        assert_matches_reference(a);
        assert_matches_reference(b);
    }

    /// Drive both seats losslessly until the infiltrator has executed
    /// `die_at` steps, then vanish the hacker — optionally leaving an orphan
    /// (a command whose commit never arrived) in the survivor's buffer.
    fn run_until_death(die_at: u64, orphan: bool) -> (LockstepCore, ScriptFeed) {
        let script = live_fixture();
        let mut a = LockstepCore::new(Role::Infiltrator, 3, &script).unwrap();
        let mut b = LockstepCore::new(Role::Hacker, 3, &script).unwrap();
        let mut fa = ScriptFeed::new(&script, Role::Infiltrator);
        let mut fb = ScriptFeed::new(&script, Role::Hacker);
        let (mut sa, mut sb) = (0u64, 0u64);

        let mut guard = 0;
        while a.step() < die_at {
            guard += 1;
            assert!(guard < 10_000, "death point never reached");
            let oa = a.pump_outgoing(&mut fa);
            let ob = b.pump_outgoing(&mut fb);
            deliver(&oa, &mut sa, &mut b);
            deliver(&ob, &mut sb, &mut a);
            a.advance_with(|_, _| {});
            b.advance_with(|_, _| {});
        }

        if orphan {
            // The hacker's dying breath: a command reaches the survivor, its
            // commit does not. `on_peer_lost` must make it un-happen — kept,
            // it would fire an Uplink the rejoiner never resamples, and the
            // logs would silently diverge from the reference.
            let at = a.peer_next;
            assert!(at < a.max_steps, "orphan tick must be inside the run");
            a.on_peer_command(
                at,
                Command::Uplink {
                    kind: idaptik_core::scenario::ActionKind::Camera,
                },
            )
            .unwrap();
        }

        a.on_peer_lost();
        (a, fa)
    }

    /// A mid-run death and snapshot resync — inside the pause window, so the
    /// restored sim must come back *paused* and resume off the scheduled
    /// command — reconstructs the reference run on both seats, including the
    /// rejoiner's full event log.
    #[test]
    fn death_and_resync_reconstruct_the_reference_run() {
        for (die_at, orphan) in [(8, false), (20, true)] {
            let script = live_fixture();
            let (mut a, mut fa) = run_until_death(die_at, orphan);
            let resync = a.make_resync();
            if die_at == 8 {
                assert!(
                    resync.snapshot.paused,
                    "step 8 is inside the pause window; the snapshot must restore paused"
                );
            }
            let mut b = LockstepCore::adopt_resync(Role::Hacker, 3, resync).unwrap();
            let mut fb = ScriptFeed::new(&script, Role::Hacker);
            let (mut sa, mut sb) = (0u64, 0u64);

            let mut guard = 0;
            while !(a.finished() && b.finished()) {
                guard += 1;
                assert!(guard < 10_000, "post-resync lockstep deadlocked");
                let oa = a.pump_outgoing(&mut fa);
                let ob = b.pump_outgoing(&mut fb);
                deliver(&oa, &mut sa, &mut b);
                deliver(&ob, &mut sb, &mut a);
                a.advance_with(|_, _| {});
                b.advance_with(|_, _| {});
            }

            assert_matches_reference(a);
            assert_matches_reference(b);
        }
    }

    /// The challenge-set floor through the live pipeline (ADR-0014): mole
    /// orders are authored on the infiltrator seat, round-trip the wire
    /// envelope, and both seats still reproduce the single-process reference
    /// run event-for-event — ordering, access and replay for `Command::Mole`
    /// verified under the existing networking model, not assumed.
    #[test]
    fn pump_house_mole_orders_lockstep_with_the_reference() {
        use idaptik_tui::script::ScriptLine;
        let script = ScriptFile {
            seed: 99,
            difficulty: "standard".to_owned(),
            scenario: Some("pump-house".to_owned()),
            reduced_motion: false,
            supervised: false,
            max_ticks: 900,
            commands: vec![
                ScriptLine {
                    at: 30,
                    hold: None,
                    press: Some(vec!["mole:move:520:under".to_owned()]),
                    test: None,
                },
                ScriptLine {
                    at: 240,
                    hold: None,
                    press: Some(vec!["mole:synth:0".to_owned()]),
                    test: None,
                },
                ScriptLine {
                    at: 420,
                    hold: None,
                    press: Some(vec!["mole:sabotage".to_owned()]),
                    test: None,
                },
            ],
            expected_event_log: None,
        };

        // The mole orders must belong to the infiltrator seat, or the feed
        // split below would strand them and the test would pass vacuously.
        assert_eq!(
            crate::envelope::seat_for(&Command::Mole {
                op: idaptik_core::scenario::command::MoleOp::Dig,
            }),
            crate::envelope::Seat::Infiltrator,
        );

        let (ref_sim, ref_log) = headless::simulate(&script).expect("reference run");
        let ref_debrief = ref_sim.debrief().cloned();
        let ref_log_json = serde_json::to_string(&ref_log).unwrap();
        // The check must be able to fail: the run really hosted the
        // companion and really folded its events into the canonical log.
        assert!(
            ref_log_json.contains("\"Mole\""),
            "the reference run carries no mole events: {ref_log_json}"
        );

        let mut a = LockstepCore::new(Role::Infiltrator, 2, &script).unwrap();
        let mut b = LockstepCore::new(Role::Hacker, 5, &script).unwrap();
        let mut fa = ScriptFeed::new(&script, Role::Infiltrator);
        let mut fb = ScriptFeed::new(&script, Role::Hacker);
        let (mut sa, mut sb) = (0u64, 0u64);

        let mut guard = 0;
        while !(a.finished() && b.finished()) {
            guard += 1;
            assert!(guard < 10_000, "lockstep deadlocked");
            let oa = a.pump_outgoing(&mut fa);
            let ob = b.pump_outgoing(&mut fb);
            deliver(&oa, &mut sa, &mut b);
            deliver(&ob, &mut sb, &mut a);
            a.advance_with(|_, _| {});
            b.advance_with(|_, _| {});
        }

        for core in [a, b] {
            let (log, debrief, _snapshot) = core.into_artifacts();
            assert_eq!(
                serde_json::to_value(&log).unwrap(),
                serde_json::to_value(&ref_log).unwrap(),
                "event log diverged from the reference run"
            );
            assert_eq!(
                serde_json::to_value(&debrief).unwrap(),
                serde_json::to_value(&ref_debrief).unwrap(),
                "debrief diverged from the reference run"
            );
        }
    }
}
