# Playing the Evil Weevil guard brain

How to run the game on the kernel brain, what to look at, and how to tell the
kernel apart from the classic FSM. Read
`docs/dev-notes/weevil-local-setup.md` first if you have not built it yet.

## Run it

```sh
just weevil-run PROFILE="cautious-sentry" SEED="7"
```

which is:

```sh
IDAPTIK_WEEVIL_PROFILE=cautious-sentry IDAPTIK_WEEVIL_SEED=7 \
    cargo run -q -p idaptik-bevy --features weevil
```

On Linux you need Bevy's system libraries once: `just bevy-linux-deps`.

| Variable | Default | Meaning |
| --- | --- | --- |
| `IDAPTIK_WEEVIL_PROFILE` | `cautious-sentry` | Which guard profile drives the guard |
| `IDAPTIK_WEEVIL_SEED` | `7` | The kernel's RNG seed |

The three profiles are `cautious-sentry`, `aggressive-pursuer` and
`equipment-specialist`. They are the **same kernel** fed a different perception
policy and applied with different host limits — not three behaviour trees.

## The one control you need: `V`

Press **V** in the Ghost Lobby scene to flip the guard brain between the kernel
and the classic FSM. Same binary, same world, no rebuild.

That is the comparison and the rollback the seam exists for. `V` is free in both
keymaps — the infiltrator claims A/D/S/W/Shift/E/Q, the hacker 1–4/P/G/X.

If the kernel did not load, `V` still works and the sim falls through to the
classic FSM rather than leaving the guard standing still.

## The developer view

Top-right of the Ghost Lobby scene when the `weevil` feature is on. Four lines,
read straight off the adapter — nothing is re-derived for the panel:

```
weevil cautious-sentry v1.0.0
see: player#1(12) decoy#4(8)
want: ADVANCE t=1
host: applied/blocked-by-door
```

| Line | Meaning |
| --- | --- |
| `weevil <profile> v<version>` | The profile in use and the kernel's reported version. If it reads `guard: classic fsm`, the plugin did not load the kernel. |
| `see:` | What the guard perceived this tick, as `source#id(distance)`. `player#1` is the infiltrator, `decoy#4` the robot vacuum, `object:<id>#<id>` a tracked object. |
| `want:` | What the kernel proposed: the action and the target id it named. |
| `host:` | What the game did with it: `applied/…` or `refused/…` and the reason. |

If the adapter raised an error, it is appended to the last line as `ERR …`.

Note the one-tick lag: the panel is built from the perception the sim produced
*before* this tick, so a proposal appears one tick after the observation that
caused it. That is deliberate and is what makes the kernel a pure function of its
inputs.

## What to watch for

The eight behaviours the slice is required to show, and where each appears.

1. **The guard patrols.** With no contacts, `want: ADVANCE` and the guard walks
   its route (`patrol_x` in the profile). An empty route means "hold" by design.
2. **A contact changes behaviour.** `see: player#1(...)` and the guard turns
   toward it: `want: ATTACK t=1` when it is within the profile's engage distance,
   `ADVANCE` while it closes.
3. **Losing the contact triggers investigation.** Once the player is gone, the
   guard's *memory* — a bearing and an age, never a position — drives
   `want: INVESTIGATE`.
4. **The memory expires and it falls back.** After the ABI's 36-tick TTL with no
   new sighting, the guard goes back to `ADVANCE`. Memory expiry is a fallback,
   not a stuck state.
5. **The guard cannot pass a closed door.** `host: applied/blocked-by-door`, and
   the guard's position is unchanged. It presses against the wall rather than
   crossing it.
6. **An invalid target is refused.** `host: refused/invalid-target` and no
   movement. A target id the game does not own — a taken note, a thrown drive, an
   inactive vacuum — cannot move the guard.
7. **The speed limit is the profile's.** The guard moves at the profile's
   `max_speed_world`, not at the kernel's own proposal. The kernel's speed is
   advisory; the game clamps it in world units.
8. **The deception works.** `see: decoy#4(...)` is the robot vacuum — real to the
   guard's perception, a machine in the world's truth — and the guard will
   investigate where it went. The panel shows the `decoy` label and the
   confidence the game attached to it; the replay records both.

## Comparing the kernel against the classic FSM

Two ways, and they answer different questions.

**In one run** — press V and watch the same world with each brain. The panel
changes identity (`weevil …` vs the classic FSM's own behaviour), and the guard's
route, reactions and speed differ. This is the comparison you want while
developing.

**Across two runs** — the classic FSM is the default, so a build without the
feature is the rollback:

```sh
just weevil-run-classic     # no --features weevil: the game as it was
just weevil-run            # the kernel brain
```

The game's own suite is the check that the classic path is unchanged:

```sh
just weevil-core-test
```

That suite passing is the evidence that the seam is additive — not the structure
of the code.

## If there is no display

The headless replay drives the same kernel through a deterministic corridor host
and prints a decision summary. It is not the game, but it checks the same
properties without a window:

```sh
just weevil-replay TICKS="240" PROFILE="cautious-sentry" SEED="11"
```

Look for `blocked > 0`. That number is zero if the world→fx scale is wrong: the
guard then crawls at a sixteenth of its speed, never reaches the first closed
door, and is never blocked.

Write one and prove it reproduces:

```sh
just weevil-replay-out OUT="/tmp/weevil.json"
just weevil-replay-check FILE="/tmp/weevil.json"
```

The replay records, per tick: the kernel/ABI/profile versions, the initial context
and per-agent state, the seed and tick rate, the observation (with source, age
and confidence), the returned intent, and the host's accept/reject plus the
effect it applied. Two runs are compared only within a matching configuration —
a different seed, profile, kernel or tick rate is reported as a mismatch with the
reason, not silently ignored.

## The bounded player check

Run this when you want to confirm the kernel is actually driving, in about a
minute:

1. `just weevil-run PROFILE="cautious-sentry" SEED="7"`.
2. The top-right panel must read `weevil cautious-sentry v1.0.0`. If it reads
   `guard: classic fsm`, the kernel did not load — stop and run
   `just weevil-doctor`, then `just weevil-kernel-check`.
3. Press **V** once. The `want:` line starts changing on its own within a few
   seconds. That proves the kernel is driving, not the FSM.
4. Press **V** again. The guard returns to the classic FSM's behaviour.
5. Let the guard patrol, then walk into its view. Watch `see:` gain a `player#1`
   contact and `want:` change. Then break line of sight and watch `want:` become
   `INVESTIGATE`.

If step 3 does not happen, the brain is `Classic` — check that the `weevil`
feature was actually enabled (the `just` recipe passes it) and that the panel's
first line is not the classic-fsm message.

## What has not been verified

**The game has never been launched.** The authoring sandbox has no display and no
Rust toolchain, so nothing in this guide has been observed running: the adapter,
the core seam, the Bevy driver and the game are all uncompiled, and the behaviours
above are design claims supported by the code and its documentation — not
observed results. The kernel side *is* verified: it builds under Zig 0.16.0, its
suite reports 32/32, the C null host passes, and the ABI mirror matches the
generated header. See `docs/evidence/weevil-integration-evidence.md` for the
distinction in full, including the six adapter defects that were found by reading
the code and are fixed but uncompiled.
