# Evil Weevil guard brain — developer instructions

IDApTIK's guard can now be driven by the Evil Weevil kernel. This note is the
short version: what the seam is, how to build it, how to run the headless
replay, how to run the game on the kernel, and what to check.

The architectural boundary is unchanged and is the thing to keep intact:

* **The game** owns world truth, physics, navigation, collision, guard identity
  and entity lifecycle, perception generation, and authoritative action
  validation and application.
* **The kernel** owns per-agent memory and decision state, and proposes intents.
  It never moves the guard.

## Where the code lives

| Path | What it is |
| --- | --- |
| `crates/idaptik-weevil/` | The native adapter: ABI mirror, fx maths, errors, observation/intent conversion, profile, host, replay, and a headless corridor world. |
| `crates/idaptik-weevil/src/epistemic.rs` | The guard's beliefs as an `enaction-runner-output/1` document (`--emit-epistemic`). |
| `crates/idaptik-weevil/src/mind_trace.rs` | The kernel's ABI 1.1 trace records as an `enaction-mind-trace/1` document (`--emit-mind-trace`), with the 1.2 vocabulary for a model run. |
| `crates/idaptik-weevil/src/canonical.rs` | RFC 8785 canonical bytes and SHA-256 for both documents. |
| `crates/idaptik-weevil/tests/fixtures/enaction/` | The committed documents of twelve runs (22 files), pinned to the driver by `tests/golden.rs`. |
| `tools/verify_abi_mirror.py` | The ABI mirror check: `abi.rs` against the generated `ee.h`, every field, offset and size. |
| `crates/idaptik-core/src/scenario/guard_brain.rs` | The game's own seam: plain-data perception and decision types, no dependency on the kernel. |
| `crates/idaptik-core/src/scenario/sim/mod.rs` | `guard_perception`, `is_live_target`, `apply_guard_decision`, `GuardBrainKind` selection. |
| `crates/idaptik-bevy/src/guard_weevil.rs` | The Bevy driver: owns the `WeevilHost`, converts perception → observation, proposal → decision. |
| `crates/idaptik-bevy/src/hud.rs` | The guard-brain developer view. |
| `docs/standards/deed/` | The DEED conformance document and report. |

## 0. Toolchain

Two toolchains are needed: **Zig** (to build the kernel) and **Rust** (to build
the adapter and the game). Install both so every user gets them.

### Zig (system-wide)

Match the version the project's CI pins — `.github/workflows/seam.yml` keys its
cache on `zig0.16.0`, and newer Zig releases are stricter about source style and
will reject the kernel's test sources. The simplest route that needs no access
to `ziglang.org` is the PyPI wheel, which is the full compiler:

```sh
python3 -m pip install --no-cache-dir --target /tmp/zig16 ziglang==0.16.0
sudo mkdir -p /opt/zig
sudo cp -a /tmp/zig16/ziglang /opt/zig/
sudo ln -sf /opt/zig/ziglang/zig /usr/local/bin/zig
echo 'export PATH="/opt/zig/ziglang:$PATH"' | sudo tee /etc/profile.d/zig.sh
sudo chmod -R a+rX /opt/zig

# check it as an unprivileged user
sudo -u nobody bash -lc 'command -v zig && zig version'
```

Or, if you have normal internet access, the upstream tarball is simpler:

```sh
sudo curl -fsSL -o /tmp/zig.tar.xz \
  https://ziglang.org/download/0.16.0/zig-linux-x86_64-0.16.0.tar.xz
sudo mkdir -p /opt/zig-upstream
sudo tar -xJf /tmp/zig.tar.xz -C /opt/zig-upstream --strip-components=1
sudo ln -sf /opt/zig-upstream/zig /usr/local/bin/zig
echo 'export PATH="/opt/zig-upstream:$PATH"' | sudo tee /etc/profile.d/zig.sh
sudo chmod -R a+rX /opt/zig-upstream
```

### Rust (system-wide)

`rustup` is the supported route, and edition 2024 (used by this workspace)
needs a recent toolchain:

```sh
sudo curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sudo sh -s -- -y \
  --default-toolchain stable --profile minimal
sudo ln -sf "$HOME/.cargo/bin/cargo" /usr/local/bin/cargo
sudo ln -sf "$HOME/.cargo/bin/rustc" /usr/local/bin/rustc
echo 'export PATH="$HOME/.cargo/bin:$PATH"' | sudo tee /etc/profile.d/rust.sh
```

`cargo` and `rustc` need to be on `PATH` for the build; symlinking them into
`/usr/local/bin` makes them work for every user without each one editing their
own profile.

```sh
cargo --version && rustc --version
```

## 1. Build the kernel

The adapter mirrors **ABI 1.2** (evil-weevil ADR-0014, "it models you", on top
of ADR-0013's mind), so it needs evil-weevil pull request 11
(`marina/weevil-c1-mind-slice-2`, `f7c333d`) until that merges. A 1.1 kernel
does not export `ee_model_fingerprint` and the adapter will not link against
it; a 1.0 kernel does not export `ee_tick_mind` either.

Evil Weevil is a Zig + Idris2 project. From a checkout of
`catachresis-colloquium/evil-weevil`:

```sh
just abi-check        # proves include/evil_weevil/ee.h is what the model generates
just kernel-build     # builds src/interface/ffi/zig-out/lib/libevil_weevil.a
just kernel-host-test # the C null host, from the other side of the ABI
just seam-check       # all of the above, plus the negative gates
```

`just abi-check` is the one worth running first. The header is generated and
committed; if it has been hand-edited, or if the model changed without a
regeneration, that command fails. The adapter transcribes the *generated*
header, so a drift there is a build-time problem in the adapter too.

Without `just`, `bash scripts/weevil_kernel.sh build` from this repository
builds the same library; `scripts/weevil_kernel.sh locate` prints the checkout it
found.

## 2. Build the adapter

From the IDApTIK workspace root:

```sh
cargo build -p idaptik-weevil
```

`crates/idaptik-weevil/build.rs` finds the library in this order:

1. `EVIL_WEEVIL_LIB_DIR`
2. `$EVIL_WEEVIL_ROOT/src/interface/ffi/zig-out/lib`
3. `../../evil-weevil/src/interface/ffi/zig-out/lib` (relative to the workspace)

It deliberately **does not stub or skip** when the library is missing. A link
error is the correct outcome: silently falling back would mean shipping an
adapter that looks built and is not.

Then check the Rust mirror against the header the library was built from:

```sh
python3 tools/verify_abi_mirror.py \
    "$(bash scripts/weevil_kernel.sh locate)/include/evil_weevil/ee.h" \
    crates/idaptik-weevil/src/abi.rs
```

It lays out all ten structs (the seven 1.0 ones, `ee_mind`, `ee_mind_trace` and
the 1.2 `ee_model_trace`) in both files and fails on any field, offset or size
that differs. `abi.rs` also carries a compile-time assert for every 1.1 and 1.2
size and offset, so a moved field fails the Rust build as well.

## 3. Run the headless replay

No window, no game, no server — just the kernel, the adapter and a recorded
decision sequence:

```sh
cargo run -p idaptik-weevil --bin idaptik-weevil-replay -- \
    --ticks 200 --profile cautious-sentry --seed 7

# write one, then check a later run against it
cargo run -p idaptik-weevil --bin idaptik-weevil-replay -- \
    --ticks 200 --profile aggressive-pursuer --seed 7 --out /tmp/a.json
cargo run -p idaptik-weevil --bin idaptik-weevil-replay -- \
    --ticks 200 --profile aggressive-pursuer --seed 7 --check /tmp/a.json
```

The driver runs the trace twice and refuses to report success unless the two
are bit-identical, so a non-reproducible run is a hard failure, not a footnote.

The replay JSON carries the kernel/ABI/profile versions, the initial context and
per-agent state, the seed and tick rate, and one entry per tick with the
observation, the agent state before the tick, the returned intent, the host's
acceptance, and the applied effect.

### 3a. The Enaction documents

The driver can write the run as data for the Enaction Engine's validator. No
`enaction-*` crate is a dependency (ADR-0009); the wire shape, the canonical
bytes and the digest are written in this crate and checked by the validator.

```sh
# the guard's beliefs, through ee_tick
cargo run -p idaptik-weevil --bin idaptik-weevil-replay -- \
    --ticks 200 --profile decoy-lure --seed 7 --emit-epistemic /tmp/runner.json

# beliefs and mind, through the ABI 1.1 ee_tick_mind: both documents, one run
cargo run -p idaptik-weevil --bin idaptik-weevil-replay -- \
    --ticks 200 --profile the-grudge --seed 7 \
    --emit-epistemic /tmp/runner.json --emit-mind-trace /tmp/mind.json

# check them with enaction-validate, built from catachresis-colloquium/enaction-engine
enaction-validate --policies crates/idaptik-weevil/tests/fixtures/enaction-policies.json \
    --mind-trace /tmp/mind.json < /tmp/runner.json
```

Both files are written only after the driver's two runs agree, and both are
RFC 8785 canonical bytes, so two runs in separate processes give identical
files.

* **`--emit-epistemic`** writes one `enaction-runner-output/1` document for the
  guard, `guard-0`. Each tick on which the kernel acts from memory
  (`INTENT_FLAG_FROM_MEMORY`) becomes one belief, `contact-<id> in room-<r> at
  tick <t>`, citing the last observation of that contact. World truth is taken
  from the corridor's own state and is closed over every proposition the
  document states: a stated proposition missing from `world_truth` is false.
  That is how a reader sees the guard being wrong, as with the decoy in
  `decoy-lure` from tick 31.
* **`--emit-mind-trace`** needs `--emit-epistemic` (alone it exits 2), because
  the mind trace cites events of the runner output written beside it. It runs
  the guard through `ee_tick_mind` and writes one `enaction-mind-trace/1` record
  per tick: the affect token, its attention and `caused_by`, the ranking of
  candidates with the incumbent and the selected one, the target and the
  tells. There is no score and no reservoir value in it. Commitment override is
  not asserted; a reader derives it from `ranking`, `incumbent` and `selected`.
* Each cause the kernel reports for an affect change becomes an event in the
  runner output: a sighting for a close contact, an `action_outcome` for damage
  (attributed to the contact the guard perceives hitting it), an observation
  for a lead that came to nothing, and the guard's own perception for decay.

A mind run records the scenario id `headless-corridor-mind`, so it never
replays as equal to a plain run of the same profile.

The documents of eight runs are committed under
`crates/idaptik-weevil/tests/fixtures/enaction/`, with a `SHA256SUMS` and a
README naming the kernel and validator commits. `tests/golden.rs` fails if the
built binary no longer writes exactly those bytes; regenerate them with the
README's commands when the change is intended.

## 4. Run the game on the kernel

The kernel brain is behind the `weevil` feature, so a normal build is
unaffected:

```sh
cargo run -p idaptik-bevy                       # classic FSM, as before
cargo run -p idaptik-bevy --features weevil     # kernel brain
```

Environment:

| Variable | Default | Meaning |
| --- | --- | --- |
| `IDAPTIK_WEEVIL_PROFILE` | `cautious-sentry` | Which guard profile to load. |
| `IDAPTIK_WEEVIL_SEED` | `7` | The kernel's RNG seed. |

If the native library is not built, the plugin degrades to the classic FSM and
records why in the developer view rather than failing the app.

## 5. Compare and roll back

`GuardBrainKind::{Classic, Kernel}` is selectable at run time through
`GhostLobbySim::set_guard_brain`, and the Bevy driver reads it each tick. The
default is `Classic`; switching does not touch any world state. So the same
binary can be flipped between the kernel and the FSM without a rebuild, which is
what makes the comparison and the rollback real rather than ceremonial.

## 6. Profiles

Eight profiles ship. The three canonical guards are `cautious-sentry`,
`aggressive-pursuer` and `equipment-specialist`. They differ **only** through
seams the architecture assigns:

* the `capabilities` mask — the kernel's documented degradation ladder downgrades
  `ADVANCE` to `HOLD` when `CAP_NAVMESH` is absent;
* `HostTuning` — the world→fx scale, the movement limit, the engage distance,
  patrol waypoints and the object-reporting perception policy.

`budget_units` and `investigate_dist_world` are **declared and validated but have
no consumer in v0**. The kernel's `ee_init_desc` has no budget field, so `ee_init`
overwrites the context's allowance with a hard-coded 16 while the kernel's own
`workUnits` is `5 + contact_count` (13 at the ABI's eight-contact maximum) — so
`INTENT_FLAG_DEGRADED` cannot fire and a profile's `budget_units` changes nothing
observable. Read the effective allowance back with `WeevilHost::budget_units()`.
Both fields are carried so a profile written against the future minor-version
bump is already well-formed; the claim is stated rather than implied.

The kernel's policy weights are **not** profile fields (ADR-0010). Profiles do
not retune the kernel's numbers.

The other eleven stage scenarios in the headless corridor and are not read by
the game:

| Profile | What it stages | Measured, 200 ticks, seed 7 |
| --- | --- | --- |
| `decoy-lure` | The sentry with a decoy (id 200) in its room for 30 ticks. | First false belief at tick 31. |
| `the-grudge` | Evil Weevil's fixture: A (21) hits the guard on ticks 10 to 12 and leaves; B (22), nearer, arrives at tick 20. | Commitment override on 37 ticks, 20 to 56. |
| `the-grudge-control` | The same cast without `CAP_DAMAGE_EVENTS`. | No override. |
| `the-flinch` | C (31) close by on ticks 10 to 19. | One startle, at tick 10. |
| `the-flinch-control` | C at 1.5 fx instead. | No startle. |
| `the-stalker` | Slice 2: contact 41 at 3.0 fx, never observes the guard; the guard closes on it. | Stalks ticks 1 to 160; first engage at tick 161, at 0.5 fx. |
| `the-stalker-control` | The same without `CAP_OBSERVATION_CUES`. | First engage at tick 65, at 1.0 fx. |
| `caught` | One glance, at tick 101, while the guard stalks. | One `caught` tell, at 101; never stalks again; engages at 116; exposed and not known on 0 ticks. |
| `caught-control` | The same without the capability. | No `caught`; its mind trace is the stalker control's, byte for byte. |
| `false-exposure` | One glance, at tick 21; the contact then looks away and keeps its distance. | The contact knows on 21 to 51. Exposed and not known on 60 ticks, 52 to 111; expires at 112. |
| `false-exposure-control` | The same without the capability. | 0 such ticks. |

The `the-*` profiles carry a `CorridorScript` (`HostTuning::script`): contacts
present for a window of ticks, standing still, that may hit the guard. The cast
replaces the corridor's default player and decoy, and is in the world's truth
while present. The guard holds at x = 50 and each contact stands at the
fixture's fx distance at the corridor's scale (8 world units to 1.0 fx). The
override and startle figures are the kernel fixtures' own.

The six slice 2 profiles reproduce evil-weevil ADR-0014's measured figures
exactly, one tick later: corridor ticks count from 1 and the fixtures' from 0.
Their script has two more switches. `cues` runs a mind run through the 1.2
model record (`WeevilHost::tick_model`) and sets `EE_CONTACT_FLAG_OBSERVES` on a
contact's `observes` ticks, for a contact the guard perceives; each control
sets the same flag and withholds only `CAP_OBSERVATION_CUES`, as the kernel's
own controls do. `approach` is the fixtures' world rule: while the guard
advances or stalks it closes on its target by the proposed speed over 32, and
it moves on no other action. `false-exposure` has no `approach`, because in the
fixture the range does not close.

## 7. The developer view

Top-right of the Ghost Lobby scene when the `weevil` feature is on. It shows,
read straight off the adapter:

* the profile and kernel/ABI versions;
* what the guard perceived this tick (with source and distance);
* what the kernel proposed;
* what the host did with it — `applied/…` or `refused/…` and the reason;
* any adapter error.

## 8. The controlled deception

The robot vacuum is exposed to the guard as a contact with source `"decoy"`,
while the world's truth about it is that it is a machine doing its rounds. The
adapter carries `source`, `age_ticks` and `confidence` in its own schema and in
the replay; they never cross the C ABI, because the kernel has no field for
them. Perception and truth are therefore recorded separately and can be
compared after the fact.

## 9. The mind (ABI 1.1)

`WeevilHost::tick_mind(obs, damage_source)` is the 1.1 entrance beside `tick`.
The host owns one zeroed `EeMind` per agent and resets it with the agent; the
mind's fingerprint (`MIND_FINGERPRINT`, 1047490) is checked on every call,
because `ee_init` checks only the 1.0 one (18276775, unchanged). The host may
attribute the tick's damage only to a contact the guard perceives this tick;
any other id is refused. The kernel's trace is decoded into a range-checked
`MindRecord`.

`ee_tick` is unchanged by 1.1: on a 1.1 kernel the 1.0 profiles record the same
entries as on 1.0. Their replays differ only in `header.kernel_version`, which
`decision_identity` includes on purpose, so a 1.0 replay does not `--check`
against a 1.1 build.

The Bevy driver still calls `tick` only; the game does not run the mind yet.

## 9a. The model (ABI 1.2)

`WeevilHost::tick_model(obs, damage_source, observers)` passes the 40-byte
`ee_model_trace` through the unchanged `ee_tick_mind` and returns the kernel's
exposure ("a target knows where I am"), its value as the tick opened, its
cause (`CUE`, `OWN_ATTACK`, `EXPIRY`, or `NONE` exactly when it did not
change) and the contact behind it. The model fingerprint (69857) is checked on
every call. The new mirror items are `CAP_OBSERVATION_CUES` (bit 10),
`CONTACT_FLAG_OBSERVES` (2), `AGENT_FLAG_EXPOSED` (16) and the lifetime bits
8..15 of `ee_agent.flags`, `AGENT_MODE_STALK` (5, `TAKE_COVER`),
`INTENT_FLAG_CAUGHT` (128) and `TELL_CAUGHT` (8). `tick` and `tick_mind` are
unchanged, so every earlier run is unchanged too: the ten earlier documents
differ from their 1.1 bytes in the kernel label only.

The profile validator refuses any capability bit the mirror does not define,
so no profile can declare bit 10 by accident, nor `0xFFFF_FFFF` (ADR-0014's
"a host that declares every bit declares the future").

**What a contact knows** is the world's rule, in `world.rs`, used everywhere
(the owner's ruling of 2026-11-15, "Contacts remember"):

> A scripted contact has `memory`, a count of ticks that the designer sets for
> each contact (default 0). The contact knows where the guard is on tick `t`
> exactly when a tick `s` exists with `t - memory <= s <= t` on which it
> observed the guard, or the guard's attack on it was applied.

With `memory` 0 a contact knows only on a tick of evidence. A later
observation or attack starts the window again. The rule is
`world::knows_within`; the world evaluates it once per tick and records the
answer in `TickTruth::knowers`, which `world::knows` reads. Integers only. The
kernel's belief never enters the rule, so the two can differ, and
`tests/models.rs` counts where they do. The kernel does not learn a contact's
memory and receives no new input: the cue is still set only on the ticks of
the `observes` schedule.

The six model scenarios give contact 41 one memory, 30 ticks
(`profile::MODEL_CONTACT_MEMORY`, half a second at 60 Hz). It meets both
conditions of the scenarios:

* In `caught` the contact glances at 101 and the guard's blows land from 116,
  15 ticks later, so the contact still knows when they begin. The kernel is
  exposed to a contact that does not know on 0 ticks. Without the memory it
  was 14 ticks, 102 to 115, and the guard's model read as stale.
* In `false-exposure` the contact glances at 21, knows on 21 to 51, and
  forgets on 52. The kernel holds exposure until it expires at 112, so it is
  exposed to a contact that does not know on 60 ticks, 52 to 111. The
  control gives 0.

Read as CAC-KERNEL section 9 reads it, `caught` has two models and neither is
stale-divergent: belief 1 (`rejected`, tick 1) agrees until 101, and belief 3
(`certain`, tick 101) agrees to the end. `false-exposure` has three, and
exactly one is stale-divergent: belief 3 (`certain`, tick 21) agrees on 21 to
51 and diverges from 52 until 112. `tests/model_timelines.rs` derives these
timelines from the contract text.

A model run's runner output states four separate facts about one proposition
without a tick, `guard-0 in room-0`:

* the world: `world_truth`, closed over every order-0 proposition and event,
  with no atom for a model;
* the contact's own belief: order 0, `bearer` `contact-41`, `certain` from the
  tick it comes to know and superseded by `unknown` from the tick it stops,
  which is the first tick after its memory ends (52 in `false-exposure`). That
  `unknown` belief cites `contact-41 does not observe guard-0 at tick T`, the
  contact's perception on that tick, as the guard's expiry cites the guard's
  perception;
* the guard's model of it: `holders = ["guard-0"]`, the same bearer and
  proposition, `certain` while exposure is true and `rejected` while it is
  false, stated on the first claim and then only when exposure changes, citing
  `contact-41 observes guard-0 at tick T` (cue), `guard-0 attacked contact-41
  at tick T` (own attack) or the guard's perception that tick (expiry, as for
  decay);
* whether the guard is right: derived by the reader (CAC-KERNEL §9), never
  asserted.

Check a model run under the depth-1 policies:

```sh
enaction-validate --policies crates/idaptik-weevil/tests/fixtures/enaction-policies-models.json \
    --mind-trace /tmp/mind.json < /tmp/runner.json
```

Under the depth-0 `enaction-policies.json` a scenario's runner output is
refused as `nesting-bounded`.

## 10. Known limitations

* The ABI bearing is a `u16` the kernel zero-extends to `i32`. A **signed**
  corridor direction is not representable through it. The adapter applies its
  own navigation and treats the kernel's move vector as advisory.
* `ContactSource`, `confidence` and `age_ticks` live in the adapter's schema, not
  in the kernel.
* The kernel's memory is a bearing plus an age (TTL 36 ticks). It is **not** a
  position. `INVESTIGATE` is resolved against the host's own last-known
  position, never against the kernel's memory.
  In the corridor the remembered bearing reads `(0, 0)`, because of the `u16`
  truncation above, so a belief's room comes from the observation it cites,
  never from the kernel's memory.
* When affect calms by decay, the kernel reports no input. The mind trace
  still has to name a cause, so it cites the guard's perception on that tick.
  Enaction ADR-0027 records this as a known limit of version 1.
* The scripted contacts report `ContactSource::Player`, the closest term the
  closed vocabulary has for a person who is not the infiltrator.
* Native pointer checks are not memory isolation.
* The kernel's exposure is about "a target", not one target (ADR-0014 leaves a
  per-target model to the owner). The corridor's model scenarios have one
  contact, so the bearer of every model is that contact; an expiry names no
  contact and keeps the bearer of the model it ends.
* The contact's position proposition has no tick, so a model run whose guard
  changes room is refused rather than given a truth atom it cannot decide.
