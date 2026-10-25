# Evil Weevil guard integration — evidence

Date: 2026-10-24. Authoring environment: a sandboxed checkout of
`catachresis-colloquium/IDApTIK` on branch `arena/9d80877f-idaptik`.

This file separates what was **run** from what was **written**. The distinction
is the whole point: an unverified design claim and an observed result are
different things and should not be read as the same thing.

## Status summary

| Claim | Status |
| --- | --- |
| Zig toolchain installed and working | **Verified** — `zig version` → `0.16.0` |
| Evil Weevil kernel builds to `libevil_weevil.a` | **Verified** — real build, 9,996,806 bytes |
| The kernel's own test suite passes | **Verified** — 32/32 (21 + 11) |
| The C null host passes (the other side of the ABI) | **Verified** — `null host: OK` |
| The ABI constants in `crates/idaptik-weevil/src/abi.rs` match the generated `ee.h` | **Verified** — `tools/verify_abi_mirror.py`, every field/offset/size |
| The DEED document conforms to the normative grammar | **Verified** — run against the owner's own linter |
| The adapter crate compiles | **Not run** — no Rust toolchain obtainable |
| The game runs on the kernel brain | **Not run** — no Rust toolchain |
| The replay is reproducible | **Not run** — requires the Rust adapter |
| The classic FSM is unchanged by default | **Not run** — additive by construction, unverified |

## What was actually run

### 1. The kernel builds and its tests pass (Zig 0.16.0)

Zig 0.16.0 is the version the project's CI pins
(`.github/workflows/seam.yml` keys its cache on `zig0.16.0`). With it:

```
$ zig build --build-file src/interface/ffi/build.zig
$ ls -la src/interface/ffi/zig-out/lib/libevil_weevil.a
-rw-r--r-- 1 user user 9997950 ... libevil_weevil.a

$ zig build test --build-file src/interface/ffi/build.zig --summary all
FLAP switches=0 mode_switches=0 attack=100 flee=0 digest=5191914376283395890
Build Summary: 8/8 steps succeeded; 32/32 tests passed
  +- run test 21 pass (21 total)
  +- run test 11 pass (11 total)
```

**Zig 0.14.0 must not be used, and that is the trap inside this repository.**
IDApTIK's own `mise.toml` pins Zig 0.14.0, so a plain `just setup` puts a 0.14.0
compiler on PATH before anything else. Under 0.14.0 `zig build` exits 0 — it
looks like it worked — but the archive it produces carries an undefined
`__zig_probe_stack` reference:

```
$ gcc -std=c11 -Wall -Wextra -Werror -Iinclude tests/host/null_host.c \
      src/interface/ffi/zig-out/lib/libevil_weevil.a -o _build/null-host
undefined reference to `__zig_probe_stack'
```

so no C host — and therefore no Rust host either — can link it. Its test suite
is broken too: the test binaries do not spawn (`FileNotFound`, 5/8 steps), and the
archive is 2,273,058 bytes against 9,997,950 under 0.16.0. Zig 0.16.0 produces a
9,997,950-byte archive with **zero** `__zig_probe_stack` references, links clean,
and reports 8/8 steps and 32/32 tests.

Zig 0.17.0 is the opposite problem — too new. The kernel's own test source fails
a style lint on `[_]u32{0} ** 9`. That is a newer-Zig strictness change, not a
kernel defect. The version that works is **0.16.0**, which is what the project's
CI pins (`.github/workflows/seam.yml` keys its cache on `zig0.16.0`). The lesson
is that the version matters and the CI pin is the thing to match.

`scripts/weevil_kernel.sh` enforces this: it refuses to build with Zig < 0.16.0
rather than producing an archive nobody can link, and it explains the pip fix
when it does so. `just weevil-doctor` reports which Zig it found and whether that
is sufficient. Note that bumping IDApTIK's `mise.toml` zig pin is a separate,
repo-wide decision — that pin serves IDApTIK's own unrelated Zig adapter in
`crates/idaptik-ffi/zig-adapter/`, and this slice leaves it alone and pins the
kernel's toolchain separately through `EVIL_WEEVIL_ZIG`.

### 2. The C null host passes, and prints the golden digests

```
$ gcc -std=c11 -Wall -Wextra -Werror -Iinclude tests/host/null_host.c \
      src/interface/ffi/zig-out/lib/libevil_weevil.a -o _build/null-host
$ ./_build/null-host
evil-weevil ABI 1.0, fingerprint 18276775
  ee_version     8   ee_init_desc  64   ee_contact 16
  ee_snapshot  184   ee_agent      48   ee_intent  48   ee_context 72
  digest after 10,000 ticks (memory enabled): 11339491356780309808
  the same trace with the memory flag cleared: 1235117735680515552
  chase digest: 12283675074724005768 (investigate=3600 advance=6400 from_memory=3600)
  flap digest: 5191914376283395890 (attack=100 flee=0 switches=0 mode_switches=0)
  actions in 200 ticks: reload=10 advance=75 attack=62 flee=14 hold=0 investigate=39
null host: OK — the kernel ran with no engine behind it.
```

The struct sizes and the four digests are exactly the constants recorded in
`crates/idaptik-weevil/src/abi.rs` and in the session's kernel notes. This is
independent confirmation from a C compiler that knows nothing about Rust.

### 3. The Rust ABI mirror matches the header (`tools/verify_abi_mirror.py`)

The header is self-describing. This script parses `include/evil_weevil/ee.h` and
`crates/idaptik-weevil/src/abi.rs`, lays each struct out with x86-64 alignment
rules, and diffs field names, field order, offsets and total size:

```
$ python3 tools/verify_abi_mirror.py \
      evil-weevil/include/evil_weevil/ee.h \
      crates/idaptik-weevil/src/abi.rs
ok   ee_version       size=   8 fields= 4
ok   ee_init_desc     size=  64 fields=11
ok   ee_contact       size=  16 fields= 6
ok   ee_snapshot      size= 184 fields=22
ok   ee_agent         size=  48 fields=10
ok   ee_intent        size=  48 fields=12
ok   ee_context       size=  72 fields=12

ABI mirror matches the generated header on every field, offset and size
```

This matters because a transcription error of the *right size* but the *wrong
field order* would compile fine in Rust and corrupt the kernel's view of the
world at runtime. It is checked here rather than hoped for.

### 4. DEED grammar conformance

```
$ python3 /home/user/standards/1-formats/deed/tools/deed_lint.py \
      docs/standards/deed/idaptik-weevil-guard-brain_chora.deed
OK   docs/standards/deed/idaptik-weevil-guard-brain_chora.deed
```

### 5. Toolchain reconnaissance (why Rust is missing)

* No Rust or Zig toolchain was present initially.
* Reachable hosts in this sandbox: `github.com`, `codeload.github.com`,
  `api.github.com`, `registry.npmjs.org`, `pypi.org`, `files.pythonhosted.org`.
* **Blocked**: `static.rust-lang.org`, `sh.rustup.rs`, `ziglang.org`,
  `deb.debian.org`, `raw.githubusercontent.com`,
  `objects.githubusercontent.com`, `release-assets.githubusercontent.com`.
* `rustup` has **no GitHub releases** (the releases list is empty) — it is
  distributed only from `static.rust-lang.org`, which is blocked.
* GitHub release *assets* redirect to
  `release-assets.githubusercontent.com`, also blocked. So GitHub can supply
  source but not prebuilt binaries.
* `rustup` on PyPI and `rustc` on npm are both downloaders for
  `static.rust-lang.org`. No PyPI package bundles a real `rustc`.
* Zig, however, **is** obtainable: the `ziglang` PyPI wheel is the full
  compiler, hosted on `files.pythonhosted.org`. That is the route used.

## How the toolchain was installed

System-wide, so every user gets it:

```sh
# Zig 0.16.0 (the CI-pinned version) from the PyPI wheel
python3 -m pip install --no-cache-dir --target /tmp/zig16 ziglang==0.16.0
sudo mkdir -p /opt/zig
sudo cp -a /tmp/zig16/ziglang /opt/zig/
sudo ln -sf /opt/zig/ziglang/zig /usr/local/bin/zig
echo 'export PATH="/opt/zig/ziglang:$PATH"' | sudo tee /etc/profile.d/zig.sh
sudo chmod -R a+rX /opt/zig
```

Verified for an unprivileged user:

```
$ sudo -u nobody bash -lc 'command -v zig && zig version'
/opt/zig/ziglang/zig
0.16.0
```

A copy also lives under `/home/user/.local/zig16/ziglang/zig`, because only
`/home/user` persists across the authoring sandbox's turns.

## Static review of the Rust against the kernel's own source

This is **inspection, not execution**. With no compiler available, the adapter
was read line by line against `evil-weevil`'s `src/core/kernel.zig` and
`src/interface/ffi/src/main.zig` — the kernel's own statements of what it reads
and what it ignores. Six defects were found and fixed; **none has been
compiled**. Recording them means the first `cargo test` reads as a check of the
fixes rather than of the integration as a whole.

1. **The world→fx scale was inverted.** `DEFAULT_FX_PER_WORLD_UNIT` was `8.0`.
   The conversion is a multiply (`world_to_fx(w) = to_fx(w * scale)`), so `8.0`
   puts the kernel's `1.0` attack range at one eighth of a canvas unit. A guard
   could never close to engage, and a corridor patrol crawled at a sixteenth of
   its speed. The corridor's own door test would have failed: at `8.0` the guard
   covers ~30 canvas units in 240 ticks and never reaches the first closed door
   at x=200, so it is never blocked. Changed to `1.0 / 8.0`, which is what "one
   fx unit is eight canvas units" requires. The kernel's own thresholds confirm
   the reading: `ATTACK_RANGE = 65536` (`1.0`), `FLEE_RANGE = 131072` (`2.0`),
   `FX_ONE = 65536`.
2. **`budget_units` was documented as behavioural.** `ee_init` copies
   `capabilities` and `flags` but has **no** budget field; it overwrites
   `ctx.budget_units` with a hard-coded 16. The kernel's `workUnits` is
   `5 + contact_count`, which is 13 at the ABI's eight-contact maximum. So
   `INTENT_FLAG_DEGRADED` **cannot fire in v0** and a profile's `budget_units`
   changes nothing observable. Corrected to a forward-compatibility declaration,
   with `WeevilHost::budget_units()` reading back the effective value.
3. **`moved` was mislabelled when blocked.** `world.rs::apply` set
   `moved: applied != 0`, so a tick pressed against a closed door — where the
   guard's position does not change — was recorded as a move. Now computed from
   the position delta, matching the game's own `apply_guard_decision`.
4. **A tick status was swallowed.** `WeevilHost::tick` mapped every unrecognised
   status to `UnknownStatus`, hiding the `VERSION_MISMATCH` a context overwrite
   produces. The status arms are now explicit.
5. **`replay_equal` missed two identity fields.** `scenario_id` and
   `initial_context` are folded into `decision_identity` but were not compared,
   so two traces differing only in their starting state would have been called
   equal.
6. **The movement limit was applied in the wrong unit.** `max_speed_world` is
   documented as "world units per tick", but `world.rs::apply` clamped the
   kernel's *fx* speed against `max_speed_fx()`. The kernel's advance is 1.0 fx
   and the sentry's ceiling is its fx form, 1.6 fx, so the clamp never bit and
   the guard ran at 8 canvas units per tick whatever the profile declared —
   `max_speed_world` was a number nothing read. The limit is now applied in
   world units after the fx→world conversion, which is what the doc says.

Two profile fields were also found to have no consumer — `investigate_dist_world`
and `DecoyTuning` — and are now documented as declared rather than implied.
`DecoyTuning` was wired into the headless corridor as the deception's authoring
seam; `investigate_dist_world` stays unconsumed, because consuming it needs the
game to read a profile it does not currently hold.

The same review confirmed the claims that matter most, by reading the kernel:
memory is a **bearing plus age** (`kernel.zig` writes `memory_x`/`memory_y` from
`bearing_cos`/`bearing_sin` and `memory_age` from the tick count) and is never a
position; `cached_target` is the id an `INVESTIGATE` will name; the kernel's
`modeForAction` matches the adapter's `Action::mode_bit` exactly; and the kernel
never calls back into the host mid-tick.

## What was written

All of it is on branch `arena/9d80877f-idaptik`, uncommitted.

**`crates/idaptik-weevil/`** (new crate; links the native kernel library)

* `Cargo.toml` — library `idaptik_weevil`, binary `idaptik-weevil-replay`.
* `build.rs` — resolves `libevil_weevil.*` from `EVIL_WEEVIL_LIB_DIR`,
  `EVIL_WEEVIL_ROOT/src/interface/ffi/zig-out/lib`, or
  `../../evil-weevil/src/interface/ffi/zig-out/lib`. Does not stub when absent.
* `src/abi.rs` — `#[repr(C)]` transcription of the generated `ee.h` (now
  independently verified, see §3).
* `src/fx.rs`, `src/error.rs`, `src/observation.rs`, `src/intent.rs`,
  `src/profile.rs`, `src/host.rs`, `src/replay.rs`, `src/world.rs`,
  `src/lib.rs`, `src/bin/weevil_replay.rs`, `tests/replay_repro.rs`.

**`tools/verify_abi_mirror.py`** — the ABI mirror checker used in §3.

**`scripts/weevil_kernel.sh`** (new) — resolves the evil-weevil checkout
(`$EVIL_WEEVIL_ROOT`, `../evil-weevil`, `../../evil-weevil`, `$HOME/evil-weevil`,
verified by `include/evil_weevil/ee.h` rather than by directory name), resolves
Zig, and refuses < 0.16.0 with the exact pip command to fix it. Subcommands
`locate`, `build`, `test`, `host`, `check`, `doctor`. Every subcommand fails
loudly on a missing kernel; a missing build is an error, never a skip.

**`justfile`** — 12 `weevil-*` recipes appended: `weevil-doctor`,
`weevil-kernel`, `weevil-kernel-check`, `weevil-abi-mirror`, `weevil-build`,
`weevil-test`, `weevil-core-test`, `weevil-check`, `weevil-replay`,
`weevil-replay-out`, `weevil-replay-check`, `weevil-run`, `weevil-run-classic`.
Validated by extracting each recipe's shell body and running `bash -n` — 51
recipes parse and all bodies are clean; the one flagged recipe is a pre-existing
`\`-continuation false positive, not a defect.

**`crates/idaptik-core/`**

* `src/scenario/guard_brain.rs` — the game's seam in plain data.
* `src/scenario/mod.rs`, `src/scenario/sim/mod.rs`, `src/scenario/sim/systems.rs`
  — perception, target validation, decision application, brain selection.
* Four tests in the sim's test module covering the seam.

**`crates/idaptik-bevy/`**

* `src/guard_weevil.rs` — the Bevy driver and plugin, plus the `V` toggle that
  flips the brain between the kernel and the classic FSM at run time, in `Update`
  so a multi-tick frame cannot flip it twice.
* `src/hud.rs` — the guard-brain developer view.
* `src/lib.rs`, `src/main.rs`, `Cargo.toml` — wiring and the `weevil` feature.

**Docs**

* `docs/standards/deed/idaptik-weevil-guard-brain_chora.deed` (validated)
* `docs/standards/deed/idaptik-weevil-guard-brain-conformance.adoc`
* `docs/dev-notes/weevil-integration.md`
* `docs/dev-notes/weevil-local-setup.md` (new) — the setup path a developer
  follows, including the Zig 0.16.0 requirement and its evidence
* `docs/dev-notes/weevil-play.md` (new) — the player-facing UI guide: the
  developer view line by line, the eight behaviours to watch, the comparison and
  rollback, and the bounded player check

## What is deliberately not claimed

* **No Rust was compiled.** `cargo build`, `cargo test`, `cargo clippy` and
  `cargo fmt` could not run. Every Rust file — the adapter, the core seam, the
  Bevy driver — is **unverified** until a Rust toolchain is available.
* The six defects recorded above were found by reading, not by a failing test.
  They are fixed in the working tree but **uncompiled**, so the fixes themselves
  are unverified until `cargo test -p idaptik-weevil` runs.
* The adapter's own tests (`tests/replay_repro.rs`, plus the unit tests inside
  each module) are **unexecuted**. Their assertions are design intent.
* The replay-reproducibility claim is untested; it needs the Rust adapter.
* The claim that the classic FSM is behaviourally unchanged is structural (the
  default is `Classic` and the new code is opt-in), not measured. The scenario's
  existing golden-JSON tests are the check that would prove it.

## How to turn the rest into evidence

Rust is the only remaining blocker. Once a toolchain is reachable:

```sh
# 0. the toolchain (see above)

# 1. the kernel, from the evil-weevil checkout
just seam-check          # or: zig build test + gcc null host, as above

# 2. the adapter and its tests
cargo build -p idaptik-weevil
cargo test -p idaptik-weevil

# 3. the game's own suite — this is the one that proves the FSM is unchanged
cargo test -p idaptik-core

# 4. the headless replay
cargo run -p idaptik-weevil --bin idaptik-weevil-replay -- \
    --ticks 200 --profile cautious-sentry --seed 7

# 5. the game, on the kernel brain
cargo run -p idaptik-bevy --features weevil
```
