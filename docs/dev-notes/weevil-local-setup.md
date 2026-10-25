# Running the Evil Weevil guard brain locally

This is the setup guide for the Evil Weevil vertical slice: getting the kernel,
the adapter and the game running on your own machine, and knowing which of the
steps have actually been verified.

Start with `docs/dev-notes/weevil-play.md` if you want to play it first and
come back to the build only when something breaks.

## What is involved

Three pieces, in this order:

| Piece | Where it lives | What it needs |
| --- | --- | --- |
| The kernel (`libevil_weevil.a`) | a separate checkout, `catachresis-colloquium/evil-weevil` | **Zig 0.16.0** (not 0.14.0 — see below), `gcc` |
| The Rust adapter (`crates/idaptik-weevil`) | this repository | `cargo`, the built kernel |
| The game (`crates/idaptik-bevy --features weevil`) | this repository | `cargo`, Bevy's system libraries, a display |

`just` recipes wrap all of it. Run `just` with no arguments to list them; the
ones you will use are `weevil-doctor`, `weevil-kernel-check`, `weevil-test`,
`weevil-replay` and `weevil-run`.

## 1. Get the code

The kernel is a separate repository. Clone it **beside** IDApTIK — the default
search paths look for `../evil-weevil`:

```sh
cd ~
git clone https://github.com/catachresis-colloquium/evil-weevil.git
```

If it is somewhere else, point at it once:

```sh
export EVIL_WEEVIL_ROOT=/path/to/evil-weevil
```

or per recipe:

```sh
EVIL_WEEVIL_ROOT=/path/to/evil-weevil just weevil-kernel-check
```

### Getting this branch

The slice lives on branch `arena/9d80877f-idaptik`. If you are on `main`:

```sh
git fetch origin arena/9d80877f-idaptik
git checkout arena/9d80877f-idaptik
```

(**Note:** other Arena sessions use branches named `arena/<uuid>-idaptik`.
`arena/07d91f52-idaptik` is a *different* session's checkout and will not have
this work. Check the branch name before you start.)

## 2. Zig 0.16.0 — the one non-obvious requirement

**The kernel needs Zig 0.16.0, and IDApTIK's `mise.toml` pins 0.14.0.** This was
measured, not assumed:

| Zig | `zig build` | Kernel test suite | Links into a C host? |
| --- | --- | --- | --- |
| 0.14.0 | succeeds | **fails** — test binaries do not spawn | **no** — `__zig_probe_stack` undefined |
| 0.16.0 | succeeds | 32/32 pass | yes — clean |

Under 0.14.0 the archive carries an undefined `__zig_probe_stack` reference, so
no C host — and therefore no Rust host either — can link the result. `scripts/weevil_kernel.sh`
checks the version and refuses to build with a stale one rather than producing
an unlinkable library.

Two ways to get 0.16.0:

**a) Install it directly (verified working):**

```sh
python3 -m pip install --target /opt/zig16 ziglang==0.16.0
export EVIL_WEEVIL_ZIG=/opt/zig16/ziglang/zig
```

The `ziglang` PyPI wheel is the full compiler. This is the route used to build
and test the kernel for the evidence in `docs/evidence/`.

**b) Let `mise` provide it.** `mise.toml` currently pins 0.14.0 for IDApTIK's own
Zig adapter (`crates/idaptik-ffi/zig-adapter/`), which is a different thing from
the evil-weevil kernel. Bumping that pin to 0.16.0 is a repo-wide toolchain
decision and has **not** been made — the kernel build is pinned separately via
`EVIL_WEEVIL_ZIG` instead, so the two adapters keep their own versions.

## 3. Check the environment

```sh
just weevil-doctor
```

Expected, on a machine with everything in place:

```
evil-weevil : /home/you/evil-weevil
zig         : /opt/zig16/ziglang/zig (0.16.0)
cargo       : cargo 1.95.0 (...)
adapter     : present
```

It exits non-zero and names the fix for anything missing. This is the first thing
to run when a later step fails.

## 4. Build and verify the kernel

```sh
just weevil-kernel-check
```

This runs the kernel's own three gates: build the library, run its Zig test
suite, then compile and run the C null host. Expected tail:

```
Build Summary: 8/8 steps succeeded; 32/32 tests passed
...
null host: OK — the kernel ran with no engine behind it.
weevil-kernel: kernel build, test suite and C null host all pass
```

The null host prints four golden digests. They are the kernel's own acceptance
numbers and are recorded in `docs/evidence/weevil-integration-evidence.md`:

```
digest after 10,000 ticks (memory enabled): 11339491356780309808
the same trace with the memory flag cleared: 1235117735680515552
chase digest: 12283675074724005768
flap digest: 5191914376283395890
```

## 5. Check the ABI mirror (no compiler needed)

```sh
just weevil-abi-mirror
```

```
ok   ee_version       size=   8 fields= 4
...
ABI mirror matches the generated header on every field, offset and size
```

This compares the Rust `#[repr(C)]` transcription in
`crates/idaptik-weevil/src/abi.rs` against the *generated* `ee.h`, field by
field, offset and size. It needs only Python, so it runs before the kernel is
built — and it is the check that catches a transcription which is the right size
but the wrong field order, which compiles fine in Rust and corrupts the kernel's
view of the world at runtime.

## 6. Build and test the adapter

```sh
just weevil-build     # cargo build -p idaptik-weevil
just weevil-test      # cargo test  -p idaptik-weevil
just weevil-core-test # cargo test  -p idaptik-core
```

`weevil-test` runs the headless replay gate. **This is the one that matters most
right now**: six defects were found in the adapter by reading it against the
kernel's own source and fixed, and none of them has been compiled. If this suite
passes, the fixes are confirmed.

`weevil-core-test` is the game's own suite. It is the check that the classic FSM
is unchanged by the seam — the seam is additive and opt-in, and this is the
evidence for that claim, not the structure of the code.

## 7. Run the headless replay

No window, no game, no server:

```sh
just weevil-replay TICKS="240" PROFILE="cautious-sentry" SEED="11"
```

Look for `blocked > 0` in the summary. That is the assertion that **fails** if
the world→fx scale is wrong — with the buggy default the guard crawls at a
sixteenth of its speed and never reaches the first closed door in 240 ticks, so
it is never blocked. See `docs/evidence/weevil-integration-evidence.md` §
"Static review of the Rust against the kernel's own source" for the full list.

Write one and check it reproduces:

```sh
just weevil-replay-out OUT="/tmp/weevil.json"
just weevil-replay-check FILE="/tmp/weevil.json"
```

## 8. Run the game

On Linux you need Bevy's system libraries first:

```sh
just bevy-linux-deps
```

Then:

```sh
just weevil-run PROFILE="cautious-sentry" SEED="7"
```

See `docs/dev-notes/weevil-play.md` for what to look at and how to compare the
kernel against the classic FSM.

## Troubleshooting

| Symptom | Cause | Fix |
| --- | --- | --- |
| `no evil-weevil checkout found` | kernel not cloned, or not where the search looks | clone beside IDApTIK, or set `EVIL_WEEVIL_ROOT` |
| `is Zig 0.14.0, but the kernel needs >= 0.16` | the mise-provided Zig is IDApTIK's pin, not the kernel's | set `EVIL_WEEVIL_ZIG` to a 0.16.0 build |
| `cannot find -levil_weevil` / link errors | the kernel library was never built, or `EVIL_WEEVIL_LIB_DIR` points elsewhere | `just weevil-kernel-check`, then `just weevil-build` |
| `__zig_probe_stack` undefined | the library was built with Zig 0.14.0 | rebuild with 0.16.0 (see §2) |
| `unknown profile '…'` | profile id is not one of the three canonical ones | `cautious-sentry`, `aggressive-pursuer`, `equipment-specialist` |
| Bevy build fails on Linux | missing system libraries | `just bevy-linux-deps` |
| The guard never moves | the profile's `patrol_x` is empty, so it holds | check the profile; an empty route means "hold" by design |

## What has and has not been verified

Verified in the authoring sandbox (Zig only — there is no Rust toolchain there):

* the kernel builds under Zig 0.16.0 and its suite reports 32/32;
* the C null host passes and prints the four golden digests above;
* the ABI mirror matches the generated header on every field, offset and size;
* the Zig 0.14.0 failure mode above, measured both ways.

**Not verified:** everything that needs `cargo`. The adapter, the core seam, the
Bevy driver and the game have never been compiled. The six defects recorded in
`docs/evidence/weevil-integration-evidence.md` are fixed in the working tree but
uncompiled, so `just weevil-test` is the check that confirms them — until it
passes, the integration is a design supported by the code, not an observed
result. See `docs/evidence/weevil-integration-evidence.md` for the same
distinction in full.
