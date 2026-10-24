//! The isolated test harness: the component that owns every production
//! object an adversarial intent can reach.
//!
//! Delivery order for each Evil Weevil intent:
//!
//! 1. the policy gate ([`crate::policy`]) confirms the adversary may propose
//!    and the harness may deliver, in this scenario;
//! 2. the in-process relay model ([`crate::relay_model`]) applies the
//!    documented relay rules — role table, `seq` de-duplication;
//! 3. anything it relays is fed through the **real** production path —
//!    [`idaptik_net::envelope::decode_command`] and
//!    [`idaptik_net::lockstep::LockstepCore`]'s validators, or a real
//!    [`idaptik_core::scenario::GhostLobbySim`] for target-claim scenarios.
//!
//! Nothing here sanitises an invalid frame, and nothing here bypasses a
//! production check. The harness's own authority ends at choosing whether an
//! *attempt* is authorised; the host keeps the accept/reject decision.
//!
//! All queues are in-memory and bounded by [`crate::limits`]; there is no
//! socket, no wall clock and no remote address anywhere in this crate.

use crate::agent::Intent;
use crate::limits::{HarnessMetrics, MAX_STEPS};
use crate::policy::{Permission, PolicyDecision, Principal, PrincipalKind, TestPolicy};
use crate::relay_model::{RelayModel, RelayVerdict};
use crate::telemetry::{TelemetryEvent, ViolationKind};
use idaptik_core::scenario::command::TickInput;
use idaptik_core::scenario::{GhostLobbySim, RuntimeSnapshot};
use idaptik_net::envelope::Role;
use idaptik_net::error::NetError;
use idaptik_net::lockstep::{LockstepCore, ScriptFeed};
use idaptik_tui::headless;
use idaptik_tui::script::ScriptFile;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// One observable fact recorded during a scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservedEffect {
    pub step: u64,
    pub what: String,
    pub detail: String,
    /// Whether the host ultimately accepted/applied the frame.
    pub accepted: bool,
}

/// A named invariant check, evaluated during the run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScenarioCheck {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

/// The path to the shared live-seat fixture, resolved relative to this crate
/// so the corpus uses the exact script the loopback gate and lockstep tests
/// use — no scenario-specific copy that could drift.
pub fn live_fixture_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/session_relay/live_script.json")
}

/// Load the shared live-seat fixture.
pub fn load_live_script() -> ScriptFile {
    headless::load(&live_fixture_path()).expect("shared live_script.json fixture loads")
}

/// Map a production validator error to the telemetry vocabulary. The
/// classification keys on the host's own error text; the detail string is
/// preserved verbatim in the trace.
fn violation_for(error: &NetError) -> ViolationKind {
    let text = error.to_string();
    if text.contains("after its commit") {
        ViolationKind::StaleAfterCommit
    } else if text.contains("past the run") {
        ViolationKind::PastRunSchedule
    } else if text.contains("same seat on both sides") {
        ViolationKind::SameSeatResync
    } else if text.contains("unknown variant") || text.contains("unknown command") {
        ViolationKind::UnknownCommandTag
    } else {
        // Missing `at`, non-object payloads, and every other shape failure.
        ViolationKind::MalformedPayload
    }
}

/// The outcome of feeding one raw frame to the production lockstep validator.
pub enum ValidatorOutcome {
    /// The frame decoded and the lockstep core accepted it into its buffer.
    Accepted,
    /// The frame was decoded but rejected by a lockstep rule.
    Rejected(NetError),
    /// The frame could not be decoded at all.
    Undecodable(NetError),
}

/// A pair of live seats plus their scripted feeds, the standard test rig.
pub struct SeatPair {
    pub infiltrator: LockstepCore,
    pub hacker: LockstepCore,
    pub inf_feed: ScriptFeed,
    pub hac_feed: ScriptFeed,
    pub script: ScriptFile,
}

impl SeatPair {
    /// Build the pair the way the lockstep unit tests do, with explicit
    /// input delays (mismatched delays are legal and must interop).
    pub fn new(inf_delay: u64, hac_delay: u64) -> Self {
        let script = load_live_script();
        let infiltrator =
            LockstepCore::new(Role::Infiltrator, inf_delay, &script).expect("core builds");
        let hacker = LockstepCore::new(Role::Hacker, hac_delay, &script).expect("core builds");
        let inf_feed = ScriptFeed::new(&script, Role::Infiltrator);
        let hac_feed = ScriptFeed::new(&script, Role::Hacker);
        Self {
            infiltrator,
            hacker,
            inf_feed,
            hac_feed,
            script,
        }
    }
}

/// The isolated harness. One instance per scenario run.
pub struct IsolatedHarness {
    policy: TestPolicy,
    metrics: HarnessMetrics,
    telemetry: Vec<TelemetryEvent>,
    effects: Vec<ObservedEffect>,
    checks: Vec<ScenarioCheck>,
    intents: Vec<Intent>,
    rng: idaptik_core::Mulberry32,
}

impl IsolatedHarness {
    pub fn new(scenario_id: &'static str, seed: u32) -> Self {
        Self {
            policy: TestPolicy::corpus_default(scenario_id),
            metrics: HarnessMetrics::default(),
            telemetry: Vec::new(),
            effects: Vec::new(),
            checks: Vec::new(),
            intents: Vec::new(),
            rng: idaptik_core::Mulberry32::new(seed),
        }
    }

    pub fn policy(&self) -> &TestPolicy {
        &self.policy
    }

    pub fn metrics(&self) -> HarnessMetrics {
        self.metrics
    }

    pub fn telemetry(&self) -> &[TelemetryEvent] {
        &self.telemetry
    }

    pub fn effects(&self) -> &[ObservedEffect] {
        &self.effects
    }

    pub fn checks(&self) -> &[ScenarioCheck] {
        &self.checks
    }

    /// The adversarial intents proposed during this run — the input side of
    /// the replay artifact.
    pub fn intents(&self) -> &[Intent] {
        &self.intents
    }

    pub fn rng_mut(&mut self) -> &mut idaptik_core::Mulberry32 {
        &mut self.rng
    }

    /// Record a named invariant check.
    pub fn check(&mut self, name: &str, passed: bool, detail: impl Into<String>) {
        self.checks.push(ScenarioCheck {
            name: name.to_owned(),
            passed,
            detail: detail.into(),
        });
    }

    fn record(&mut self, step: u64, what: &str, detail: String, accepted: bool) {
        self.effects.push(ObservedEffect {
            step,
            what: what.to_owned(),
            detail,
            accepted,
        });
    }

    /// The policy gate. A denial is itself an observed effect and a failed
    /// check only when the scenario expected the attempt to be authorised.
    pub fn authorise(&mut self, principal: &Principal, permission: Permission) -> bool {
        self.policy.decide(principal, permission) == PolicyDecision::Allow
    }

    /// Record an intent proposal and count it against the per-scenario
    /// budget. The intent is the replay artifact's input side: its raw wire
    /// payload is persisted verbatim, unsanitised.
    pub fn record_intent(&mut self, intent: &Intent) {
        self.intents.push(intent.clone());
        self.metrics.intents += 1;
    }

    /// Record the observed peak depth of an in-memory wire queue.
    pub fn metrics_peak(&mut self, depth: usize) {
        self.metrics.peak_queue_depth = self.metrics.peak_queue_depth.max(depth);
    }

    /// Advance the logical step counter (used by phases that pump without
    /// delivering, where [`Self::exchange_round`] does not run).
    pub fn bump_step(&mut self) {
        self.metrics.steps += 1;
        assert!(self.metrics.steps <= MAX_STEPS, "scenario exceeded MAX_STEPS");
    }

    /// Route one proposed command frame through the relay model and, when it
    /// relays, the real production validator of `target` (the peer seat).
    ///
    /// The adversary principal is only ever tested for
    /// [`Permission::ProposeBoundedIntent`]; delivery permissions belong to
    /// the harness principal and are checked explicitly.
    pub fn deliver_command(
        &mut self,
        relay: &mut RelayModel,
        target: &mut LockstepCore,
        proposer: &Principal,
        payload: Value,
    ) -> ValidatorOutcome {
        assert_eq!(proposer.kind, PrincipalKind::Adversary);
        assert!(self.authorise(proposer, Permission::ProposeBoundedIntent));
        assert!(self.authorise(
            &Principal::harness(),
            Permission::DeliverThroughTestRelay
        ));
        assert!(self.authorise(
            &Principal::harness(),
            Permission::FeedProductionValidator
        ));

        let step = target.step();
        // Clone for the bookkeeping arms: the relay verdict owns its own
        // copy of the relayed payload, while drop/reject reasons still need
        // to quote the original frame.
        let verdict = relay.command(payload.clone());
        self.metrics.wire_messages += 1;
        match verdict {
            RelayVerdict::Relayed { payload } => {
                let decoded = idaptik_net::envelope::decode_command(&payload);
                match decoded {
                    Ok((at, cmd)) => match target.on_peer_command(at, cmd) {
                        Ok(()) => {
                            self.telemetry.push(TelemetryEvent::Accepted {
                                command: cmd_name(&payload),
                                at,
                            });
                            self.record(step, "relayed+accepted", payload.to_string(), true);
                            ValidatorOutcome::Accepted
                        }
                        Err(e) => {
                            let violation = violation_for(&e);
                            self.telemetry
                                .push(TelemetryEvent::ValidatorRejected {
                                    violation,
                                    detail: e.to_string(),
                                });
                            self.record(step, "validator-rejected", e.to_string(), false);
                            ValidatorOutcome::Rejected(e)
                        }
                    },
                    Err(e) => {
                        let violation = violation_for(&e);
                        self.telemetry.push(TelemetryEvent::ValidatorRejected {
                            violation,
                            detail: e.to_string(),
                        });
                        self.record(step, "undecodable", e.to_string(), false);
                        ValidatorOutcome::Undecodable(e)
                    }
                }
            }
            RelayVerdict::Dropped { reason } => {
                self.telemetry.push(TelemetryEvent::RelayDropped {
                    reason: reason.to_owned(),
                });
                self.record(
                    step,
                    "relay-dropped",
                    format!("{reason}: {payload}"),
                    false,
                );
                ValidatorOutcome::Rejected(NetError::Session(format!("relay dropped: {reason}")))
            }
            RelayVerdict::Rejected { reason } => {
                let error = NetError::Session(format!("relay rejected: {reason}"));
                self.telemetry.push(TelemetryEvent::ValidatorRejected {
                    violation: ViolationKind::RoleForbidden,
                    detail: reason.clone(),
                });
                self.record(step, "relay-rejected", reason, false);
                ValidatorOutcome::Rejected(error)
            }
        }
    }

    /// Feed an already-decoded command straight to a lockstep core's
    /// validator, bypassing the relay model — used for post-resync frames the
    /// relay would simply forward. The production validator still runs.
    pub fn feed_raw_to_validator(
        &mut self,
        target: &mut LockstepCore,
        payload: &Value,
    ) -> ValidatorOutcome {
        assert!(self.authorise(
            &Principal::harness(),
            Permission::FeedProductionValidator
        ));
        self.metrics.wire_messages += 1;
        let step = target.step();
        match idaptik_net::envelope::decode_command(payload) {
            Ok((at, cmd)) => match target.on_peer_command(at, cmd) {
                Ok(()) => {
                    self.telemetry
                        .push(TelemetryEvent::Accepted {
                            command: cmd_name(payload),
                            at,
                        });
                    self.record(step, "accepted", payload.to_string(), true);
                    ValidatorOutcome::Accepted
                }
                Err(e) => {
                    self.telemetry.push(TelemetryEvent::ValidatorRejected {
                        violation: violation_for(&e),
                        detail: e.to_string(),
                    });
                    self.record(step, "validator-rejected", e.to_string(), false);
                    ValidatorOutcome::Rejected(e)
                }
            },
            Err(e) => {
                self.telemetry.push(TelemetryEvent::ValidatorRejected {
                    violation: violation_for(&e),
                    detail: e.to_string(),
                });
                self.record(step, "undecodable", e.to_string(), false);
                ValidatorOutcome::Undecodable(e)
            }
        }
    }

    /// Feed a raw commit watermark to the real commit validator.
    pub fn feed_commit(
        &mut self,
        target: &mut LockstepCore,
        role: Role,
        through: u64,
    ) -> Result<(), NetError> {
        self.metrics.wire_messages += 1;
        let raw = crate::wire::raw_commit(role, through);
        let commit = idaptik_net::envelope::Commit::from_control(&raw)
            .expect("raw_commit builds a valid commit control frame");
        match target.on_peer_commit(commit.through) {
            Ok(()) => {
                self.record(target.step(), "commit-accepted", format!("through={through}"), true);
                Ok(())
            }
            Err(e) => {
                self.telemetry.push(TelemetryEvent::ValidatorRejected {
                    violation: violation_for(&e),
                    detail: e.to_string(),
                });
                self.record(target.step(), "commit-rejected", e.to_string(), false);
                Err(e)
            }
        }
    }

    /// Record an injected transport fault (scenario 5). Faults come from the
    /// harness's own in-memory wire, never from a remote machine.
    pub fn record_fault(
        &mut self,
        kind: crate::telemetry::FaultKind,
        detail: impl Into<String>,
    ) {
        let detail = detail.into();
        self.telemetry.push(TelemetryEvent::Fault {
            kind,
            detail: detail.clone(),
        });
        self.record(self.metrics.steps, "fault", detail, false);
    }

    /// Record a host validator refusal observed through a production
    /// predicate rather than a delivered frame (handshake/epoch and resync
    /// checks, whose reject path lives in the session drivers). The detail
    /// string is still the host component's own wording.
    pub fn record_validator_rejection(
        &mut self,
        violation: ViolationKind,
        detail: impl Into<String>,
    ) {
        let detail = detail.into();
        self.telemetry
            .push(TelemetryEvent::ValidatorRejected {
                violation,
                detail: detail.clone(),
            });
        self.record(self.metrics.steps, "validator-rejected", detail, false);
    }

    /// Record a successful recovery.
    pub fn record_recovery(&mut self, via: impl Into<String>) {
        let via = via.into();
        self.telemetry.push(TelemetryEvent::Recovered { via: via.clone() });
        self.record(self.metrics.steps, "recovered", via, true);
    }

    /// Record a sim-level denial or no-op for target-claim scenarios.
    pub fn record_sim_outcome(
        &mut self,
        command: &str,
        denial: Option<&str>,
        no_op: bool,
    ) {
        if let Some(reason) = denial {
            self.telemetry.push(TelemetryEvent::SimDenied {
                command: command.into(),
                reason: reason.into(),
            });
            self.record(
                self.metrics.steps,
                "sim-denied",
                format!("{command}: {reason}"),
                false,
            );
        } else if no_op {
            self.telemetry.push(TelemetryEvent::SimNoOp {
                command: command.into(),
            });
            self.record(self.metrics.steps, "sim-no-op", command.into(), false);
        }
    }

    /// Advance both seats one bounded pump/deliver/advance round, mirroring
    /// the lockstep unit tests' in-memory wire. `in_flight` is the harness's
    /// own queue; its peak depth is the queue-growth measurement.
    pub fn exchange_round(
        &mut self,
        pair: &mut SeatPair,
        in_flight: &mut std::collections::VecDeque<WireFrame>,
        max_delivery_per_turn: usize,
    ) {
        let oa = pair.infiltrator.pump_outgoing(&mut pair.inf_feed);
        let ob = pair.hacker.pump_outgoing(&mut pair.hac_feed);
        for o in oa {
            in_flight.push_back(WireFrame::from_outgoing(Role::Infiltrator, o));
        }
        for o in ob {
            in_flight.push_back(WireFrame::from_outgoing(Role::Hacker, o));
        }
        self.metrics.peak_queue_depth = self.metrics.peak_queue_depth.max(in_flight.len());
        self.metrics.wire_messages = self
            .metrics
            .wire_messages
            .saturating_add(in_flight.len().min(max_delivery_per_turn));

        let mut delivered = 0;
        while delivered < max_delivery_per_turn {
            let Some(frame) = in_flight.pop_front() else { break };
            match frame {
                WireFrame::Command { from, at, cmd } => {
                    let target = match from {
                        Role::Infiltrator => &mut pair.hacker,
                        Role::Hacker => &mut pair.infiltrator,
                    };
                    target.on_peer_command(at, cmd).expect("valid fixture frames");
                }
                WireFrame::Commit { from, through } => {
                    let target = match from {
                        Role::Infiltrator => &mut pair.hacker,
                        Role::Hacker => &mut pair.infiltrator,
                    };
                    target.on_peer_commit(through).expect("valid fixture commits");
                }
            }
            delivered += 1;
        }
        pair.infiltrator.advance_with(|_, _| {});
        pair.hacker.advance_with(|_, _| {});
        self.metrics.steps += 1;
        assert!(self.metrics.steps <= MAX_STEPS, "scenario exceeded MAX_STEPS");
    }

    /// Apply typed commands to a fresh sim for exactly one tick and return
    /// the events the host emitted. The sim is the production gameplay
    /// validator; the harness only supplies inputs and reads outputs.
    pub fn sim_tick(script: &ScriptFile, commands: &[idaptik_core::scenario::command::Command])
        -> (GhostLobbySim, Vec<idaptik_core::scenario::event::Event>, RuntimeSnapshot) {
        use idaptik_core::scenario::command::{Buttons, fold};
        let mut sim = headless::build(script).expect("sim builds");
        let startup = sim.drain_events();
        let mut held = Buttons::default();
        let input: TickInput = fold(commands, &mut held);
        let mut events = startup;
        events.extend(sim.tick(&input));
        let snapshot = sim.snapshot();
        (sim, events, snapshot)
    }

    /// Final accounting: fail the scenario if any strict limit was crossed.
    pub fn enforce_limits(&mut self) {
        for limit in self.metrics.violations() {
            self.check(
                &format!("limit:{limit}"),
                false,
                format!("bounded-test limit crossed: {limit}; metrics={:?}", self.metrics),
            );
        }
    }
}

/// A frame on the harness's in-memory wire.
#[derive(Debug, Clone)]
pub enum WireFrame {
    Command {
        from: Role,
        at: u64,
        cmd: idaptik_core::scenario::command::Command,
    },
    Commit {
        from: Role,
        through: u64,
    },
}

impl WireFrame {
    pub fn from_outgoing(from: Role, outgoing: idaptik_net::lockstep::Outgoing) -> Self {
        match outgoing {
            idaptik_net::lockstep::Outgoing::Command { at, cmd } => {
                WireFrame::Command { from, at, cmd }
            }
            idaptik_net::lockstep::Outgoing::Commit { through } => {
                WireFrame::Commit { from, through }
            }
        }
    }
}

fn cmd_name(payload: &Value) -> String {
    payload
        .get("cmd")
        .and_then(Value::as_str)
        .unwrap_or("?")
        .to_owned()
}
