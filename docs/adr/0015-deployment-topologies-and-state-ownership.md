# ADR-0015: Deployment topologies, simulation authority, and state ownership

- Status: Accepted (architecture contract; runtime implementation is partial)
- Date: 2026-10-24
- Relates to: ADR-0002 (transport), ADR-0004 (deterministic simulation),
  ADR-0005 (relay-only slice), ADR-0006 (current Burble client path),
  ADR-0007 (versioned content package)

## Context and evidence boundary

The current IDApTIK code is a deterministic Rust game core and two-seat client
lockstep, not a hosted game server or a federation implementation. Keep this
fact separate from the deployment contract below.

- `crates/idaptik-net/src/lockstep.rs` is a sans-I/O state machine. Its tests
  exchange inputs between two cores over an in-memory wire and compare both
  event logs with the headless reference. `live.rs` instead drives the
  `PhoenixClient` to Burble's `game:<id>` topic. It is not a direct peer
  transport.
- `scripts/multiplayer-runtime.sh` starts a temporary, player-hosted Burble
  relay for the current host flow; both seats still run the Rust simulation.
  `scripts/loopback_check.sh` runs Burble's offline-capable test environment
  without GSA or VeriSimDB and checks relayed client lockstep/loss/resync. That
  is useful transport coverage, but it is **not** a server-authority test.
- At the Burble revision pinned by this repository
  ([`ef6756c`](https://github.com/metadatastician/burble/tree/ef6756cf1591406d434c5c04c3c018e935836e4c)),
  `BurbleWeb.GameChannel` validates a game profile's role table and relays
  `command`/`event` payloads. Both clients publish events; the channel does not
  run `GhostLobbySim` or persist game state. Burble's `Topology` module lists
  monarchic/oligarchic/distributed/serverless profiles, but that declaration
  does not turn this channel into an IDApTIK authority or prove cross-node
  behavior. The inspected application supervision tree starts its services
  independently of that topology descriptor. Production Burble also starts a
  VeriSimDB-backed account/config store; that is a separate Burble runtime
  dependency, not a GSA or IDApTIK simulation-state contract.
- The current Game Server Admin contract at
  [`8f089ac`](https://github.com/catachresis-colloquium/game-server-admin/tree/8f089ac2dda906b3bb845625fd25233104ee591b)
  has draft/fixture game-profile material, not a live IDApTIK deployment,
  session-authority, or failover API. A server must therefore run without GSA.
- VeriSimDB at [`4e6bae8`](https://github.com/hyperpolymath/verisimdb/tree/4e6bae8bddb8ede0eb4f19750dba75a55a36f7ff)
  has real federation query fan-out code, a Rust in-memory peer registry, and
  Elixir KRaft/WAL code and tests. Its readiness document still calls the
  executor/resolver stubs and consensus design-only. The sources do not line
  up cleanly: Elixir registration forwards no `secret` while the Rust
  registration handler requires a configured PSK; KRaft's remote transport
  documents HTTP RPC paths for which no handler was found in the inspected
  orchestration or Rust API routers; the application starts one configured
  KRaft node by default. Tests cover same-VM peers and WAL recovery, not a
  deployed cross-node partition/recovery. Therefore neither VeriSimDB
  federation nor its KRaft code is an IDApTIK session-authority dependency.
- SNIF's documented boundary at
  [`720eb82`](https://github.com/hyperpolymath/snifs/tree/720eb82168adb0947ab624a7e27972c7c7c9f9d6)
  is freestanding WebAssembly compute over marshalled values/buffers. SNIF
  guests do not receive database, socket, network, or BEAM process I/O.

This ADR specifies the target boundaries and state policy. It does not claim
that the direct peer transport, authoritative Rust server, LMDB session
journal, federation directory, GSA adapter, or multi-server failover has
already been implemented.

## Decision

### 1. Keep the deployment axes independent

A deployment is described by four separate facts, not by one overloaded
`mode` flag:

| Axis | Question | Values in this contract |
|---|---|---|
| Network topology | Which nodes can communicate, and through what boundary? | Peer-to-peer, federated servers, hosted server |
| Administrative ownership | Which people/operators own policy and configuration? | Participants, one server operator, or one operator per federation server |
| Simulation authority | Which process/replicas may commit the next game state? | Deterministic peer replicas, or one server authority per session |
| GSA integration | Is an optional Game Server Admin control client configured? | Absent or present; never a gameplay prerequisite |

`crates/idaptik-net/src/deployment.rs` records these axes and their state-class
placement as a declarative contract model. It is not used by the current
launcher and does not implement a server.

### 2. Support three topology profiles, each with and without GSA

| Topology | Administrative owner | Simulation authority | Required central services |
|---|---|---|---|
| Peer-to-peer | Each participant owns their local configuration. | Both peers run deterministic replicas over the same committed input prefix. This is cooperative lockstep, **not consensus** and not anti-cheat. | None for game operation. A reachable direct path (for example, LAN or an already-configured VPN/manual route) is still needed; this ADR does not promise NAT traversal. |
| Federated | Every server has its own operator and local policy. Federation is an explicit trust/allowlist boundary, not a shared administrator. | One home server is the sole authority for each session. Other servers may route/discover the session but do not co-write or replicate its simulation. | No global game-state database. A local authority store is required for durable hosted sessions; directory and operations services are optional/advisory. |
| Hosted | One operator owns the server's local configuration and sessions. | One server process is the sole authority for each hosted session. Clients submit inputs and consume authorized events/snapshots. | No GSA requirement. A local authority store is required for durable hosted sessions; the server can use validated local configuration when GSA is absent. |

All three profiles are valid with `GsaIntegration::Absent` and
`GsaIntegration::Present`. GSA is an optional management client/projection,
not the source of simulation truth or the only way to configure a server. If it
is present, its commands must be authenticated, scoped, versioned, validated
by the game server, and recorded against the responsible operator. If it is
absent or unreachable, local operator-owned configuration remains usable and
remote administrative edits are unavailable. GSA presence does not change a
session's topology, owner, authority, or storage policy.

A federated deployment is several independently owned hosted authorities, not
a single cluster with implicit shared state. Each session is pinned to one
home authority. Federation does not imply session migration, multi-writer
transactions, shared player state, or automatic high availability.

### 3. Separate simulation authority from the relay and management planes

- **Peer-to-peer:** each peer holds a deterministic replica. A peer may share a
  committed snapshot with a rejoining peer, as the current lockstep core
  already supports in memory. Neither replica is a consensus leader. Divergent
  or modified clients cannot be made trustworthy by a relay, cache, signature
  directory, or matching digest; this profile is for cooperative play unless a
  separate trust/anti-cheat design is adopted.
- **Hosted/federated:** a single server authority owns the ordered input stream,
  game tick, event sequence, snapshots, and commit acknowledgements for a
  session. Every record is scoped to `(session_id, authority_server_id,
  authority_epoch, sequence)`. The server must append the committed record to
  its local durable journal before acknowledging it. Clients may predict or
  cache presentation state, but may not commit outcomes.
- **No silent promotion:** if a session's authority is unavailable, that
  session pauses/fails closed. Other sessions on healthy federation servers
  continue. A different server must not accept writes merely because a
  directory says the owner is down. Transfer to a new authority requires a
  separately specified consensus-backed fencing/transfer protocol and a
  recoverable checkpoint. Until that exists, recovery is to the same authority
  from its local journal. This rule intentionally prefers temporary
  unavailability over split-brain game history.
- **No false consensus:** Dragonfly replication, BEAM distribution/cluster
  membership, Phoenix PubSub, health checks/service discovery, and VeriSimDB
  query federation are not session consensus, durable authority leases, or
  cross-server write coordination. None may elect or replace a session owner.

### 4. Place state by class, not by deployment-wide database choice

| State class | Owner and hot location | Durability and consistency | Partition behavior | Recovery |
|---|---|---|---|---|
| Active simulation / current tick | P2P: each peer's Rust process. Hosted/federated: one session authority's Rust process. | P2P: volatile replica state; deterministic only for the same ordered inputs. Hosted/federated: local LMDB journal/checkpoint under the authority; single-writer sequence. | P2P pauses at a missing peer/watermark. Hosted/federated pauses when the authority or a required seat is unavailable; no cache writes or alternate owner. | P2P adopts the survivor's validated snapshot/log. Hosted/federated replays the same authority's journal and resumes at its committed sequence. |
| Pending inputs and commit watermarks | Sending seat and/or session authority process memory; uncommitted data is not game truth. | Ordered per seat/session; not durable until committed. A server acknowledges only after its durable append. | Hold at the missing watermark. | Drop uncommitted inputs and resample/retry with idempotent sequence identity. |
| Committed session history / checkpoints | P2P: matching copies in peer memory. Hosted/federated: authority-owned process state plus local LMDB. | P2P: volatile for the life of the peers; no central persistence requirement. Hosted/federated: local journal/checkpoint durability for live-session recovery. LMDB is not shared between servers. Long-term save retention/schema compatibility remains open. | A disconnected peer waits for resync; loss of all peer copies ends a P2P run. A hosted session is unavailable if its owner/disk is unavailable. | Snapshot/log resync from the surviving peer, or same-authority LMDB replay. A failed durable append must not be acknowledged. |
| Membership and presence | Direct peer handshake for P2P. In a relay deployment, BEAM session processes and local ETS/Registry/Phoenix presence. | Ephemeral, node-local, best-effort; process/ETS restart is not durable membership. | Expire or mark disconnected; never infer a new simulation authority from presence alone. | Rejoin, reauthenticate, reannounce, then resync from the authority. |
| Federation directory / node health | Per-operator static manifest/allowlist; local ETS or optional Dragonfly cache. | Durable source is operator configuration; cached reachability is TTL/eventual and advisory only. Not a session log. | Stale/missing directory entry means unavailable or retry; it never changes session ownership. | Reprobe and reannounce configured nodes; retain the configured authority stamp. |
| Operator configuration / permissions | Each participant or server operator owns a local, versioned, validated configuration. GSA may submit changes but does not silently own them. | Local config file is the durable fallback; one writer/version per server. | Keep last validated local config; deny remote edits until the control plane returns. | Reload and reconcile explicit config versions and operator audit records. |
| Operations catalog / audit projection | Optional GSA/VeriSimDB operations plane, owned by the relevant operator. Not live simulation state. | Optional durable operations DB; projections are eventual and rebuildable from local manifests/audit exports. | Game sessions continue; new operations queries/edits may be unavailable. | Reconcile/rebuild the projection without replaying it as game authority. |
| Game content and scenario definitions | IDApTIK's versioned package/release; local files identified by content digest. | Immutable, versioned package is durable; package hash is the consistency check. | Use an already-installed matching package or refuse to start; no database lookup is required by the simulation. | Fetch the exact required digest from the release/package source. |
| Derived cache / route and lobby hints | No authoritative owner. P2P: no required store. Hosted/federated: local ETS or optional Dragonfly. | Disposable, TTL-bounded, reconstructible; never the only copy of state. | Cache miss or stale hint falls back to the configured source or returns unavailable. | Recompute from the owning session/server/configuration. |
| Media playout frames | P2P: no required store. Hosted/federated: Burble media subsystem, local bounded LMDB ring buffer with ETS fallback in the inspected implementation. | Short-lived ordered frames; the current buffer is 50 frames by default and is semantically disposable even if LMDB survives a process crash. | Drop/reset frames; voice/media loss does not mutate game state. | Flush and rejoin media. Never restore a game session from the playout buffer. |

The table is the policy for **live runtime state**. It does not settle player
save formats, cross-version save migration, or indefinite session-log retention;
those remain separate decisions.

### 5. Storage selection and execution boundary

- **BEAM process state** is suitable for session lifecycle, connection state,
  room coordination, and short-lived per-session orchestration. Supervision
  restarts processes; it does not make their heap durable or replicated.
- **ETS** is suitable for local lookup/rate-limit/presence tables and other
  disposable state. It is owned by a local VM/table owner and must be
  reconstructed after loss. It is not a distributed transaction store.
- **Local LMDB** is selected only for the durable live-session journal and
  checkpoint on a hosted/federated authority (subject to implementation and
  crash tests). It remains local to that authority; it does not replicate or
  resolve split brain. P2P operation must not require LMDB. Burble's existing
  LMDB module is a bounded audio playout ring, not IDApTIK session persistence.
- **Dragonfly** is optional for derived, short-lived directory/lobby/operations
  caches only. No Dragonfly integration was found in the inspected Burble
  revision or GSA tree; this is a future cache option, not existing capability.
  A Dragonfly outage must become a cache miss, not a loss of game truth. Its
  replication/persistence configuration is not consensus.
- **VeriSimDB** may hold optional GSA/operations metadata. Its current
  federation query path is not an IDApTIK game-state transaction or authority
  protocol; no game command, snapshot, or session journal is stored there by
  this decision.
- **SNIF guests** remain pure compute/buffer kernels. Database access, file
  access, sockets, service discovery, and all network I/O are host operations
  outside the guest boundary.

No single store is required in every topology. In particular, P2P can run with
no GSA, VeriSimDB, Dragonfly, or persistent server; hosted play can run with no
GSA; federation can route sessions without sharing their authoritative logs.

## Contract tests and implementation gates

The Rust model tests check that (a) all three topologies validate with GSA
absent and present without changing the other axes, (b) peer state does not
require central storage, (c) hosted state can be configured without GSA and
places durable history at the session authority, and (d) a federation
partition fails closed and recovery rejects a different authority or epoch.
The `LockstepCore` test `peer_lockstep_runs_without_central_services` exercises
two deterministic peers over an in-memory wire. These are **contract/model
checks**, not production transport/server/storage integration tests.

Before any runtime is described as supporting the corresponding mode, add and
run all of these integration gates:

1. **Peer-to-peer:** run two real clients over a direct transport with no
   Burble, GSA, VeriSimDB, Dragonfly, or other central service; include join,
   ordered lockstep, one-peer loss/rejoin/resync, and a check that no hidden
   server dependency is contacted.
2. **Hosted without GSA:** start one real authoritative game server with GSA
   absent, connect both seats, verify only the host can commit outcomes,
   verify the session pauses on missing required inputs, and crash/restart from
   the local journal without changing the committed event prefix. Keep this
   distinct from the existing relay-only loopback test.
3. **Federated failure/recovery:** run at least two independent server
   processes with different operators and sessions on each; partition or kill
   the home authority, assert its session fails closed while the other server's
   sessions continue, restart the home authority from its own LMDB checkpoint,
   and reject stale-epoch writes. Do not test or claim automatic promotion
   until a real consensus/fencing protocol is separately specified and proven.

The existing `scripts/loopback_check.sh` remains valuable for the Burble relay
contract, and is intentionally not relabelled as any of the three gates above.

## Consequences and deferred decisions

- ADR-0005 remains the accepted relay-only **vertical-slice** design; it is not
  the universal production topology. ADR-0006's Phoenix rendezvous describes
  the current client path, not a ban on a future direct peer transport.
- The current IDApTIK launcher still starts the peer-hosted relay and both
  clients simulate. No direct peer wire, hosted authority service, LMDB session
  journal, GSA adapter, or cross-server session router is implemented here.
  The implementation must land in the responsible repositories and pass the
  integration gates above before the product claims these modes.
- Automatic cross-server authority transfer, NAT traversal/rendezvous,
  matchmaking/public discovery, long-term save compatibility, and retention
  are not decided by this ADR.
- VeriSimDB may independently improve its federation/KRaft contracts, but
  IDApTIK will adopt them only after the actual route, transport, auth, recovery,
  and failure guarantees are demonstrated against the exact supported version.
