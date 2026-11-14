<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# Enaction documents from the headless corridor

The `enaction-runner-output/1` and `enaction-mind-trace/1` documents that
`idaptik-weevil-replay` writes for eight corridor runs, committed for consumers
that can fetch this repository but cannot build the Evil Weevil kernel.

They are written by the binary, never by hand. `tests/golden.rs` fails if any
file differs from what the built binary writes today, or if `SHA256SUMS` does
not match the complete sorted file set.

## Provenance

- Kernel: catachresis-colloquium/evil-weevil `main` at `c655f46` (ABI 1.1,
  evil-weevil ADR-0013), built with Zig 0.16.0.
- Validator: catachresis-colloquium/enaction-engine `main` at `03cb27b`,
  `enaction-validate`. Every document here exits 0 from it.
- Policies: `../enaction-policies.json`.

## Files

| Profile | Runner output | Mind trace |
|---|---|---|
| `decoy-lure` | `decoy-lure.runner-output.json` | none (a run through `ee_tick`) |
| `cautious-sentry` | `cautious-sentry.runner-output.json` | none (a run through `ee_tick`) |
| `the-grudge` | `the-grudge.runner-output.json` | `the-grudge.mind-trace.json` |
| `the-grudge-control` | `the-grudge-control.runner-output.json` | `the-grudge-control.mind-trace.json` |
| `the-flinch` | `the-flinch.runner-output.json` | `the-flinch.mind-trace.json` |
| `the-flinch-control` | `the-flinch-control.runner-output.json` | `the-flinch-control.mind-trace.json` |

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
for p in the-grudge the-grudge-control the-flinch the-flinch-control; do
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
```
