//! The real-relay leg of the Evil Weevil corpus (ADR-0013).
//!
//! Ignored by default: it opens a WebSocket and therefore runs only when
//! `scripts/evil_weevil_relay_check.sh` starts a throwaway local burble and
//! invokes it with `--ignored` and `IDAPTIK_RELAY_URL` set. It never dials
//! anything but the loopback endpoint the script controls, and it sends a
//! fixed, bounded handful of frames with a guest dummy identity.
//!
//! The offline corpus models the same relay rules in
//! `idaptik_evil_weevil::relay_model`; this test confirms the model against
//! the pinned burble `GameChannel` itself, including the demonstrated
//! `Signal` profile gap.

use idaptik_net::envelope::{Role, encode_command};
use idaptik_net::phoenix::PhoenixClient;
use idaptik_net::ws::PlainWebSocketTransport;
use idaptik_core::scenario::command::Command;
use idaptik_core::scenario::tuning::ActionKind;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

fn relay_url() -> String {
    // 4002 is burble's hard-coded MIX_ENV=test port (config/test.exs); the
    // gate script exports IDAPTIK_RELAY_URL explicitly anyway.
    std::env::var("IDAPTIK_RELAY_URL")
        .unwrap_or_else(|_| "ws://127.0.0.1:4002/voice/websocket".into())
}

fn unique_session() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    format!("evil-weevil-{nanos}")
}

async fn client() -> PhoenixClient<PlainWebSocketTransport> {
    let mut url = relay_url();
    if !url.contains('?') {
        url.push('?');
    }
    url.push_str("guest=true&display_name=ew-dummy-relay-0001");
    let transport = PlainWebSocketTransport::connect(&url)
        .await
        .expect("local throwaway relay is reachable");
    PhoenixClient::new(transport)
}

#[tokio::test]
#[ignore = "requires the throwaway local burble from scripts/evil_weevil_relay_check.sh"]
async relay_duplicate_seq_is_dropped_and_role_misrouting_rejected() {
    let mut hacker = client().await;
    hacker
        .join(
            &format!("game:{}", unique_session()),
            json!({ "role": Role::Hacker.as_str(), "game": "idaptik" }),
        )
        .await
        .expect("hacker seat joins the idaptik game lane");

    // 1. A fresh typed command relays.
    let first = encode_command(
        &Command::Uplink {
            kind: ActionKind::Camera,
        },
        1,
        2,
    )
    .expect("encode");
    let ok = hacker.push("command", first).await.expect("fresh command relays");
    assert_eq!(ok.get("relayed").and_then(|v| v.as_bool()), Some(true));

    // 2. The identical seq again: acknowledged-and-dropped, not relayed.
    let dup = encode_command(
        &Command::Uplink {
            kind: ActionKind::Camera,
        },
        1,
        2,
    )
    .expect("encode");
    let dropped = hacker.push("command", dup).await.expect("duplicate is an ack, not an error");
    assert_eq!(
        dropped.get("relayed").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert_eq!(
        dropped.get("reason").and_then(|v| v.as_str()),
        Some("stale_or_duplicate")
    );

    // 3. A lower seq: same stale treatment.
    let stale = encode_command(
        &Command::Uplink {
            kind: ActionKind::Door,
        },
        0,
        3,
    )
    .expect("encode");
    let stale_reply = hacker.push("command", stale).await.expect("stale is acked");
    assert_eq!(
        stale_reply.get("relayed").and_then(|v| v.as_bool()),
        Some(false)
    );

    hacker.leave().await;
}

#[tokio::test]
#[ignore = "requires the throwaway local burble from scripts/evil_weevil_relay_check.sh"]
async relay_role_table_rejects_misrouted_and_unknown_commands() {
    let mut infiltrator = client().await;
    infiltrator
        .join(
            &format!("game:{}", unique_session()),
            json!({ "role": Role::Infiltrator.as_str(), "game": "idaptik" }),
        )
        .await
        .expect("infiltrator seat joins");

    // Uplink is a hacker verb; the infiltrator seat must be refused.
    let misrouted = encode_command(
        &Command::Uplink {
            kind: ActionKind::Camera,
        },
        1,
        1,
    )
    .expect("encode");
    assert!(
        infiltrator.push("command", misrouted).await.is_err(),
        "role enforcement must refuse the misrouted command"
    );

    // The demonstrated profile gap (ADR-0013): the Signal verb exists in the
    // client table but the pinned burble profile carries no Signal entry.
    // Record the observed verdict; if burble adds the entry, this assertion
    // changes and the ADR defect entry is re-evaluated.
    let signal = json!({ "cmd": "Signal", "at": 1, "seq": 2 });
    assert!(
        infiltrator.push("command", signal).await.is_err(),
        "Signal currently fails as an unknown command at the relay"
    );

    // Non-object payloads are refused politely rather than crashing.
    assert!(infiltrator
        .push("command", json!("not-an-object"))
        .await
        .is_err());

    infiltrator.leave().await;
}

#[tokio::test]
#[ignore = "requires the throwaway local burble from scripts/evil_weevil_relay_check.sh"]
async relay_join_rejects_unknown_role_and_game() {
    let mut seat = client().await;
    let bad_role = seat
        .join(
            &format!("game:{}", unique_session()),
            json!({ "role": "spectator", "game": "idaptik" }),
        )
        .await;
    assert!(bad_role.is_err(), "unknown roles cannot join");

    let bad_game = seat
        .join(
            &format!("game:{}", unique_session()),
            json!({ "role": Role::Hacker.as_str(), "game": "not-idaptik" }),
        )
        .await;
    assert!(bad_game.is_err(), "unknown games cannot join");
}
