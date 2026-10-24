# Toolchain authority: one provider per tool

Status: current policy for the development environment.
Last corrected: 2026-10-25, after the first version of this file turned out to
describe an environment that could not be built.

## The problem this replaces

IDApTIK had four places that each described the toolchain, and they disagreed:

| File | It claimed |
| --- | --- |
| `guix.scm` | the environment, including `rust` |
| `mise.toml` | tool versions, no Rust |
| `rust-toolchain.toml` | the Rust version |
| `scripts/install-idris2.sh` | Idris2, which none of the above provide |

The failure was not that they disagreed on a version. It was that `guix.scm`
provided a bare `rust` package, which puts Guix's compiler ahead of the pinned
one on `PATH`. Everything would still work — `rustc` would report a version —
and the build would quietly stop being the one `rust-toolchain.toml` names.
A wrong answer that looks like a working one is the worst kind.

## The correction (2026-10-25)

The first version of this policy said "Guix provides the environment, including
`rustup`, `nickel`, `just`, `zig`, `elixir` and `erlang`; `mise.toml` records
what those should be." That is not true, and it was never checkable here because
no Guix daemon was available. Checked against `packages.guix.gnu.org`:

| Package | In Guix? | Version there |
| --- | --- | --- |
| `mise` | **yes** | 2026.4.4 |
| `podman` | **yes** | 6.1.4 |
| `zig` | **yes** | 0.16.0, 0.15.2, **0.14.1**, 0.13.0 … |
| `elixir` | **yes** | 1.19.5 |
| `just` | **yes** | 1.59.0 |
| `nickel` | **no** | — |
| `rustup` | **no** | — |

Two of the packages the policy named do not exist, so `guix shell -f guix.scm`
would have failed on the first `specification->package` call. The policy was
describing a machine nobody had built.

Note also `zig`: `mise.toml` pins **0.14.0** and Guix ships **0.14.1**. Had both
been providers, they would have disagreed by a patch release on day one.

## The rule

**One authority per decision, and one provider per tool.**

```
guix.scm             THE ENVIRONMENT   C toolchain, system libraries, podman,
                                       and mise itself
mise.toml            THE VERSIONS      every versioned tool, exactly one version
rust-toolchain.toml  THE RUST VERSION  compiler, components, targets
scripts/install-*    THE BOOTSTRAP     what no package manager provides
```

The load-bearing sentence: **`guix.scm` provides `mise` and then stops.**

It does not also provide Nickel, just, Zig, Erlang or Elixir — even though Guix
has four of those five. Providing a tool from both places gives it two versions
on `PATH` and no rule for which wins. That is the same defect as the bare
`rust`, just quieter: the build works, and the version you tested against is not
necessarily the version you get.

So `mise install` is the only thing that puts a versioned tool on `PATH`, and
`mise.toml` is the only thing that says which version. Guix's job is the parts
mise cannot do: a C toolchain, `alsa-lib`, `wayland`, `vulkan-loader`,
`libxkbcommon`, the X libraries, fonts, and the container runtime.

### Rust is provided as `rustup`, never as `rust`

`rustup` is not a Guix package, so it cannot be an input to the environment that
would provide it. It is bootstrapped by `scripts/install-rustup.sh`, which
installs with `--no-modify-path` and `--default-toolchain none`, then lets
`rustup show` resolve `rust-toolchain.toml`.

The invariant is now easier to state than before, because it is a statement
about absence: **Guix provides no Rust at all here, so a Guix shell cannot
shadow the pin.** The script's only job is to make the pin real.

`--no-modify-path` is a deliberate choice, not caution. An installer that edits
`~/.bashrc` makes the toolchain ambient — present whether or not you entered the
environment — which is a second, invisible authority. The environment should be
entered explicitly (`guix shell`, `mise`, a devcontainer), never inherited.

## What still drifts

Nothing versioned drifts any more, because there is only one provider. What
remains is a bootstrap-order fact worth knowing:

| Situation | What you get |
| --- | --- |
| Fresh `guix shell -D -f guix.scm` | libraries, C toolchain, podman, mise — **no `just`, no `zig`, no `nickel`, no `rustc` yet** |
| …then `mise install` | exactly the versions in `mise.toml` |
| …then `just install-rustup` | `rustc 1.95.0`, clippy, rustfmt, wasm32 |
| …then `just doctor` | all of the above, asserted rather than assumed |

That first row is the one that surprises people. It is correct behaviour: the
shell has no tools because the version authority has not run yet.

`just doctor` is where a missing step becomes visible, and it is the only place
worth checking. A doctor that passes on a bare Guix shell would be lying.

## Deliberately absent from `guix.scm`

| Not there | Why | Where it comes from instead |
| --- | --- | --- |
| `rust` | would shadow `rust-toolchain.toml` | `scripts/install-rustup.sh` |
| `rustup` | not packaged in Guix | `scripts/install-rustup.sh` |
| `nickel` | not packaged in Guix | `mise.toml` |
| `just`, `zig`, `elixir`, `erlang` | packaged, but `mise.toml` is the version authority | `mise.toml` |
| `idris2` | bootstraps through Chez Scheme + GMP | `scripts/install-idris2.sh` |
| `trufflehog` | a Go binary used only by the pre-push scan | `scripts/install-git-hooks.sh` |
| Bevy's system libraries via `sudo` | a Guix shell does not escalate | `guix.scm` provides them directly; `just bevy-linux-deps` is the Debian path |

## Containers

`.devcontainer/` is runtime-agnostic: Wolfi base, `nonroot`, and it installs
neither Guix nor mise's tools beyond the bootstrap. Podman is the estate default
(rootless, and `guix.scm` provides it); Docker works because nothing in the
Containerfile asks which runtime is building it.

The Containerfile deliberately **refuses to escalate** for Bevy's system
libraries. Those are a host concern — Guix, or `just bevy-linux-deps` under
sudo — not a container concern, and a container that installs them is a
container that no longer matches the host it claims to reproduce.

## Proving the check

`scripts/toolchain-parity.sh` enforces the rule structurally — who provides
what, never which version. Its self-test plants ten defects and requires the
check to kill each:

| Mutant | Killed by |
| --- | --- |
| `guix.scm` stops providing `mise` | bridge control |
| `guix.scm` also provides `nickel` | single-provider control |
| `guix.scm` provides a bare `rust` | Rust-authority control |
| `guix.scm` stops providing `podman` | container control |
| `mise.toml` stops pinning `nickel` | doctor cross-check |
| `doctor.sh` stops asserting `nickel` | doctor cross-check |
| the Containerfile empties | container control |
| `rust-toolchain.toml` loses its channel | Rust-authority control |
| `install-rustup.sh` empties | bootstrap control |
| `install-rustup.sh` may modify PATH files | `--no-modify-path` control |

Two of those mutants exist because the *previous* version of this script could
not fail: one control had a `pass` branch and no `fail` branch, so an empty
Containerfile was accepted by the script whose purpose was catching exactly
that. A check that cannot fail is worse than no check, which is why every
control here is paired with a mutant rather than trusted.

## What this does not check

- **It does not run Guix.** No Guix daemon has evaluated `guix.scm`. Package
  *names* were checked against `packages.guix.gnu.org` on 2026-10-25, which
  proves the packages exist; it does not prove this expression parses, resolves
  or builds. That check is the first thing to run on a machine with Guix, and
  until someone does, this file's claims about `guix.scm` are evidence-backed
  but not demonstrated.
- **It does not predict versions.** Deliberate: a check that guessed which
  version a Guix revision carries would be wrong on most machines, and a guess
  presented as a check is the defect `docs/DOCUMENTATION-CLAIMS.md` exists to
  prevent.
- **It does not check `mise install` works.** `just doctor` does that at runtime,
  per machine.
