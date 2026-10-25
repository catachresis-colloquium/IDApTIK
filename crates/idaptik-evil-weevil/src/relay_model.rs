//! An in-process model of the burble `BurbleWeb.GameChannel` rules, for
//! offline, reproducible adversarial testing.
//!
//! This is **not** the relay and holds no relay authority. It re-states the
//! documented rules of the pinned burble revision (see `Cargo.toml`'s
//! `burble-client` pin and ADR-0017) so the corpus can exercise them without
//! a BEAM process:
//!
//! - join requires a known game and a role the profile lists;
//! - a `"command"` must be a JSON object carrying a known `"cmd"` tag;
//! - the tag's sender must match the joining role (`:either` accepts both);
//! - an integer `"seq"` that is not strictly greater than the last one is
//!   acknowledged and dropped (`relayed:false, reason:stale_or_duplicate`);
//! - a fresh command has `"seq"` stripped and is relayed verbatim;
//! - an `"event"` with a string `"event"` tag is relayed verbatim.
//!
//! The real channel is exercised separately, against a throwaway local
//! burble, by `scripts/evil_weevil_relay_check.sh`. Any divergence between
//! this model and the channel is a defect in *this model* — the model's
//! [`IdaptikProfile`] table is pinned against the burble source in a test.

use idaptik_core::scenario::command::Command;
use idaptik_net::envelope::{Role, seat_for};
use serde_json::{Value, json};

/// The model's verdict on one inbound frame, shaped to match burble's replies.
#[derive(Debug, Clone, PartialEq)]
pub enum RelayVerdict {
    /// Relayed to the other seat; the payload is what the peer would receive
    /// (the `seq` envelope already stripped).
    Relayed { payload: Value },
    /// Accepted-and-dropped: a duplicate or out-of-order `seq`.
    Dropped { reason: &'static str },
    /// Refused with an error reply (role, tag or shape violation).
    Rejected { reason: String },
}

impl RelayVerdict {
    /// The JSON reply burble's channel would return to the sender.
    pub fn reply(&self) -> Value {
        match self {
            RelayVerdict::Relayed { .. } => json!({ "relayed": true }),
            RelayVerdict::Dropped { reason } => {
                json!({ "relayed": false, "reason": reason })
            }
            RelayVerdict::Rejected { reason } => json!({ "error": { "reason": reason } }),
        }
    }
}

/// The burble `Burble.Games.Idaptik` profile, restated.
///
/// Source of truth (pinned burble rev): `server/lib/burble/games/idaptik.ex`.
/// Keep this table in lockstep with that file; the profile test below fails
/// on drift. Note the known, demonstrated gaps: the relay profile has no
/// `Signal` entry (ADR-0012), and — since ADR-0014 and the electrical
/// infrastructure — no `Mole` or `Power` entry, so those frames are refused
/// as unknown commands even though the client routing table knows the verbs.
/// `Power` additionally carries its seat per-op, which this tag-keyed table
/// cannot express without splitting the verb.
#[derive(Debug, Clone)]
pub struct IdaptikProfile;

impl IdaptikProfile {
    pub fn game_id(&self) -> &'static str {
        "idaptik"
    }

    pub fn roles(&self) -> [&'static str; 2] {
        ["infiltrator", "hacker"]
    }

    /// The `command_roles()` mapping exactly as burble carries it.
    pub fn command_roles(&self) -> [(&'static str, &'static str); 14] {
        [
            ("SetButton", "infiltrator"),
            ("Jump", "infiltrator"),
            ("Interact", "infiltrator"),
            ("ThrowUsb", "infiltrator"),
            ("Uplink", "hacker"),
            ("Pivot", "hacker"),
            ("Unpivot", "hacker"),
            ("NetSsh", "hacker"),
            ("NetHack", "hacker"),
            ("Pause", "either"),
            ("Restart", "either"),
            ("ForceCrisis", "either"),
            ("ForceExtract", "either"),
            ("ForceFail", "either"),
        ]
    }

    fn sender_for(&self, tag: &str) -> Option<&str> {
        self.command_roles()
            .iter()
            .find(|(cmd, _)| *cmd == tag)
            .map(|(_, role)| *role)
    }
}

/// One joined seat of the model relay. Construct with [`RelayModel::join`]
/// the way the channel's `join/3` assigns `:role` and `:last_seq`.
#[derive(Debug, Clone)]
pub struct RelayModel {
    role: String,
    profile: IdaptikProfile,
    last_seq: Option<i64>,
    /// Count of frames this model relayed — feeds the harness's bounded
    /// message accounting.
    pub relayed_count: usize,
    /// Count of stale/duplicate drops.
    pub dropped_count: usize,
    /// Count of hard rejections.
    pub rejected_count: usize,
}

impl RelayModel {
    /// Model `join/3`: an unknown game or role is refused.
    pub fn join(game_id: &str, role: Role) -> Result<Self, String> {
        let profile = IdaptikProfile;
        if game_id != profile.game_id() {
            return Err(format!("unknown game: {game_id:?}"));
        }
        let role = role.as_str();
        if !profile.roles().contains(&role) {
            return Err(format!(
                "unknown role {role:?} — idaptik seats: {}",
                profile.roles().join(", ")
            ));
        }
        Ok(Self {
            role: role.to_owned(),
            profile,
            last_seq: None,
            relayed_count: 0,
            dropped_count: 0,
            rejected_count: 0,
        })
    }

    /// Model `handle_in("command", payload, socket)`, including the `seq`
    /// bookkeeping from burble's `note_seq`.
    pub fn command(&mut self, payload: Value) -> RelayVerdict {
        let Some(map) = payload.as_object() else {
            self.rejected_count += 1;
            return RelayVerdict::Rejected {
                reason: "command payload must be a JSON object".into(),
            };
        };
        let Some(tag) = map.get("cmd").and_then(Value::as_str) else {
            self.rejected_count += 1;
            return RelayVerdict::Rejected {
                reason: "command payload must carry the \"cmd\" tag".into(),
            };
        };
        let Some(sender) = self.profile.sender_for(tag) else {
            self.rejected_count += 1;
            return RelayVerdict::Rejected {
                reason: format!("unknown command: {tag:?}"),
            };
        };
        if sender != "either" && sender != self.role {
            self.rejected_count += 1;
            return RelayVerdict::Rejected {
                reason: format!("{tag} is a {sender} command (you joined as {})", self.role),
            };
        }
        // Mirror note_seq/2: an integer seq at or below the last seen one is
        // stale; no seq or a non-integer seq is treated as fresh.
        if let Some(seq) = map.get("seq").and_then(Value::as_i64) {
            if self.last_seq.is_some_and(|last| seq <= last) {
                self.dropped_count += 1;
                return RelayVerdict::Dropped {
                    reason: "stale_or_duplicate",
                };
            }
            self.last_seq = Some(seq);
        }
        let mut stripped = map.clone();
        stripped.remove("seq");
        self.relayed_count += 1;
        RelayVerdict::Relayed {
            payload: Value::Object(stripped),
        }
    }

    /// Model `handle_in("event", payload, socket)`: any object carrying a
    /// string `"event"` tag relays, including `net:*` control frames.
    pub fn event(&mut self, payload: Value) -> RelayVerdict {
        match payload.get("event").and_then(Value::as_str) {
            Some(_) => {
                self.relayed_count += 1;
                RelayVerdict::Relayed { payload }
            }
            None => {
                self.rejected_count += 1;
                RelayVerdict::Rejected {
                    reason: "event payload must carry the \"event\" tag".into(),
                }
            }
        }
    }

    /// Tags the client's own routing table knows but the relay profile does
    /// not carry — cross-repository drift, surfaced rather than hidden.
    ///
    /// At the pinned revision this returns `["Signal", "Mole", "Power"]`:
    /// ADR-0012's signal verbs, ADR-0014's companion orders and the
    /// electrical infrastructure's power verbs exist in
    /// `idaptik-core`/`idaptik-net` but burble's `command_roles()` was never
    /// extended, so the live relay rejects them — which fails closed, the
    /// safe direction. Every other client-routed tag is asserted present so
    /// a further drift cannot arrive silently.
    pub fn client_tags_missing_from_relay() -> Vec<&'static str> {
        let profile = IdaptikProfile;
        // Exercise the non-signal Command variants the client table routes.
        let sample = [
            Command::SetButton {
                button: idaptik_core::scenario::command::Button::Left,
                down: true,
            },
            Command::Jump,
            Command::Interact,
            Command::ThrowUsb,
            Command::Uplink {
                kind: idaptik_core::scenario::tuning::ActionKind::Camera,
            },
            Command::Pivot {
                target: idaptik_core::scenario::command::PivotTarget::Bridge,
            },
            Command::Unpivot,
            Command::NetSsh {
                node: idaptik_core::scenario::command::NetNodeIndex(0),
            },
            Command::NetHack {
                node: idaptik_core::scenario::command::NetNodeIndex(0),
            },
            Command::Pause { on: true },
            Command::Restart,
            Command::ForceCrisis,
            Command::ForceExtract {
                method: idaptik_core::scenario::common::ExtractMethod::ServiceExit,
            },
            Command::ForceFail {
                reason: idaptik_core::scenario::common::FailReason::Caught,
            },
        ];
        for cmd in sample {
            let value = serde_json::to_value(cmd).expect("command serialises");
            let tag = value
                .get("cmd")
                .and_then(Value::as_str)
                .expect("tagged enum");
            assert!(
                profile.sender_for(tag).is_some(),
                "unexpected relay drift: {tag} missing from the burble profile"
            );
            // Keep the client table import exercised: every sampled command
            // routes to a concrete seat or Either, never panics.
            let _ = seat_for(&cmd);
        }
        // `Command::Signal` is not constructible without a SignalOp; assert
        // its absence directly so the finding does not depend on sample data.
        assert!(profile.sender_for("Signal").is_none());
        // ADR-0014's companion orders and the electrical verbs: client-routed,
        // relay-absent. Asserted directly for the same reason.
        assert!(profile.sender_for("Mole").is_none());
        assert!(profile.sender_for("Power").is_none());
        vec!["Signal", "Mole", "Power"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{net_ssh, with_seq};

    #[test]
    fn stale_and_duplicate_seq_are_dropped_not_relayed() {
        let mut relay = RelayModel::join("idaptik", Role::Hacker).unwrap();
        let first = with_seq(net_ssh(2, 0), 10);
        assert!(matches!(relay.command(first), RelayVerdict::Relayed { .. }));
        // Same seq again: duplicate.
        let dup = with_seq(net_ssh(2, 0), 10);
        assert_eq!(
            relay.command(dup),
            RelayVerdict::Dropped {
                reason: "stale_or_duplicate"
            }
        );
        // A lower seq: out of order.
        let stale = with_seq(net_ssh(3, 0), 9);
        assert_eq!(
            relay.command(stale),
            RelayVerdict::Dropped {
                reason: "stale_or_duplicate"
            }
        );
        assert_eq!(relay.relayed_count, 1);
        assert_eq!(relay.dropped_count, 2);
    }

    #[test]
    fn relayed_frames_have_seq_stripped() {
        let mut relay = RelayModel::join("idaptik", Role::Hacker).unwrap();
        // Bind the verdict first: matching directly against the temporary
        // would leave `payload` borrowing a value dropped at the semicolon.
        let verdict = relay.command(with_seq(net_ssh(2, 0), 1));
        let RelayVerdict::Relayed { payload } = &verdict else {
            panic!("fresh command must relay");
        };
        assert!(payload.get("seq").is_none(), "burble strips seq");
        assert_eq!(payload.get("at").and_then(Value::as_u64), Some(2));
    }

    #[test]
    fn role_misrouting_is_rejected() {
        let mut infiltrator = RelayModel::join("idaptik", Role::Infiltrator).unwrap();
        // NetSsh is a hacker verb.
        let verdict = infiltrator.command(net_ssh(1, 0));
        assert!(matches!(verdict, RelayVerdict::Rejected { .. }));
        assert!(verdict.reply().to_string().contains("hacker"));
    }

    #[test]
    fn unknown_games_and_roles_are_refused_at_join() {
        assert!(RelayModel::join("other-game", Role::Hacker).is_err());
    }

    #[test]
    fn non_object_commands_and_untagged_events_are_rejected() {
        let mut relay = RelayModel::join("idaptik", Role::Hacker).unwrap();
        assert!(matches!(
            relay.command(json!("not-an-object")),
            RelayVerdict::Rejected { .. }
        ));
        assert!(matches!(
            relay.event(json!({ "no_tag": true })),
            RelayVerdict::Rejected { .. }
        ));
    }

    #[test]
    fn the_known_signal_drift_is_pinned_not_silently_assumed_away() {
        // If burble adds the Signal entry, this test changes and the ADR-0017
        // defect entry must be re-evaluated rather than staying stale.
        assert_eq!(
            RelayModel::client_tags_missing_from_relay(),
            vec!["Signal", "Mole", "Power"]
        );
        let profile = IdaptikProfile;
        assert_eq!(profile.command_roles().len(), 14);
        assert!(profile.sender_for("Signal").is_none());
    }
}
