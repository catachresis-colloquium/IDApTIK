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

Three canonical profiles ship: `cautious-sentry`, `aggressive-pursuer`,
`equipment-specialist`. They differ **only** through seams the architecture
assigns:

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

## 9. Known limitations

* The ABI bearing is a `u16` the kernel zero-extends to `i32`. A **signed**
  corridor direction is not representable through it. The adapter applies its
  own navigation and treats the kernel's move vector as advisory.
* `ContactSource`, `confidence` and `age_ticks` live in the adapter's schema, not
  in the kernel.
* The kernel's memory is a bearing plus an age (TTL 36 ticks). It is **not** a
  position. `INVESTIGATE` is resolved against the host's own last-known
  position, never against the kernel's memory.
* Native pointer checks are not memory isolation.
