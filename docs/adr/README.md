# Architecture Decision Records

Decisions for the Rust era of IDApTIK are recorded here as ADRs, rather than
asserted in the README and quietly contradicted later.

| # | Decision | Status |
|---|----------|--------|
| [0001](0001-toolchain-and-runtime-management.md) | Toolchain & runtime management (mise + rustup; Idris2 via pack; opsm later) | Accepted |
| [0002](0002-multiplayer-transport.md) | Multiplayer transport: Bandit + Phoenix Channels, not LiveView | Accepted |
| [0003](0003-engine-strategy.md) | Engine strategy: engine-agnostic core, Bevy + Fyrox evaluation | Superseded in part by ADR-0008 |
| [0004](0004-deterministic-event-sourced-scenario-sim.md) | Deterministic, event-sourced, definition-as-data scenario simulation | Accepted (Envelope milestone) |
| [0005](0005-session-relay-topology.md) | Session relay topology: relay-only lockstep over typed Command/Event | Accepted (vertical-slice milestone) |
| [0006](0006-gossamer-host-and-burble-transport.md) | gossamer host window + burble transport: two-player windowed slice | Accepted (design; implementation deferred) |
| [0007](0007-versioned-ums-package-boundary.md) | IDApTIK owns the versioned UMS package boundary | Accepted |
| [0008](0008-select-bevy-and-retire-fyrox.md) | Select Bevy and retire the Fyrox evaluation frontend | Accepted |
| [0009](0009-enaction-kernel-dependency-direction.md) | Enaction kernel dependency direction; IDApTIK publishes `idaptik/esm/v1` | Accepted |
| [0010](0010-cac-kernel-grown-in-idaptik-core.md) | The CAC kernel is grown in `idaptik-core`, not extracted yet | Accepted |
| [0011](0011-cognitive-model-documented-in-the-open.md) | Cognitive-model work is documented in the open | Accepted |
| [0012](0012-signal-network.md) | A signal network for re-wiring the building lives in `idaptik-core` | Proposed |
| [0013-water](0013-water-gas-airflow.md) | Water, gas and airflow are three coarse building systems, not a fluid solver | Proposed |
| [0013-electrical](0013-electrical-infrastructure.md) | Electrical infrastructure in `idaptik-core` | Proposed |
| [0013-evil-weevil](0013-evil-weevil-adversarial-testing-adapter.md) | Evil Weevil deterministic adversarial testing adapter | Proposed |
| [0014](0014-infrastructure-consequences-as-observations.md) | Infrastructure consequences are canonical-sim facts that reach actors only as observations | Proposed |
| [0015](0015-deployment-topologies-and-state-ownership.md) | Deployment topologies, simulation authority, and state ownership | Accepted (contract; runtime partial) |

New ADRs: copy the format of an existing one, take the next number, and add a row
above.
