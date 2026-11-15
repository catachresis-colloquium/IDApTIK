<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# Enaction documents from the headless corridor

The `enaction-runner-output/1` and `enaction-mind-trace/1` documents that
`idaptik-weevil-replay` writes for twelve corridor runs, committed for consumers
that can fetch this repository but cannot build the Evil Weevil kernel.

They are written by the binary, never by hand. `tests/golden.rs` fails if any
file differs from what the built binary writes today, or if `SHA256SUMS` does
not match the complete sorted file set.

## Provenance

- Kernel: catachresis-colloquium/evil-weevil at `1f68271` (ABI 1.2,
  evil-weevil ADR-0014, draft pull request 11), built with Zig 0.16.0. The
  ten stage 1 documents, first written on `c655f46` (ABI 1.1) are byte-identical on
  it except for the kernel version they name: `manifest.source` (and so
  `manifest.digest`) and the mind trace's `producer.abi`.
- Validator: catachresis-colloquium/enaction-engine `main` at `81fa3ed`,
  `enaction-validate`. Every document here exits 0 from it.
- Policies: `../enaction-policies.json` (`depth.max_order` 0) for the stage 1
  runs, and `../enaction-policies-depth-1.json` (the same status policy,
  `depth.max_order` 1) for the six model runs, whose runner output holds
  order-1 beliefs. Under the depth-0 file a model scenario's runner output is
  refused as `nesting-bounded`; its control holds no model and is accepted
  by either.

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
    the-stalker the-stalker-control caught caught-control false-exposure false-exposure-control; do
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
enaction-validate --policies crates/idaptik-weevil/tests/fixtures/enaction-policies-depth-1.json \
  --mind-trace "$D/false-exposure.mind-trace.json" < "$D/false-exposure.runner-output.json"
```

## The model scenarios (ABI 1.2)

Each of `the-stalker`, `caught` and `false-exposure` has one contact,
`contact-41`, and its runner output holds, for the proposition
`guard-0 in room-0`, four separate facts: the world (`world_truth`), what
`contact-41` believes (order 0, `bearer` `contact-41`), what `guard-0` thinks
it believes (order 1, `holders` `["guard-0"]`), and, for a reader that compares
the two over each model's lifetime, whether `guard-0` is right about it. A
model is stated only where the kernel's exposure changes. Each control
withholds `CAP_OBSERVATION_CUES`, so the kernel makes no claim and its runner
output holds the contact's own beliefs only. See `src/epistemic.rs`, "The
guard's model of a contact", and `src/world.rs`, "What a contact knows".
