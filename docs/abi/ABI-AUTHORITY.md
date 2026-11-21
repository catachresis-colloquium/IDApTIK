# ABI Authority — hypatia, with the ABI in Idris2 and the FFI in Zig

Status: **authority ruled and pinned** — this resolves the conformance blocker
recorded in
[issue #103](https://github.com/catachresis-colloquium/IDApTIK/issues/103)
("did not locate a canonical Zig unified Hexadeca API repository/specification").

## The authority, and which layer is normative

The canonical ABI authority is **`hyperpolymath/hypatia`**, ruled by the estate
owner on 2026-09-21. There is no separate "Hexadeca spec" to find: hypatia *is*
the specification, and the estate-wide 2026-09 rename
(Hexadeca-Connector → UnifiedApiAdapter) consolidated on it.

The authority is **layered, and the layering is load-bearing**:

| Layer | Role | Where it lives in hypatia |
|---|---|---|
| **ABI** | **Normative.** The specification itself, written in Idris2. | `src/Hypatia/ABI/*.idr` |
| **FFI** | **Implementation.** Conforms to the ABI; is not itself the spec. | `ffi/zig/src/` |
| **Wire manifest** | The connector id/name table both layers agree on. | `ffi/connectors.json` |
| **BEAM boundary** | Supervised **SNIFs**, never raw NIFs, so a native fault is isolated from the BEAM. | *(none in hypatia today)* |

Conformance therefore runs in one direction: an implementation is checked
**against** the Idris2 ABI. The Zig FFI is the reference *implementation* of
that ABI, not the ground truth — a disagreement between the two is a defect in
the Zig, never a redefinition of the ABI.

## Pinned digests

Canonical revision: `hyperpolymath/hypatia@f079bc06061951d8e4c12f86675f81e97ec6e3a7` (main).

This pin moved from `bcb8bb37` on 2026-09-22 because the ABI stopped being
hand-maintained. Two upstream commits matter and they are **not** the same one:

- **`e0393c2d`** (hypatia #811, closing #120) is where generation landed — the
  Idris2 generator `hypatia-abi-gen`, the three generated targets, and the
  `abi-codegen-drift` workflow.
- **`86aa936a`** (hypatia #819) is where that workflow **first went green**.
  `abi-codegen-drift` failed at `e0393c2d` and at the two pushes after it; the
  fix was in the generator's own argument handling, not in the ABI. Every
  generated artefact is byte-identical at both revisions, so "the output did not
  change" would *not* have told you the gate was red. Pinning `e0393c2d` would
  have recorded a revision at which this document's central claim was false.

`f079bc06` is main's tip at the time of writing, and `abi-codegen-drift` is
green there. Digests below were recomputed against `f079bc06` itself, not
carried forward from the previous pin.

### Normative layer — the Idris2 ABI

| File @ `f079bc06` | sha256 |
|---|---|
| `src/Hypatia/ABI/Types.idr` | `5f6692718b262e9926179144fb527946cbe94503e37815473f18b9c37ed4f3f4` |
| `src/Hypatia/ABI/FFI.idr` | `e7fb117bfe137e76232ddb220db82a389808a63d3eda29e3d47057052366c669` |
| `src/Hypatia/ABI/REST.idr` | `3d3bfe554afcfca22d5fa064621d9f6fb94e16a9b0a2a56b434ef5c66543813f` |
| `src/Hypatia/ABI/GraphQL.idr` | `a5122cfe2039fd1b06ab1e81fadb9d11b23418edd0b8bc25179d5546200914c5` |
| `src/Hypatia/ABI/GRPC.idr` | `c04165ae3426b7bde34d64d21277bbf5d0e0f66fdfa08ac71c9968df6888e5b4` |
| `src/Hypatia/ABI/RuleEngine.idr` | `a191ada02ed9534cd66eb2b3c8de45e354682358e8f9cec4b676b68788adc3bd` |
| `src/Hypatia/ABI/Gen.idr` | `912c55624633fbb44e805d0e3c22ceccb73a9bcfb242e648b7e40a1fe890cc30` |
| `src/abi/hypatia-abi-gen.ipkg` | `3f4f2f8955fdef0db847335810f4c3380f0820f8f01f92c6ceca0f9b32580ce4` |

`Gen.idr` and its `.ipkg` are pinned as part of the normative layer on purpose.
They are not the specification, but they are the *only* thing standing between
the specification and the three files below: if the generator changes, what
"generated from the ABI" means changes with it, and an audit that pins the ABI
but not its generator has a hole exactly the size of that change. `Types.idr`
is the only ABI module whose digest moved from the previous pin.

### Generated layer — emitted from the Idris2 ABI, never hand-edited

| File @ `f079bc06` | sha256 |
|---|---|
| `ffi/connectors.json` | `a96fc2ef5cd83ea36be3953eadbf214eca012e7f6efebccaf4e48a8ae2adf330` |
| `ffi/zig/src/connector_generated.zig` | `0dae1baa3774e1b9dc14cc317c1f50a75a7f1397f36f9ccb508a94a104f6e86e` |
| `clients/rust/hypatia-client/src/connector_generated.rs` | `e16acd06bbad43336032c9ea108510a38b72fd4db5355648e431aa57ffb5ea69` |

`ffi/connectors.json` has moved layer. It previously called itself the *"Golden
source-of-truth"*; it is now generated output and says so. Its own header
declares the git blob hash of the ABI file it was generated from —
`788493e6f54b6d436ea551dbb5d34c6bc0d819ab` for `src/Hypatia/ABI/Types.idr` at
this revision, verified against the blob. That stamp is a **40-hex** blob hash,
so it is deliberately invisible to the 64-hex digest comparison below; it is a
second, independent provenance check that this file was emitted from *that*
source, and it is worth running by hand when moving the pin.

### Implementation layer — the Zig FFI

| File @ `f079bc06` | sha256 |
|---|---|
| `ffi/zig/src/unified-api-adapter.zig` | `8905802c15886f93b9448ceeb35d9fce9221c5def0be27481dc3a0ccf45d9b0e` |

### Superseded pin — `bcb8bb37`, the last hand-maintained revision

Only the files whose digests actually changed are retained. The other five ABI
modules are byte-identical at `bcb8bb37` and `f079bc06`, so a second row would
pin the same number twice and imply a difference that does not exist.

| File @ `bcb8bb37` | sha256 |
|---|---|
| `src/Hypatia/ABI/Types.idr` | `a5f794b11cfbb38b9c38ba861af71d7cbd3e9b97c29a14973b212642d2f0d84e` |
| `ffi/zig/src/unified-api-adapter.zig` | `f9a0b6fa9158ac7055d2d68f39a126800d8724a5c171bb2d35994e60dc3b5676` |
| `ffi/connectors.json` | `33ee987eeb20283676e663bb6eacb5534ecb0da4bec344891aca08e7933be842` |

### Historical pin (the pre-rename revision of record, for audits)

| File @ `2bbd3b2` | sha256 |
|---|---|
| `ffi/connectors.json` | `16878b5b50145d860c9717bb035a001f5841aad20eb682308279e9063a44c15e` |
| `ffi/zig/src/hexadeca.zig` (renamed, see below) | `9e43876ae926567f6e0180a9da83a64bcb07222fde5347ef059a1bb0ecb56f22` |
| `src/Hypatia/ABI/Types.idr` | `b04478e836754334c474e4f09317e88344b95c884cac4fc591bdb50157faf187` |
| `clients/rust/hypatia-client/src/connector.rs` | `6c12e56e4c35954ee5eaab81a72c9f460b71e966364eeb0a9ee5e675702538c8` |

`2bbd3b2` predates the rename: its `ffi/zig/src/hexadeca.zig` is the same
surface now published as `ffi/zig/src/unified-api-adapter.zig`. The rename also
applied the estate identifier mapping inside the files, which is why those
digests differ.

`ffi/connectors.json` **did** change across the rename, contrary to what an
earlier draft of this document asserted. The diff is one line: the advisory
`_note` string was renamed (`hexadeca-connector` → `unified-api-adapter`, and
the guard test it names). **Connector ids, names and ordering are unchanged**,
so the wire contract is identical while the file digest is not. Both digests
are pinned above; do not assume a "documentation-only" rename leaves a file
byte-identical — re-derive it.

## The wire contract (what conformance means here)

From the pinned `ffi/connectors.json` (16 connectors, ids stable):

| id | connector | id | connector |
|----|-----------|----|-----------|
| 0 | grpc | 8 | trpc |
| 1 | graphql | 9 | capnproto |
| 2 | rest | 10 | soap |
| 3 | flatbuffers | 11 | verisimdb-rest |
| 4 | bebop | 12 | bsp |
| 5 | jsonrpc | 13 | scip |
| 6 | websocket | 14 | ipfs |
| 7 | mqtt | 15 | arrow-flight |

`CONNECTOR_COUNT = 16`; `portFor = base + id + 1`. Connector ids, names and the
wire format are byte-identical across the estate — the rename changed *names of
identifiers and paths only*, never ids or wire bytes. **Wire ordering is
load-bearing: do not renumber.**

## How IDApTIK consumes this

IDApTIK mirrors the same layering:

| Layer | IDApTIK |
|---|---|
| ABI (normative, Idris2) | `crates/idaptik-ffi/abi/src/Idaptik/Abi/*.idr` |
| FFI (implementation, Zig) | `crates/idaptik-ffi/zig-adapter/` |
| C boundary | `crates/idaptik-ffi/src/lib.rs`, cbindgen → `include/idaptik.h` |
| BEAM boundary | *none — IDApTIK has no BEAM source and no NIF usage (measured 2026-09-21)* |

Should IDApTIK ever grow BEAM-native code, the estate rule is **supervised
SNIFs rather than raw NIFs** — a raw NIF executes inside the BEAM scheduler,
so a native fault fells the VM and destroys the supervision guarantees that
are the reason to be on the BEAM in the first place. The estate scanner is
`cadastra/tools/estate-migration-toolkit/scripts/find-nif-to-snif.sh`.

The Zig adapter implements IDApTIK's C boundary (8 exports) and its conformance
fixtures pin against the digests above. See `.github/workflows/abi-conformance.yml`
and `crates/idaptik-ffi/zig-adapter/test/conformance_test.zig`.

## What the digest gate does and does not verify

Be precise about this, because the obvious reading is wrong.

**It verifies pin integrity.** Every `check` fetches at a *fixed* hypatia
revision, so it proves three things: the digests recorded in this document are
true of the bytes at that revision; the pinned commits still resolve (a history
rewrite or a force-push that orphaned them fails the gate rather than passing
silently); and this document and the workflow pin the *same* digest set —
whatever its size. The job prints the count it actually compared, so the number
lives in the run output rather than in a sentence here that nothing checks.
That comparison is a real diff of the two files, with an emptiness guard —
two empty sets compare equal, which is how a check like this usually goes
vacuous — and a perturbation control proving the comparison can fail at all.

**It does not track hypatia `main`.** Bytes at a fixed commit are immutable, so
no amount of upstream movement can make this gate go red. If hypatia's ABI
advances, the pins here keep verifying happily against the old revision. Moving
the pins forward is a deliberate human act, and reviewing what changed between
the old and new revisions is the point at which divergence is actually noticed.

## What is prevented upstream, and what is only detected here

These are two different guarantees and this document previously conflated them.

**Upstream, divergence is now prevented rather than detected.** The
16-connector enum is no longer maintained by hand in three places. Since
hypatia #811 (issue #120), `ffi/connectors.json`,
`ffi/zig/src/connector_generated.zig` and
`clients/rust/hypatia-client/src/connector_generated.rs` are all emitted from
`src/Hypatia/ABI/Types.idr` by the Idris2 generator `hypatia-abi-gen`, and the
`abi-codegen-drift` workflow re-runs that generator into a temporary directory
on every pull request and diffs the result against the tracked files. It has no
`paths:` filter, so it cannot be dodged by touching only the generated files,
and it prints its denominator (`connectors emitted: N`, `files compared: M of
3`) so a generator that silently emitted nothing fails instead of reading
green. It is a required check on hypatia `main`.

Three further pins fire independently of that gate, each by a different
mechanism, which is what makes the arrangement more than four copies agreeing
with each other:

- `connectorCount = Refl` — an Idris2 proof, checked at compile time.
- `count_is_sixteen` and `wire_ids_are_stable` in the Rust client, run by
  `cargo test --workspace` on every PR.
- the `comptime` 16-connector assertion in `unified-api-adapter.zig`, now
  reachable: `ffi/zig/build.zig` roots that file as a test target and the
  `zig test (wire contract)` job runs it. Before hypatia #811 the adapter was
  not a test root anywhere, so that assertion never executed — the "independent
  Zig pin" this document used to imply did not exist at the time it was written.
- `test/unified-api-adapter-contract_test.exs` scrapes the names out of the
  *live* `src/Hypatia/ABI/Types.idr` by regex and compares them to the generated
  files — deliberately a different mechanism from evaluating the ABI, so it can
  catch a bug in the generator itself.

What is **not** prevented, and should not be described as impossible:
`CONNECTOR_COUNT` in the generated Rust is a literal fixed at generation time.
It is correct for as long as the generated file is in step with the ABI, which
is the drift gate's job — but it is not, in itself, derived at compile time the
way the Idris2 `Refl` and the Zig `comptime` assertion are.

**Here, divergence is still only detected — and only on demand.** Nothing in
the paragraphs above is a property of *this* repository. IDApTIK holds digests
of a fixed hypatia revision; as recorded above, that gate cannot go red when
hypatia moves, because bytes at a fixed commit are immutable. Moving the pin
remains a deliberate human act, and the upstream generation machinery makes
that act *easier to get right* — the ABI and its generated outputs move
together or the upstream gate rejects them — without making it automatic. The
control here is still this document, the surface check and the digest gate.

One thing neither gate can catch: the digest comparison only diffs 64-hex
strings, so the human-readable wire table above is unverified prose. A wrong
connector *name* there passes every check indefinitely. One such error
(`verismbd-rest` for `verisimdb-rest`, id 11) was found by reading and fixed in
the same change that moved this pin; the other fifteen names were checked
against the pinned `ffi/connectors.json` and are correct. Machine-checking that
table against the JSON is tracked as a follow-up issue.
