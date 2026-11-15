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
| `crates/idaptik-weevil/src/mind_trace.rs` | The kernel's ABI 1.1 trace records (and, for a model scenario, the 1.2 vocabulary) as an `enaction-mind-trace/1` document (`--emit-mind-trace`). |
| `crates/idaptik-weevil/src/canonical.rs` | RFC 8785 canonical bytes and SHA-256 for both documents. |
| `crates/idaptik-weevil/tests/fixtures/enaction/` | The committed documents of twelve runs, pinned to the driver by `tests/golden.rs`. |
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

The adapter mirrors **ABI 1.2** (evil-weevil ADR-0013, the mind, and ADR-0014,
the model of the target's knowledge), so it needs evil-weevil at `1f68271`
(draft pull request 11) or later. A 1.0 kernel does not export `ee_tick_mind`,
a 1.1 kernel does not export `ee_model_fingerprint`, and the adapter will not
link against either.

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
the 1.2 `ee_model_trace`)
in both files and fails on any field, offset or size that differs. `abi.rs` also
carries a compile-time assert for every 1.1 and 1.2 size and offset, so a moved field
fails the Rust build as well.

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

# a model scenario (ABI 1.2): its runner output holds order-1 beliefs, so it is
# checked under the depth-1 policies; under the depth-0 file it is refused as
# nesting-bounded
cargo run -p idaptik-weevil --bin idaptik-weevil-replay -- \
    --ticks 200 --profile false-exposure --seed 7 \
    --emit-epistemic /tmp/runner.json --emit-mind-trace /tmp/mind.json
enaction-validate --policies crates/idaptik-weevil/tests/fixtures/enaction-policies-depth-1.json \
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

* For a model scenario (section 9a) the runner output also holds the
  contact's own belief about where the guard is and the guard's model of it,
  and the mind trace declares `stalk` and `caught`.

A mind run records the scenario id `headless-corridor-mind`, so it never
replays as equal to a plain run of the same profile.
A model run records `headless-corridor-model`.

The documents of twelve runs are committed under
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

Fourteen profiles ship. The three canonical guards are `cautious-sentry`,
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
| `the-stalker` | ABI 1.2: one target, contact 41, 3.0 fx away, never looks at the guard. | Stalks ticks 1 to 160; first engage at tick 161, at 0.5 fx (strike range); its own attack exposes it. |
| `the-stalker-control` | The same without `CAP_OBSERVATION_CUES`. | No stalk; first engage at tick 65, at 1.0 fx. |
| `caught` | 41 looks at the guard once, on tick 101, while it stalks. | One `caught` tell, at tick 101; exposed 101 to 200; engages at 116. |
| `caught-control` | The same cast, cue included, without the capability. | No tell, no exposure; the stalker control's mind trace exactly. |
| `false-exposure` | 41 glances once, on tick 21, then looks away and keeps its distance. | One `caught`, at 21; exposure true on 90 ticks (22 to 111) on which 41 does not know; expires at 112. |
| `false-exposure-control` | The same without the capability. | 0 such ticks. |

The `the-grudge` and `the-flinch` profiles carry a `CorridorScript`
(`HostTuning::script`): contacts present for a window of ticks, standing still,
that may hit the guard. The cast replaces the corridor's default player and
decoy, and is in the world's truth while present. The guard holds at x = 50 and
each contact stands at the fixture's fx distance at the corridor's scale (8
world units to 1.0 fx). The override and startle figures are the kernel
fixtures' own.

The six model profiles (section 9a) add four script fields: a contact's
`observes` ticks (the cue), `retreats` (it walks off by each step the guard
takes toward it), and the script's `pace` and `model`. Their figures are the
kernel fixtures' (evil-weevil ADR-0014 §7), each one tick later, because the
corridor counts from tick 1 and the fixtures from 0; the cue ticks are shifted
by one to match.

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

## 9a. The model of the target's knowledge (ABI 1.2)

Evil Weevil ABI 1.2 (ADR-0014, Proposed) adds one belief, *exposure*: "a target
knows where I am". `WeevilHost::tick_model(obs, damage_source, observers)` is
`ee_tick_mind` with the 40-byte `ee_model_trace`, passed by its first member
(the 1.1 record). It sets `EE_CONTACT_FLAG_OBSERVES` on each observer, refusing
any id the guard does not perceive this tick, checks the model fingerprint
(`MODEL_FINGERPRINT`, 69857) on every call, and decodes the exposure fields
(`Exposure`), refusing a change without a cause. The kernel reads the cue only
under `CAP_OBSERVATION_CUES` (bit 10), which no stage 1 profile declares; the
host now declares `abi_minor` 2. `tick_mind` still passes the 32-byte trace and
the kernel writes nothing past it.

New ABI terms the mirror carries: `CAP_OBSERVATION_CUES`,
`CONTACT_FLAG_OBSERVES`, `INTENT_FLAG_CAUGHT` (128), `TELL_CAUGHT` (8),
`AGENT_MODE_STALK` (5, the action `TAKE_COVER`), `AGENT_FLAG_EXPOSED` and the
exposure counter bits of `ee_agent.flags`, and `EXPOSURE_CAUSE_{NONE, CUE,
OWN_ATTACK, EXPIRY}`.

The corridor's world says what a contact knows by its own rule, never by the
kernel's belief (`src/world.rs`, "What a contact knows"): **a contact knows
where the guard is on tick t exactly when, on tick t, it observes the guard or
the guard attacks it.** The two can differ, and that difference is the effect.

In a model run's runner output, for the one contact C and the untimed
proposition `guard-0 in room-0`:

* C's own belief (order 0): `certain` from the tick C comes to know, superseded
  by an `unknown` one on the tick it stops, as `conformance/tom/v1` writes it.
* The guard's model of it (`holders = ["guard-0"]`, `bearer` C, the same
  proposition): `certain` while exposure is true, `rejected` while it is false;
  stated first where the kernel first has a target, then only where exposure
  changes, so a stale model stays visible. None without the capability.
* Each change cites its cause: the cue as an observation, the own attack as an
  `action_outcome`, expiry as the guard's perception on the tick (the decay
  convention).

The proposition carries no tick, so the emitter refuses a run in which the
guard changes room. The policies for these documents are
`tests/fixtures/enaction-policies-depth-1.json` (`depth.max_order` 1).

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
* The kernel's exposure is about "a target", not a named one (ADR-0014): the
  first visible contact with the cue sets it. A model scenario therefore casts
  exactly one contact, and the emitter refuses a run that names a second.
* A control (no `CAP_OBSERVATION_CUES`) holds no model, so its runner output
  is accepted under the depth-0 policies too.
* Native pointer checks are not memory isolation.
