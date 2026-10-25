# ADR-0003: Engine strategy — engine-agnostic core, Bevy + Fyrox as frontends

- Status: Accepted core boundary; dual-frontend evaluation superseded by ADR-0008
- Date: 2026-07-09

## Context

The brief names **both Fyrox and Bevy** as primary tools. They are competing
full Rust engines, so "use both" needs an architecture that does not fork the
game in two. At the same time, the README's north star is unambiguous: **Rust
owns gameplay truth.** The network simulation the hacker plays against — devices,
zones, routing, DNS, traceroute, power, trace timers, alerts — is game logic, not
rendering.

## Decision

1. **Keep game semantics in an engine-agnostic core crate** (`idaptik-core`)
   with no dependency on Bevy or Fyrox. This holds the network/topology model,
   simulation, movement rules, and state transitions. The process allowed to
   commit a session's simulation state is a separate topology-specific choice
   (ADR-0015); current Elixir/Burble wiring coordinates and relays sessions
   (ADR-0002). The core is `no_render` and unit-testable in isolation.
2. **Treat Bevy and Fyrox as interchangeable frontends** over that core, each in
   its own crate (`idaptik-bevy`, `idaptik-fyrox`), during an evaluation
   phase. Both are pulled by Cargo — they are crates, not toolchains (ADR-0001).
3. **Pick one at the end of the Envelope milestone**, recorded in a follow-up
   ADR, judged on: 2D + UI ergonomics (this is a 2D platformer with a lot of
   diegetic UI — terminals, network desktops, popups), editor/tooling value
   (Fyrox ships a scene editor; Bevy leans code-first), Wasm story, and iteration
   speed. Until then, whichever frontend is behind must not accrete game logic —
   logic belongs in core.

The FFI/ABI surface (`idaptik-ffi`, modelled in Idris2 under `abi/` — the
Zig adapter half of the ADR-0001 seam is still open, see issue #103) also
targets the core, not a specific engine.

## Consequences

- "Both engines" is honoured without maintaining two games: the frontends are
  thin, the core is the product.
- The eventual engine decision is cheap, because switching frontends does not
  touch gameplay truth or the multiplayer boundary.
- Workspace shape this implies: `crates/idaptik-core`, `crates/idaptik-bevy`
  and `crates/idaptik-ffi`, all scaffolded since this decision, plus
  **(intended)** `crates/idaptik-fyrox` — the Fyrox frontend, not yet
  scaffolded.
