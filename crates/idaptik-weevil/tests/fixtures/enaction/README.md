<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# Enaction documents from the headless corridor

The `enaction-runner-output/1` and `enaction-mind-trace/1` documents that
`idaptik-weevil-replay` writes for twelve corridor runs (22 documents), committed for
consumers that can fetch this repository but cannot build the Evil Weevil
kernel. Six of them are slice 2, "it models you": runs through the ABI 1.2
model record, whose runner outputs hold models (order-1 beliefs).

They are written by the binary, never by hand. `tests/golden.rs` fails if any
file differs from what the built binary writes today, or if `SHA256SUMS` does
not match the complete sorted file set.

## Provenance

- Kernel: catachresis-colloquium/evil-weevil `marina/weevil-c1-mind-slice-2`
  at `f7c333d` (ABI 1.2, evil-weevil ADR-0014; pull request 11, not merged),
  built with Zig 0.16.0. The ten documents of the runs before slice 2 were first
  written on `main` at `c655f46` (ABI 1.1). On the 1.2 kernel they differ from
  those bytes in the kernel's version label only (`producer.abi`,
  `manifest.source`, and `manifest.digest`, which covers `source`): with the
  label put back to 1.1 and the digest recomputed, each is byte-identical to
  its 1.1 predecessor.
- Validator: catachresis-colloquium/enaction-engine `main` at `03cb27b`,
  `enaction-validate`, for the six runs before slice 2. The six model runs were checked
  with `enaction-validate` built from pull request 112 (`marina/weevil-c0-models`,
  not merged) at `0a67878`, the contract of `conformance/tom/v1` and ADR-0028;
  those six runs' documents exit 0 from that build too. Every document here exits 0.
- Policies: `../enaction-policies.json` (`depth.max_order` 0) for the six
  runs before slice 2, and `../enaction-policies-models.json` (the same status policy,
  `depth.max_order` 1) for the six model runs. Under the depth-0 file a model
  run's runner output is refused as `nesting-bounded`.

## Files

| Profile | Runner output | Mind trace |
|---|---|---|
| `decoy-lure` | `decoy-lure.runner-output.json` | none (a run through `ee_tick`) |
| `cautious-sentry` | `cautious-sentry.runner-output.json` | none (a run through `ee_tick`) |
| `the-grudge` | `the-grudge.runner-output.json` | `the-grudge.mind-trace.json` |
| `the-grudge-control` | `the-grudge-control.runner-output.json` | `the-grudge-control.mind-trace.json` |
| `the-flinch` | `the-flinch.runner-output.json` | `the-flinch.mind-trace.json` |
| `the-flinch-control` | `the-flinch-control.runner-output.json` | `the-flinch-control.mind-trace.json` |
| `the-stalker` | `the-stalker.runner-output.json` | `the-stalker.mind-trace.json` |
| `the-stalker-control` | `the-stalker-control.runner-output.json` | `the-stalker-control.mind-trace.json` |
| `caught` | `caught.runner-output.json` | `caught.mind-trace.json` |
| `caught-control` | `caught-control.runner-output.json` | `caught-control.mind-trace.json` |
| `false-exposure` | `false-exposure.runner-output.json` | `false-exposure.mind-trace.json` |
| `false-exposure-control` | `false-exposure-control.runner-output.json` | `false-exposure-control.mind-trace.json` |

`the-stalker-control.mind-trace.json` and `caught-control.mind-trace.json` are
the same bytes: without `CAP_OBSERVATION_CUES` the kernel ignores the cue, so
the guard's mind does not differ, as evil-weevil ADR-0014 measured for its own
fixtures. Their runner outputs differ, because the world still records the
contact's glance and the contact's own belief.

A mind trace's `caused_by` ids name events of the runner output with the same
profile, which comes from the same run.

## Commands

From the repository root, with the kernel built (`scripts/weevil_kernel.sh build`):

```sh
D=crates/idaptik-weevil/tests/fixtures/enaction
B=target/debug/idaptik-weevil-replay
cargo build --locked -p idaptik-weevil --bin idaptik-weevil-replay
for p in decoy-lure cautious-sentry; do
  "$B" --profile "$p" --ticks 200 --seed 7 --emit-epistemic "$D/$p.runner-output.json"
done
for p in the-grudge the-grudge-control the-flinch the-flinch-control \
         the-stalker the-stalker-control caught caught-control \
         false-exposure false-exposure-control; do
  "$B" --profile "$p" --ticks 200 --seed 7 \
    --emit-epistemic "$D/$p.runner-output.json" --emit-mind-trace "$D/$p.mind-trace.json"
done
(cd "$D" && find . -maxdepth 1 -type f ! -name SHA256SUMS -printf '%f\n' | LC_ALL=C sort \
  | xargs sha256sum > SHA256SUMS)
```

To check one pair with the validator:

```sh
enaction-validate --policies crates/idaptik-weevil/tests/fixtures/enaction-policies.json \
  --mind-trace "$D/the-grudge.mind-trace.json" < "$D/the-grudge.runner-output.json"
enaction-validate --policies crates/idaptik-weevil/tests/fixtures/enaction-policies-models.json \
  --mind-trace "$D/false-exposure.mind-trace.json" < "$D/false-exposure.runner-output.json"
```
