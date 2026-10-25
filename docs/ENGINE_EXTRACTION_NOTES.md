# Engine extraction notes

A running log of what in IDApTIK is a candidate to lift into a standalone
runtime, where it is currently welded to this game, and what it would have to
become to stand alone.

The destination is **`metadatastician/enaction-engine`**, renamed from
`affective-engine`. IDApTIK is the proving ground: things get built here, used in anger, and only generalised once
they have taught us something. Nothing in this repo depends on that one, and
that is deliberate — see [Why there is no dependency](#why-there-is-no-dependency)
and ADR-0009, which states the conditions under which a dependency becomes
permissible.

Scope today: **fixed-timestep render interpolation** (`crates/idaptik-core/src/interp.rs`).
This document grows as further subsystems become extraction candidates.

**This document is also the extraction-trigger evidence log.** Enaction
ADR-0017 requires three things before code moves: a second caller whose want
was recorded first, bidirectional conformance fixtures, and *a recorded
surprise* — at least one documented case where the naive boundary was wrong.
The five coupling boundaries and two measured float results below are what a
recorded surprise looks like. A candidate with none has not been used in anger.

---

## 1. Render interpolation

### What it is

The simulation advances in whole ticks at `TICK_DT` (60 Hz, `scenario/mathf.rs`).
A display refreshes whenever it likes. Before this change the Bevy frontend read
live simulation state every frame, so on any display that is not exactly 60 Hz
each position was held for a varying number of frames and motion juddered.

`interp.rs` keeps tick *N* alongside tick *N+1* and draws between them, using a
fraction of the way through the current tick as the blend factor.

### Layering

| Layer | Location | Extraction status |
|---|---|---|
| `Pose`, `Blending`, `ChannelSpec`, `CHANNELS`, `Blend`, `lerp`, `DoubleBuffer` | `idaptik-core/src/interp.rs` | **Generic.** Knows nothing about IDApTIK. Lifts unchanged. |
| `VisualSlot`, `N_VISUAL_SLOTS`, `MAX_DOORS`, `poses_of`, `door_opens_of` | same file, below the divider | **Game-specific.** Must be replaced, not moved. |
| `VisualBuffers` resource, commit-on-tick | `idaptik-bevy/src/driver.rs` | **Host-specific.** Bevy `Resource` and `FixedUpdate`. |
| `render_alpha`, the sampling calls | `idaptik-bevy/src/scene.rs` | **Host-specific.** Reads Bevy's `Time<Fixed>`. |

The generic half is deliberately at the crate root rather than under
`scenario/`, so the seam is visible in the module tree rather than only in prose.

### Coupling boundaries, and how to sever each

**1. `VisualSlot` — a hardcoded enum of this game's actors.**
`Player, Billy, Note, Usb, Vacuum`, with `N_VISUAL_SLOTS = 5`. A generic engine
cannot know these. *Sever by:* replacing the enum with an opaque handle issued
by the buffer at registration time (`fn register(&mut self) -> SlotId`), or by
making the buffer generic over an index type that the host supplies. The enum
buys compile-time bounds-checking and zero indirection, which is the right
trade *here* and the wrong one for a library.

**2. `MAX_DOORS` — a fixed ceiling picked for one scenario.**
Ghost Lobby has 4 doors; the constant is 16. Fixed size is what makes the
zero-allocation guarantee true, so the ceiling cannot simply be removed. *Sever
by:* making the const generic parameter the caller's choice
(`DoubleBuffer<f64, N>` already is — only the `DoorBuffer` alias pins it), and
letting the host declare its own capacity at construction.

**3. `poses_of` / `door_opens_of` — they read `RuntimeState` directly.**
This is the tightest coupling: these functions name `state.player.x`,
`state.billy.facing`, `state.doors[..].open`. *Sever by:* inverting it. The
engine should never read game state; the host should push poses in. The
signature becomes the host's problem:
`buffer.commit(&host.collect_poses())`. In this repo the coupling is worth it —
it keeps the frontends thin, per ADR-0003, and lets the TUI adopt the same
buffers later without duplicating the extraction logic.

**4. Bevy's `Time<Fixed>` supplies alpha.**
`scene::render_alpha` is a one-line wrapper over `overstep_fraction_f64()`,
written as a named function precisely so this seam is a single edit. *Sever by:*
the engine owning its own accumulator (see below). The wrapper is the only place
render timing enters the scene.

**5. The discontinuity signal is `Event::Restarted`.**
`driver::step_sim` scans the tick's events for a restart and passes
`discontinuous: true` into `VisualBuffers::commit`. That event is IDApTIK's.
*Sever by:* generalising to a host-raised discontinuity flag — "this tick was
not travelled through" — which covers restarts, teleports, level loads and
network resyncs identically. `idaptik-net`'s `net:resync` snapshot hand-off is
the second caller that will want it, and is the case that will prove the
generalisation.

### What the engine needs that IDApTIK does not

**A `FixedStep` accumulator.** IDApTIK gets this free from Bevy: `Time<Fixed>`
runs the accumulator and `FixedUpdate` runs the steps. A standalone engine has
no host to borrow it from and must own:

```
advance(real_dt) -> iterations   // how many sim steps to run this frame
alpha() -> f64                   // leftover fraction, for sampling
```

with the usual spiral-of-death guard (a cap on iterations per call, so a long
stall does not try to catch up forever and fall further behind). This is the one
piece that genuinely cannot be copied out of this repo, because it does not
exist here.

That asymmetry is the reason `enaction-engine`'s crate is not a mirror of
`interp.rs`, and it is worth stating plainly: **the extraction is not a copy.**

---

## 2. Design decisions and trade-offs

**The channel table is data, not branching.** `CHANNELS` declares which visual
quantities blend and how (`Lerp` / `Snap`). It is inspectable, and a test asserts
the implementation matches it, so it cannot rot into decoration. This mirrors how
the rest of the estate treats contracts (Nickel config and K9 contractiles). The
trade: the table is currently *checked against* the code rather than *driving*
it — a generic engine would dispatch on it. Making it drive dispatch here would
cost a branch per channel per frame to buy flexibility nothing needs yet.

**Only continuous quantities are interpolated.** Discrete state — `hidden`,
`crouching`, `BillyMode`, visibility, colour — is read live from the current
tick. Blending a boolean is meaningless; blending an enum is nonsense. This is
not an optimisation, it is a correctness boundary, and it is the first thing to
restate when generalising.

**`facing` snaps rather than lerps.** It is a sign, not a position. Lerping it
passes through zero, which draws the actor facing neither way mid-turn.

**Two floating-point results decided the shape of `lerp`,** and both were
measured rather than reasoned about:

- The endpoints are *clamped*, not computed. `prev + (curr - prev) * 1.0` is not
  exact — `(-5.5, 3.3)` gives `3.3000000000000007`, `(1e16, 1.0)` gives `0.0`.
  An inexact endpoint pops once per tick, which is the artefact interpolation
  exists to remove.
- The interior uses the *one-term* form. The symmetric-looking
  `prev * (1 - alpha) + curr * alpha` is worse: with `prev == curr == 42.0` and
  `alpha = 0.1` it returns `42.00000000000001`, so a stationary object shimmers
  every frame. The one-term form is exactly stationary.

Both are pinned by tests that fail if someone "simplifies" them back.

**`commit` then `snap`, never `snap` alone.** On a discontinuity the fresh state
still has to enter the buffer; `snap` only collapses `prev` into `curr`. Snapping
*instead of* committing leaves the new state out entirely and keeps drawing the
pre-restart position for another interval before jumping. Both halves of this
are pinned: `snap_alone_would_lose_the_fresh_state` documents the wrong version,
and `a_restart_does_not_render_as_a_slide_back_to_spawn` drives the right one
through the real Bevy driver.

**The TUI is deliberately excluded.** `idaptik-tui` renders to terminal cells
and has no sub-tick spatial resolution to interpolate into. Leaving it reading
live state is correct, not an omission.

**Zero allocation is asserted, not asserted-to.** `buffers_are_inline_fixed_size_storage`
checks `size_of::<PoseBuffer>() == 2 * N * size_of::<Pose>()`, so the claim
fails a test rather than living in a comment.

---

## 3. Why there is no dependency

`IDApTIK` does **not** depend on `enaction-engine`, and should not yet.
ADR-0009 now states this as a decision with exit conditions rather than as an
absence.

A cross-repo build dependency now would freeze the interface before the
prototype has taught us anything — and the whole premise of the proving ground
is that we do not know the right shape yet. The five coupling boundaries above
are hypotheses about what generalisation will need; at least one of them is
probably wrong, and finding out is cheaper while the two trees are independent.

The cost is a real one and worth naming: the generic half of `interp.rs` and its
counterpart in `enaction-engine` will drift. That is accepted for now. The
trigger to reconsider is a **second** host wanting the same buffers — the TUI,
another graphical frontend, or `idaptik-net`'s resync path. At that point the duplication
starts costing more than the coupling would, and this section should be revised
rather than quietly ignored.

---

*Started 2026-07-22 alongside the render-interpolation change.*
