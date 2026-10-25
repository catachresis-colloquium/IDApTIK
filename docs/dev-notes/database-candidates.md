# Choosing a database for IDApTIK

Status: **Tier 1 is built; Tiers 2–3 are still analysis.** See
§*What was actually built* at the end. The numbers are measured from this tree;
the external facts are dated and sourced.

## What the game actually stores

| Thing | Size on disk | Shape | Changes |
| --- | --- | --- | --- |
| Scenario definition (`exchange_house.json`) | 22,420 B | immutable, authored in Nickel | at build time |
| Run snapshot (`exchange_house_run.json`) | 69,215 B | `BuildingSnapshot` = definition + state + validation | every checkpoint |
| Full scenario (`ghost-lobby-scenario.json`) | 11,060 B | `ScenarioDefinition` | at build time |
| Per-tick state | in memory | `BuildingState`, `InfrastructureSim` | every tick |
| Commands | in memory | `Vec<BuildingCommand>` | every tick |
| Events / debrief | in memory | `BuildingEvent`, `BuildingDebrief` | every tick |
| Netplay frames | wire | `serde_json` over Phoenix Channels | 50 ms |

A save is **kilobytes**. This is the single fact that should decide the
question, and it makes most database comparisons irrelevant: nothing here is
volume-limited, throughput-limited or fan-out-limited.

Six format tags already exist, in three conventions:

```
idaptik-ghost-lobby-v2            definition
idaptik-scenario/1                building definition   <- the odd one out
idaptik-ghost-lobby-runtime-v3    snapshot
idaptik-ghost-lobby-export-v3     export
idaptik-building-runtime-v1       snapshot
idaptik-building-after-action-v1  debrief
```

Whatever store is chosen, **the migration story is these strings**, and today
they are convention, not contract. That is the real persistence debt — not the
absence of a database engine.

## The rule

**The tick does not talk to a database.**

`BuildingSim::apply` and `GhostLobbySim::tick` run on a Bevy `FixedUpdate`
schedule and their state is `serde`-round-trippable by construction. Putting a
store — any store — between the tick and its own state would add latency, a
failure mode, and an async boundary to code that is currently a pure function of
`(previous state, command)`.

So the question is not "which database runs the game". It is:

> which store holds the things the tick produces, so that a session can be
> saved, resumed, replayed, audited and queried after the fact?

Those are different requirements, and they have different answers.

## The observation that reframes it

The simulation is already event-sourced, whether or not anyone called it that:

| Event sourcing | Already in the tree |
| --- | --- |
| command log | `Vec<BuildingCommand>`, and `exchange_house_script()` is a literal one |
| materialised view | `BuildingState`, rebuilt by `apply` |
| snapshot | `BuildingSnapshot`, restores exactly (`restore()`) |
| read model | `BuildingDebrief`, `BuildingExport` |
| schema version | `BUILDING_SNAPSHOT_FORMAT` |

What is missing is not a query engine. It is a durable place to put (a) the log,
(b) the latest snapshot, and (c) an index over sessions. That is a **log plus a
key-value store**, about 300 lines of Rust.

## Candidates

| | Licence | Runtime | Shape | Runs in the tick? | Verdict |
| --- | --- | --- | --- | --- | --- |
| **redb** | MIT / Apache-2.0 | Rust, embedded | single-file B-tree KV, ACID | could, but should not | **right size for the spine** |
| **VeriSimDB** | MPL-2.0 | Rust, embeddable or service | octad: graph, vector, tensor, semantic, document, temporal, provenance, spatial | no (async) | **right model for the record** |
| **SurrealDB** | **BUSL-1.1** | Rust, embedded or server | SurrealQL: document + graph + vector + live queries | no | **blocked, then wrong shape** |
| **PoloDB** | Apache-2.0 | Rust, embedded | BSON documents, MongoDB-ish API, RocksDB under v5 | no | **query layer you do not need** |
| **CubDB** | Apache-2.0 | **Elixir only** | append-only B-tree KV, ACID, MVCC | n/a | **right shape, wrong language** |

### SurrealDB — blocked before it is discussed

`deny.toml` allows `BSL-1.0` (the Boost licence) and not `BUSL-1.1` (Business
Source). SurrealDB's core is BUSL-1.1: source-available, not OSI-approved,
converting to Apache-2.0 four years after each release (3.0: 2030-01-01). So
`cargo deny check licenses` fails on it today, and adopting it would mean an
explicit `deny.toml` exception — the same mechanism already used, narrowly, for
Creusot's two LGPL crates.

Its Additional Use Grant *would* permit embedding in a game. The blocker is
policy, not law.

Even with a licence exception, the shape is wrong. SurrealQL, live queries and a
distributed mode are answers to "many clients, one queryable store". IDApTIK has
one player, one building, and state it already has as structs. You would be
adding a query language between the game and its own data.

Where it *would* be right: a level browser, a telemetry warehouse, or anything
where a human writes a query against thousands of runs. That is a real future
need — and it is the need VeriSimDB is aimed at too, at a licence this estate
already uses everywhere.

### VeriSimDB — the strongest fit, with a maturity caveat

`hyperpolymath/verisimdb`: 50,370 lines of Rust across 18 crates, MPL-2.0, in
this estate already. Verified by reading the tree, not the README:

| Crate | Lines | What it gives IDApTIK |
| --- | --- | --- |
| `verisim-storage` | 1,852 | `StorageBackend` trait: opaque bytes, `scan_prefix`, atomic `batch_put`; **redb** backend behind a feature |
| `verisim-wal` | 1,985 | segmented WAL with CRC32, checkpoints, `replay_from(seq)`, `find_last_checkpoint`, segment pruning |
| `verisim-octad` | 5,488 | the entity, transactions with isolation levels, per-modality versioning, WAL replay, graceful shutdown |
| `verisim-temporal` | 1,321 | "state at version N" — save history, rewind, replay |
| `verisim-provenance` | 1,381 | why a state is what it is: command → event → state |
| `verisim-graph` | 1,237 | rooms, portals, circuits as relationships |
| `verisim-spatial` | 1,279 | floor-plan geometry |
| `verisim-nif` | 135 | rustler NIF bridge — the same store, called in-process from Elixir |
| `verisim-api` | 12,256 | axum server: `GET/PUT/DELETE /octads/{id}`, `/search/*`, `/drift/*`, GraphQL, `/.well-known/groove` |

Four facts make this the leading candidate, and they are not marketing:

1. **The licence is already the estate's.** MPL-2.0, same as everything else
   here. No `deny.toml` exception.
2. **The modalities match the domain.** A building has topology (graph),
   geometry (spatial), history (temporal) and lineage (provenance). The game is
   a *simulation*, and this is a database named for simulating identity across
   modalities.
3. **It already spans the Rust/Elixir seam.** `verisim-nif` exists so the BEAM
   can call the store in-process. IDApTIK has exactly that seam — burble behind
   `burble-client`, pinned at `ef6756c`.
4. **The estate already deploys it as a service.** `standards/.verisimdb/` ships
   `config.toml`, `compose.yml`, `Containerfile` and `deploy.sh` for an instance
   on port 8097 with per-instance octad schemas. An IDApTIK instance would be
   another directory, not a new integration.

**Skip these crates for a game.** `verisim-vector`, `verisim-tensor`,
`verisim-semantic`, `verisim-document` (Tantivy full-text), `verisim-drift`,
`verisim-normalizer`, `verisim-planner`, `verisim-repl`. They serve the
knowledge-base use case — consonance, drift, normalisation — which a game does
not have. `verisim-api` only if IDApTIK talks to it out of process.

**The caveats, stated plainly:**

- **Version 0.1.0, one author, 4 stars.** 50k lines is a lot of code written
  quickly, and I did not read its tests. Treat it as an estate project with
  real code, not as a battle-tested dependency.
- **Everything is `async`** (`tokio`, `async_trait`, `Send + Sync`). The tick is
  synchronous. Integration means a channel or actor at checkpoint boundaries,
  never `.await` inside `apply`.
- **Every crate is `publish = false`.** Consumption is a git dependency, which
  means a second `allow-git` entry in `deny.toml` beside burble's.
- **The octad model is per-entity.** A building becomes one octad per room,
  portal and circuit, not one octad per save. Fine at this scale; worth sizing
  before a 200-room building.

### PoloDB — not inadequate, just the wrong tool

PoloDB v5 is Apache-2.0, actively maintained, and needs no server. But v5
**replaced its own storage engine with RocksDB**, so it is no longer a
single-file embedded database — you take a C++ dependency to get a MongoDB-style
query layer you do not need. Indexes and aggregation are still marked alpha, and
it is a personal project.

The guess in the question was right, for a better reason than maturity: PoloDB
answers "I want documents and queries without a server". IDApTIK wants a log and
a KV store.

### CubDB — right shape, wrong runtime

CubDB is genuinely good: pure Elixir, append-only immutable B+tree, ACID, MVCC,
crash-safe, single file, Apache-2.0, supervised like any other process.

**It cannot be called from Rust at all.** For the game core that makes it not
inadequate but inapplicable — a different runtime, not a weaker option.

It becomes relevant to exactly one question: should *burble* keep local durable
state (lobby registry, session history, reconnect tokens)? CubDB would be a fine
answer there. But if the same data is needed by the Rust core, putting it in
CubDB creates a **database seam** — which `docs/SEAMS-SPEC.adoc` in the standards
repo classifies as `elixir-disciplined` by default, and which says explicitly
that *"VeriSimDB integration upgrades a database seam when invariants are
formally declared."* Two stores, one seam, no invariant is the version to avoid.

## Recommendation

Three tiers, adopted in this order. Each is useful on its own, and each is
smaller than the last one's failure.

**Tier 0 — the tick. Nothing changes.** `BuildingSim`, `InfrastructureSim` and
`GhostLobbySim` stay in memory, synchronous and pure. No database goes near them.

**Tier 1 — the spine. `redb` directly, behind an `idaptik-persist` crate.**
Three things: an append-only command log keyed `(session, seq)`; a checkpoint
snapshot every N commands, using `BuildingSnapshot` as-is; a session index. The
WAL question is settled by redb's own durability, and the snapshot is already
provably restorable. ~300 lines, MIT/Apache, no async, no C.

**Tier 2 — the record. VeriSimDB as a service on its own port, opt-in.**
Emit octads for what is genuinely cross-modal: a room that must be the same room
in the definition, the graph, the spatial map and the save; a run whose
provenance chain is command → event → state; a scenario whose authored and
compiled forms must not drift. That is the consonance problem VeriSimDB names,
and the one place a plain KV store would leave you hand-writing drift checks.

**Tier 3 — never, unless the game grows a query surface.** A document database.
If a level browser or telemetry warehouse ever ships, answer it with the VeriSimDB
instance already running rather than a second engine.

## What would change this answer

- **If sessions must be shared live between players** (rather than lockstep
  frames), the store moves into the authority path and the latency budget
  decides — that is a different design, not a database choice.
- **If save files must be portable and inspectable**, JSON on disk stays
  correct and redb becomes a cache. Do not adopt a store that makes a save
  opaque unless it buys something the game actually needs.
- **If VeriSimDB reaches 1.0 with the tests to match**, Tier 1 and Tier 2
  collapse into Tier 2 and there is one store.
- **If Guix is a hard requirement for the store itself**, only embedded crates
  qualify: Guix has no `surrealdb` package (checked `packages.guix.gnu.org`,
  2026-10-25), so SurrealDB-as-a-server would need a container outside the
  environment authority.

## What is not verified

- VeriSimDB's test suite, fuzzing and real-world durability. I read its code
  and manifests; I did not run it, and this sandbox cannot build it (crates.io
  is blocked here).
- Whether its WAL and redb layers actually hold under a game's checkpoint
  cadence. That needs a benchmark, not an argument.
- SurrealDB embedded's build size and cold-start, which I cite as "heavy"
  qualitatively.
- Whether burble needs local durable state at all. That is a burble question.


## What was actually built

Tier 1 shipped as `crates/idaptik-persist` (PR #171). Tiers 2 and 3 are
untouched: no VeriSimDB, no document store, no service.

Three parts of the plan above did not survive contact with the source, and the
corrections are worth recording because each was found by reading rather than by
assuming:

**1. The key is `seq`, not `tick`.** `GhostLobbySim::reset` sets `self.tick = 0`,
and `Command::Restart` calls it. Keying the log on `(session, tick)` would have
made a post-restart tick 5 silently overwrite the pre-restart tick 5, in both
the command table and the checkpoint table, with nothing to report it. The key
is a store-assigned `seq` that never rewinds; `tick` and `epoch` are payload.

**2. `held` had to be stored, and rebuilt through `fold`.** Buttons live in the
Bevy layer (`CommandQueue.held`), not in `RuntimeSnapshot`. A resume that
restored only the snapshot would come back with no keys held and diverge on the
first tick — while `verify`, replaying from `seq 0`, would still pass. So a
checkpoint records `held`, and `resume` advances it through
`idaptik_core::scenario::fold` rather than re-deciding which command touches
which button.

**3. `last_seq` is `Option<u64>`, not `u64`.** The first draft used `0` as
"empty", which made `seq 0` unwritable — the first batch could only be 1, and
the log then never started at zero. Its own `NonZeroStart` check caught this,
and a test failed on it before any of it shipped.

### What is proven

`cargo test -p idaptik-persist`: **18 passing** (5 unit, 13 integration),
including the two that matter most:

* `resume_restores_held_buttons` — a checkpoint taken with `Left|Sprint` held
  resumes with them held;
* `verify_can_fail` — a checkpoint whose recorded `held` is not what the log
  replays to is reported as `HeldMismatch`. A verification that cannot fail is
  not verification.

### What is not proven

* The crate has not been run against a live `idaptik-bevy` process; the plugin
  (PR B) does not exist yet.
* `BuildingSim` sessions can be written, listed and verified, but there is no
  typed `resume` for them — `Store::resume` returns `UnsupportedSimKind`.
  That arrives with the runtime that consumes the payload, not before.
* `MoletaireSim` is excluded for the reason given above: its snapshot carries no
  per-tick `dt`, so it is not replayable yet.
* The digest is FNV-1a 64. Identity and corruption detection, not a security
  property.
