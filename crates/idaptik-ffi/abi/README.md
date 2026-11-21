# idaptik-abi — Idris2 model of the `idaptik-ffi` C ABI

An executable specification of the boundary `crates/idaptik-ffi` exports, in
total Idris2, per [issue #103](https://github.com/catachresis-colloquium/IDApTIK/issues/103)
and ADR-0001. The C header (`include/idaptik.h`) is the product surface; this
package is its proof-carrying shadow.

## What is machine-checked

The package builds under `%default total` — every declaration is total, and
the properties below are equations the typechecker verified, not claims:

- **Handle ownership** (`Idaptik.Abi.Handles`) — `free` consumes the one
  linear reference to a live handle; a freed handle is a different type.
  `freedNotLive` checks `Freed ≠ Live`. For a linear client, double-free and
  use-after-free are type errors.
- **String ownership** (`Idaptik.Abi.CABI`) — `stringFree` consumes an owned
  string exactly once; `CString` is validated UTF-8 with no interior NUL
  (`So` proof).
- **Wire shape** (`Idaptik.Abi.Json`) — `renderTickIdentifiesShape` checks
  that the first character of a rendered tick result identifies its shape:
  a success events array can never be confused with an `{"error":"..."}`
  object. The snapshot contract honestly does *not* get this — both its
  success and failure forms are objects.
- **Determinism** (`Idaptik.Abi.Run`) — `step`/`run` are pure functions of
  state and command stream (ADR-0004's determinism is the model's *shape*);
  `stepAdvancesTick` and `buttonPersistsAcrossTicks` check the frame counter
  and the header's "held buttons persist across ticks" rule.
- **Snapshot invariants** (`Idaptik.Abi.Run`) — `snapshotRoundTrip` checks
  that a serialised run state restores to exactly itself; the format tag is
  diffed against the Rust `SNAPSHOT_FORMAT` constant by the gate.
- **A whole session** (`Idaptik.Abi.Exports.demoSession`) — a complete
  well-typed client (new → tick → free string → snapshot → free string →
  free run) typechecks and runs; the protocol has no dead ends.

`Idaptik.Abi.Exports.exportedFunctions` is diffed against the header by the
gate, so the model cannot drift from the C surface in either direction.

### The four proof obligations of [#117](https://github.com/catachresis-colloquium/IDApTIK/issues/117)

`Idaptik.Abi.Protocol` states the C calling protocol as one indexed session
type, `Session : HandleState -> Nat -> HandleState -> Nat -> Type`: the
handle's state and the number of owned strings outstanding, before and after.
A `Complete` session is `Session Live 0 Freed 0`. Ownership and string
pairing are then the same theorem about one object, rather than two theories
that could disagree.

- **O1 — ownership and lifetime.** `noResurrection` checks that no operation
  takes a freed handle back to live. `completeFreesExactlyOnce` checks that a
  complete session frees exactly once, by induction through
  `liveToFreedFreesOnce` and `noFreeAfterFreed`, so it holds on every path
  rather than for one example. A double free has no typing derivation at all.
- **O2 — string/`idap_string_free` pairing.** `sessionBalance` checks
  `n₁ + emitCount s = freeStrCount s + n₂` at *every* prefix;
  `completeBalancesStrings` specialises it, so a complete session frees
  exactly as many strings as it was handed. Freeing a string is deliberately
  polymorphic in handle state, because C permits `idap_string_free` after
  `idap_ghost_lobby_free`.
- **O3 — wire shape and NUL-freedom.** `renderTickCloses` and
  `renderSnapshotCloses` check the closing delimiter, `renderSnapshotIsObject`
  the opening one, and `snapshotOfIsNulFree` that a rendered snapshot holds no
  interior NUL — the property that makes it safe to hand to C as a `char *` —
  stated over the exact expression `Exports` passes to `ownSnapshot`.
- **O4 — deterministic tick and snapshot.** `runAdvancesByFrameCountOnRun`
  checks that the frame clock advances by the number of frames whatever they
  contain, stated about `run` itself via `runFstIsRunState` rather than about
  a model function written alongside it.  `restoreRejectsForeignFormat`
  checks that restore refuses any tag but the current one, with
  `restoreRejectsV2` as a worked instance.

`entryPointIsExported` ties the protocol to the C surface: every operation the
session type permits denotes a function in `exportedFunctions`, which the gate
diffs against `include/idaptik.h`. Header to model by the gate, model to
protocol by the proof.

Each obligation carries **witnesses and positive controls**, because a theorem
about an uninhabited type is true and worthless. `typicalSession` and
`lateStringFree` are inhabitants of `Complete`; `typicalFreesOnce`,
`typicalBalances`, `typicalFreesTwoStrings`, `natCharsIsDecimal` and
`restoreRejectsV2` pin known answers instead of restating the general lemma.

## What is deliberately *not* claimed

- **That `natChars` agrees with `show`.** The snapshot tick counter is
  rendered by `Idaptik.Abi.Json.natChars`, not `show`, because
  `show : Nat -> String` elaborates to `primNumShow` and does not reduce, so
  no proof can be stated over it. That `natChars` emits the *same bytes* as
  `show` is therefore not a theorem. The smoke harness measures it over 2005
  values, checking a deliberately wrong renderer first to establish that the
  comparator is able to disagree.
- **That "two runs of the same inputs agree".** In this model that is `Refl`
  — a lemma that cannot fail, and so evidence of nothing. The determinism
  content lives in O4's frame clock, which can fail.

## Honest gaps (also tracked in #103)

- The **Zig adapter** now exists (`../zig-adapter/`), gated by
  `.github/workflows/abi-conformance.yml` (surface lockstep against
  `exportedFunctions` and the header, fixture conformance against the Rust
  cdylib incl. a planted failing fixture, authority-digest drift rejection).
  The C ABI itself is still implemented directly in Rust (`idaptik-ffi`).
- The tick parser accepts a **simplified commands wire** (`;`-separated
  `Jump` / `SetButton:<button>:<down|up>`), not the serde JSON the session
  layer and TUI share. Bridging the two — plus the full event vocabulary —
  is now exercised by the conformance harness (`../zig-adapter/`): the serde
  JSON wire is the wire the fixtures drive through the ABI, and the simplified
  model wire remains a documented idealisation of it.
- Two idealisations, documented at their declaration sites: a C `const`
  borrow cannot be expressed linearly, so observation functions thread the
  handle through and hand it back; and the demo network's device count is
  modelled as an opaque number.

## Gate

```
just abi-model-check        # or: bash scripts/abi_model_check.sh
```

Escape scan, typecheck, runtime smoke, header correspondence,
snapshot-format parity, and proof mutants. A missing toolchain is a failure,
never a skip, and so is a toolchain that is not 0.8.x.

### The escape scan (`scripts/idris_escape_scan.sh`)

Runs first and needs no compiler, so it gates every pull request rather
than only the ones where the Idris2 bootstrap job runs. It also runs
standalone in the `fixtures-and-must` CI job. Two checks:

1. **No proof escape outside comments** — `postulate`, `believe_me`,
   `really_believe_me`, `assert_total`, `assert_smaller`, `assert_linear`,
   `unsafePerformIO`, `idris_crash`, `partial`, `covering`, `%unsafe`.
   Matched on word boundaries, so `partialOrder` is not a hit; `{- -}`,
   `|||` and `--` comments are stripped first, and string literals are
   deliberately *not* stripped — a false positive is one line to fix, a
   false negative is a hole nobody sees.
2. **Every packaged module carries `%default total`** — the module list
   comes from `idaptik-abi.ipkg`, not a glob, so the check tracks what is
   actually built. This escape's form is an *absence*: deleting the pragma
   disarms every proof in a module with no compiler error and leaves no
   token for a text scan to find, so it needs its own check.

Keeping an escape requires a marker on the line (`-- ESCAPE-WAIVER: <id>`)
*and* a reviewed row in [`ESCAPE-LEDGER.tsv`](./ESCAPE-LEDGER.tsv) naming
the term, the file and the reason. Waivers key on the id, never a line
number, so inserting a line cannot transfer a waiver to different code;
one id excuses exactly one line; and a row matching no escape fails the
gate, because an orphan row is a standing permission the next escape to
land there would inherit.

The scan prints its denominator on success — `7 .idr files, 719 lines of
code scanned, 0 waived escapes` — and refuses to pass over an empty tree,
so a gate that has stopped looking at anything says so instead of `ok`.
`--self-test` builds a fixture tree and runs 12 controls, 10 of them
mutants the gate must kill; CI runs it on every push, so the gate is
checked for its ability to fail rather than only observed to pass.

### The typecheck deletes `build/` first

That is load-bearing, not hygiene. The package carries `opts = "--total"`,
but the flag is applied when a module is *compiled*, and a `.ttc` built
without it is reused unchecked. Staged against this package with a
non-terminating function planted in `Json.idr`:

| build | artefacts | result |
|---|---|---|
| no `--total` | clean | passes |
| `--total` | **`build/` kept** | **passes — the mutant survives** |
| `--total` | `build/` deleted | fails — the mutant dies |

A gate that reused artefacts would report "typechecked and total" over a tree
that had never been checked for totality at all.

### The proof mutants (`scripts/abi_proof_mutants.sh`)

Compiling under `--total` and agreeing with the header says nothing about
whether the *proofs* have teeth. So each mutant falsifies one lemma, and the
build must fail **and name the lemma it rejected**. A non-zero exit code
alone cannot tell a dead mutant from a typo or a toolchain fault, and a mutant
killed by an unrelated error is a false pass that looks exactly like a real
one.

Seven mutants cover all four obligations and the header tie-in: a wrong free
count, a double free, a wrong string count, an entry point renamed to a
different exported function, a wrong decimal rendering, a wrong opening
delimiter, and an off-by-one frame clock. An unmutated copy is built first as
a positive control — without it, a tree that failed to build for an unrelated
reason would report every mutant as killed. A mutation that matches nothing
fails loudly rather than reading as a survivor, because a renamed lemma
otherwise stops being tested in silence. Every mutation is applied to a copy;
the working tree is never modified.

## Toolchain (bootstrap recipe)

The model targets Idris 2 **0.8.x** with the Chez backend, and the gate
refuses any other version: a 0.7.x compiler fails deep inside the proofs with
errors that read like defects in the model rather than like a version
mismatch.

If you already have a 0.8.x build, point the gate at it and skip the recipe
entirely:

```sh
IDRIS2=/path/to/idris2 bash scripts/abi_model_check.sh
```

There is no distribution package for Idris 2 on the runner image this
repository's CI uses, so CI builds one with the recipe below (native Chez
10.4.1, no curses/X11 — no system library dependencies beyond libc):

```sh
git clone --depth 1 --branch v10.4.1 --recurse-submodules \
  --shallow-submodules https://github.com/cisco/ChezScheme.git
cd ChezScheme
./configure --threads --disable-curses --disable-x11 \
  --installprefix="$PREFIX/chez-native"
make -j"$(nproc)" && make install

git clone https://github.com/idris-lang/Idris2.git && cd Idris2
git checkout v0.8.0
export PATH="$PREFIX/chez-native/bin:$PATH"
export LD_LIBRARY_PATH="$PREFIX/chez-native/lib"
make -C support/c        # a fresh clone must build this first: without it,
                         # `make bootstrap` stops at the RefC support (gmp.h)
make bootstrap SCHEME="$PREFIX/chez-native/bin/scheme"   # install-refc may
# still fail on a missing gmp.h — harmless for the Chez codegen; the compiler
# and libraries are built by then
export IDRIS2_PREFIX="$PREFIX/idris-install"   # where the libraries below land
for lib in prelude base linear network; do
  (cd libs/$lib && ../../build/exec/idris2 --install $lib.ipkg)
done
# `make bootstrap` installs the C support library only into the bootstrap's own
# prefix, never into $IDRIS2_PREFIX. `idris2 --build` copies it from
# <prefix>/idris2-0.8.0/lib into the generated <exe>_app/ dir, so without these
# two lines a build SUCCEEDS and the binary then dies at load time with
# `Exception: (while loading libidris2_support.so)`.
mkdir -p "$IDRIS2_PREFIX/idris2-0.8.0/lib"
install -m 755 support/c/libidris2_support.so "$IDRIS2_PREFIX/idris2-0.8.0/lib/"
install -m 644 support/c/libidris2_support.a  "$IDRIS2_PREFIX/idris2-0.8.0/lib/"
export IDRIS2_PACKAGE_PATH="$IDRIS2_PREFIX/idris2-0.8.0"   # installed package root
```

`scripts/abi_model_check.sh` finds the toolchain via `$IDRIS2`, then
`/tmp/idris-bootstrap/bin/idris2`, then `PATH`, and rejects anything that does
not report 0.8.x.

## Licence

AGPL-3.0-or-later, like the workspace this model shadows.
