<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# Enaction documents from the headless corridor

The `enaction-runner-output/2` and `enaction-mind-trace/1` documents that
`idaptik-weevil-replay` writes for twelve corridor runs (22 documents), committed for
consumers that can fetch this repository but cannot build the Evil Weevil
kernel. Six of them are slice 2, "it models you": runs through the ABI 1.2
model record, whose runner outputs hold models (order-1 beliefs).

They are written by the binary, never by hand. `tests/golden.rs` fails if any
file differs from what the built binary writes today, or if `SHA256SUMS` does
not match the complete sorted file set.

## Kernel contract 2.0 is a draft

The runner outputs follow Enaction kernel contract 2.0 (Enaction ADR-0030):
the tag `enaction-runner-output/2`, `manifest.kernel_version` 2.0,
`manifest.profile.targets_kernel` 2.0, and `manifest.migration_from` 1.0, as
every package of Enaction's `conformance/tom/v2` keeps it. The policies keep
the tag `enaction-policies/1` and the mind traces keep
`enaction-mind-trace/1`.

At enaction-engine `main` `11bd247`, 2.0 is a draft: `enaction-validate
--version` prints `enaction-validate kernel contract 2.0 (draft)`. A draft may
still change its shape and rules with no further bump, and nothing may be
published as conforming to it. These goldens track the draft at `11bd247`.
They are not a claim of conformance to a released contract.

A supersession link is `{belief, because}`: the belief replaced and the event
that caused the revision. The emitter names the first event the successor
already cites:

- the guard's model, on a cue: the guard's observation `contact-41 observes
  guard-0 at tick T` (`caught`, `false-exposure`);
- the guard's model, on its own attack: the action outcome `guard-0 attacked
  contact-41 at tick T` (`the-stalker`);
- the guard's model, on expiry: the first event of the guard's perception on
  that tick (`false-exposure`, tick 112);
- the contact's own belief, when its memory ends: its observation `contact-41
  does not observe guard-0 at tick T` (`false-exposure` and
  `false-exposure-control`, tick 52);
- the contact's own belief, when it comes to know again: its sight of the
  guard, or else the blow it took. No committed run has such a link.

Each cause meets the six rules of ADR-0030. `tests/kernel_contract_2.rs`
checks them, and checks `belief-key-chained` (ADR-0029), from the contract
text.

The move from contract 1.0 changed every runner output and no mind trace.
Each runner output changed its tag, its two kernel versions and its digest.
The four with a link (`caught`, `the-stalker`, `false-exposure` and
`false-exposure-control`) also changed each link from a bare id to
`{belief, because}`. Nothing else in any document changed, and event ids are
the same, so every `caused_by` in a mind trace still names the same event.

## Provenance

- Kernel: catachresis-colloquium/evil-weevil, a checkout whose `src` and
  `include` equal `main` at `db850ed` (ABI 1.2), built with Zig 0.16.0. The
  slice 2 documents were first written on `marina/weevil-c1-mind-slice-2` at
  `f7c333d` (pull request 11). The ten documents of the runs before slice 2
  were first written on `main` at `c655f46` (ABI 1.1). On the 1.2 kernel they
  differ from those bytes in the kernel's version label only (`producer.abi`,
  `manifest.source`, and `manifest.digest`, which covers `source`).
- Validator: catachresis-colloquium/enaction-engine `main` at `11bd247`,
  `enaction-validate` (kernel contract 2.0, draft). Every document here exits
  0 from that build, with the policies below and with its mind trace where
  it has one. No validator of contract 1.0 was run on them.
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

## What a contact knows

The contact's own belief follows the world's rule, in `src/world.rs`: a
scripted contact has `memory`, a count of ticks (default 0), and it knows
where the guard is on tick `t` exactly when a tick `s` exists with
`t - memory <= s <= t` on which it observed the guard, or the guard's attack
on it was applied. In the six model runs contact 41 has a memory of 30 ticks.

The memory changed three runner outputs and nothing else:

- `caught.runner-output.json`: the contact glances at 101 and still knows
  when the blows land from 116, so its belief is `certain` from 101 to the
  end. Before, it was `unknown` on 102 to 115 and the guard's model read as
  stale.
- `false-exposure.runner-output.json` and
  `false-exposure-control.runner-output.json`: the contact's belief turns
  `unknown` on 52, the first tick after its memory ends, instead of 22. The
  guard's model still holds until 112, so it agrees on 21 to 51 and diverges
  from 52.

The kernel learns nothing of a contact's memory, so every mind trace is
byte-identical to its bytes before the change. `tests/golden.rs` pins them.

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
