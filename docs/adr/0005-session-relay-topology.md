# ADR-0005: Session relay topology — relay-only lockstep over typed Command/Event

- Status: Accepted (vertical-slice milestone)
- Date: 2026-07-17

## Context

Issue #5 asks the Elixir session layer (ADR-0002) to carry the Ghost Lobby
scenario's typed `Command`/`Event` stream between the two asymmetric roles,
with `idaptik-core` defining the sole game semantics. Which process commits
simulation state is a separate, topology-specific authority decision
(ADR-0013). Two topologies were on the table:

1. **Rust-authoritative host** — a headless Rust process runs `GhostLobbySim`
   per session; Elixir relays each role's `Command`s to it and broadcasts the
   resulting `Event`s. One true world; clients are dumb terminals.
2. **Relay-only** — Elixir fans `Command`s/`Event`s between the clients; each
   client runs its own deterministic sim in lockstep.

The issue frames this milestone as *vertical-slice de-risking*: prove the
homoiconic "interaction is data" design survives the network boundary.

## Decision

**Relay-only (option 2) for this slice.** `IdaptikServerWeb.SessionChannel`
relays the serde wire enums verbatim — `"command"` messages (tagged `"cmd"`)
and `"event"` messages (tagged `"event"`) pass through byte-preserving to the
other seat. Elixir reads exactly one key of each payload (the tag), for routing
and role enforcement; it interprets nothing else.

What the evidence in the codebase supports:

- **Lockstep is already guaranteed.** ADR-0004 made the sim deterministic and
  event-sourced by construction; `replay_determinism`, `snapshot_equivalence`
  and the RNG vector tests enforce it. Two sims fed the same seed and the same
  ordered `Command` stream *are* the same world — the relay only has to
  preserve order and bytes, which is what a Phoenix Channel topic does.
- **The seam was built for this.** `command::fold` documents itself as "the
  seam the Elixir session layer and the TUI frontend share": clients fold the
  merged command stream into `TickInput`s locally. No host process exists to
  reuse — `idaptik-ffi` is a C ABI for engine bindings, not a network daemon —
  so option 1 means building and *operating* a new surface (per-session OS
  processes or NIFs under Elixir supervision, stdin/stdout or socket framing,
  crash/restart semantics) before the slice proves anything.
- **The slice's question is the wire, not authority.** What needed de-risking
  is whether `Command`/`Event` survive the boundary unchanged. The shared
  fixture (`fixtures/session_relay/`, captured via `idaptik-tui --headless`)
  is asserted byte-identical on both sides: the Elixir channel test proves
  pass-through, the Rust test (`session_relay_fixture.rs`) proves each relayed
  value deserializes into the real types and re-serializes identically.

Role enforcement is a pure routing table over the `"cmd"` tag — body verbs
(`SetButton`/`Jump`/`Interact`/`ThrowUsb`) are infiltrator-side; uplink and
pivot verbs (`Uplink`/`Pivot`/`Unpivot` — the v2 additions) are hacker-side;
session/test immediates (`Pause`/`Restart`/`Force*`) are either seat. An
optional integer `seq` envelope key (stripped before relay, never part of the
Rust JSON) lets the channel acknowledge-and-drop duplicate or out-of-order
commands instead of relaying them.

## Trade-offs, honestly

Option 1 remains the better end state for a **server-authoritative hosted
session**, and the issue is right to prefer it for a shipped hosted game. It is
not the only supported deployment topology: ADR-0013 separately defines
cooperative peer replicas and independently operated federated servers.

- **Cheating.** In relay-only, each client computes its own truth; a modified
  client can lie about outcomes. Acceptable between two cooperating
  playtesters; unacceptable for open play.
- **Late join / reconnection.** With no server-held sim there is no snapshot to
  hand a reconnecting client; peers must exchange `snapshot()`/`restore()`
  blobs themselves.
- **Hidden information.** A lockstep peer receives the other role's commands
  and could derive state its role should not see. The Ghost Lobby's asymmetry
  is currently cooperative, so this is tolerable — but a competitive mode is
  not viable on this topology.
- **Drift detection.** Lockstep failures (a non-determinism bug, a version
  skew) surface as silent divergence. Mitigated, not solved, by the `"event"`
  cross-feed: clients can hash and compare event streams.

None of these block the vertical slice, and none are made worse later: the
wire protocol is topology-agnostic. Moving to a Rust-authoritative host changes
*who consumes* the `"command"` topic and *who publishes* `"event"` (a host
process joining the session as a third, privileged participant) — the payload
shapes, the role table, and every client stay as they are. The fixture tests
carry over unchanged. That migration gets its own ADR when it lands (expected
alongside matchmaking/presence, where a server-held world is needed anyway).

## Consequences

- The relay stays logic-free: the diff greps clean of scoring, FSM, and tick
  math. The only game-adjacent knowledge is the `Command` tag → seat table
  (now in `Burble.Games.Idaptik` in the burble repo), which is protocol routing,
  not rules.
- Clients own simulation cost and pacing (tick cadence is theirs); the relay
  scales by connection count, not by session CPU.
- The `seq` envelope is the seam where lockstep input-delay scheduling will
  attach when real clients arrive; today it only de-duplicates.
- `fixtures/session_relay/` is a cross-language contract: regenerate
  `events.json` with `idaptik-tui --headless --script
  fixtures/session_relay/capture_script.json` (take `event_log`) whenever the
  `Event` wire format changes, and keep `commands.json` covering the full
  `Command` alphabet (a Rust test enforces this).

## Amendment (2026-07-21): the `at` scheduling envelope

The consequence above predicted the `seq` envelope is "the seam where lockstep
input-delay scheduling will attach when real clients arrive". The first real
client (`crates/idaptik-net`) landed scheduling client-side as a second
envelope key instead: `"at"` — the lockstep tick a command is scheduled for.
Clients author and consume it; the relay's behaviour is unchanged (it strips
exactly `"seq"` and relays the rest verbatim), so `"at"` passes through
untouched, and serde's internally-tagged `Command` decoding ignores it.
Neither envelope key is ever part of the Rust `Command` JSON itself. Clients
also exchange `"event"`-relayed control messages namespaced `"net:*"`
(handshake and event-log digest — the drift-detection mitigation above, made
real); the namespace cannot collide with the sim's `Event` tags, and the relay
treats them as ordinary events.

The live client (interactive slice, same day) grew the `"net:*"` vocabulary
without touching the relay: `net:commit` (a cumulative per-seat watermark —
"every command of mine with `at <= through` has been pushed", which is what a
sparse command stream cannot say on its own) and `net:resync` (a
`RuntimeSnapshot` hand-off to a rejoining peer, with both seats' committed
pending commands). `net:hello` gained a `rejoin` flag and its `proto` moved
to 2. All of it rides the existing `"event"` pass-through; the relay still
strips exactly `"seq"` and interprets nothing.

## Amendment (2026-08-11): relay now lives in burble

The relay implementation has been **moved from IDApTIK to burble**. As of estate
ruling 2026-08-04, burble is the estate's gaming communication platform, and its
`game:<session_id>` channel (burble PR #182) provides the same relay-only
lockstep topology at full parity with IDApTIK's former `SessionChannel`.

- The `BurbleWeb.GameChannel` joins on `"game:"* topics and relays
  `"command"`/`"event"` messages verbatim, stripping only the `"seq"` envelope
  exactly as IDApTIK's `SessionChannel` did.
- Role enforcement is driven by a **game profile registry** (`Burble.Games`)
  instead of a hardcoded table: each game (including IDApTIK as `"idaptik"`)
  declares its `command_roles()` mapping, which the channel consults at join
  time. The IDApTIK profile (`Burble.Games.Idaptik`) carries the complete
  command table including `NetSsh`/`NetHack` (fixing the bug that prompted
  IDApTIK PR #71).
- The `IdaptikServerWeb.SessionChannel` and the entire `server/` directory
  have been **removed** from IDApTIK. The client code (`crates/idaptik-net`,
  `crates/idaptik-bevy`) now joins `game:<id>` with `game: "idaptik"` params.
- The wire contract (`fixtures/session_relay/`) is unchanged; the four-leg
  loopback gate (ADR-0006 §4) now runs against burble in both CI suites.
- The relay remains transport-agnostic (ADR-0006 §2); burble uses Bandit
  + Phoenix Channels (ADR-0002), the same stack.
