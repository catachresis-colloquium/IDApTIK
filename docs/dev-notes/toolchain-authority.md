# Toolchain authority: three files, one job each

Status: describes the tree as it stands (2026-10-25). Enforced by
`scripts/toolchain-parity.sh`, which is a check with eight mutants — see
[Proving the check](#proving-the-check).

## The problem this replaces

Before this note, three files each claimed to describe the development
environment, and they disagreed:

| File | Claimed | Actually said |
|---|---|---|
| `guix.scm` | the environment | rust, zig, just, elixir, erlang — **no Nickel**, no container runtime, and a bare `rust` that shadows the pinned compiler |
| `mise.toml` | tool versions | erlang, elixir, zig, just, nickel, trufflehog — **no Rust**, by design |
| `rust-toolchain.toml` | Rust | 1.95.0 + components + wasm32 |

Nothing said which of them won. A reader could reasonably conclude the
repository used a Rust that might be any of three versions.

## The rule

**One authority per decision, and the environment is not a version pin.**

```
guix.scm             THE ENVIRONMENT      system libraries, C toolchain, the
                                           tools themselves, the container runtime
mise.toml            THE VERSIONS         every non-Rust tool
rust-toolchain.toml  THE RUST VERSION     the compiler, its components, its targets
```

Guix is the environment policy — nix is retired, and this is the only place the
environment is declared. It is deliberately *not* a second opinion about
versions: Guix's `nickel` is whatever the Guix revision in use carries, and
`mise.toml` is what `just` recipes actually invoke.

### Rust is provided as `rustup`, never as `rust`

This is the one non-obvious line in `guix.scm`, and it is load-bearing.

A `guix shell -f guix.scm` that provided Guix's `rust` package would put a
compiler on `PATH` that is older than `rust-toolchain.toml`'s pin — and worse,
it would *look* correct, because `rustc` would work. rustup, by contrast, reads
`rust-toolchain.toml` and installs exactly the pinned compiler, so the Guix
shell and a mise machine agree by construction.

`scripts/toolchain-parity.sh` asserts both halves: `rustup` present, bare `rust`
absent.

### Where the versions can still drift

| Tool | Authority | Guix shell | mise machine |
|---|---|---|---|
| Rust | `rust-toolchain.toml` | 1.95.0 (via rustup) | 1.95.0 (via rustup) |
| Nickel, Zig, just, Elixir, Erlang | `mise.toml` | Guix revision's version | pinned version |

The second row is honest drift, not a defect to hide: Guix's Nickel may lag
1.16.0. `scripts/doctor.sh` asserts the *pinned* versions and is therefore the
place that drift becomes visible — in a Guix shell, `just doctor` will say the
pinned Nickel is not the one on `PATH`, and that is the correct answer rather
than a silently different config language.

If you need the exact pins inside a Guix shell, run `mise install` there too;
mise's copies win on `PATH` and `guix.scm` has still given you the libraries,
the C toolchain and everything that is expensive to build.

## Deliberately absent from `guix.scm`

| Tool | Why | Where it comes from |
|---|---|---|
| Idris2 | bootstraps through Chez Scheme + GMP; not a day-to-day dependency | `just install-idris2` |
| trufflehog | a Go binary for the pre-push secret scan, not a build input | `scripts/install-git-hooks.sh` |
| Bevy's system libraries via sudo | a Guix shell does not escalate | `guix.scm` provides them directly; `just bevy-linux-deps` is the Debian path |

Each is excused **by name, with a reason, inside the parity script**. Adding a
tool to that list is a decision, and the reason is the record of it.

## Containers

`guix.scm` provides `podman`, and `.devcontainer/` is runtime-agnostic so it
builds identically under Podman and Docker.

Podman is the default because it is rootless and daemonless: a dev container
that requires a root daemon on the host is a larger security surface than the
thing it is developing. The `Containerfile` accordingly runs as `nonroot` and
refuses to escalate — Bevy's system libraries are a host concern (Guix, or
`just bevy-linux-deps` under sudo), not a container concern.

Three ways in, in order of preference:

```sh
guix shell -f guix.scm                      # the environment, no container
guix shell -f guix.scm -C --network         # ...in a rootless container
podman build -t idaptik-dev -f .devcontainer/Containerfile .
```

## Proving the check

```sh
scripts/toolchain-parity.sh              # the real tree: 25 controls
scripts/toolchain-parity.sh --self-test  # 7 controls + 8 mutants
```

Mutants plant one defect each and require the check to fail: Nickel removed from
`guix.scm`, `rustup` removed, a bare `rust` added, `podman` removed, an unserved
tool added to `mise.toml`, `doctor.sh` no longer asserting Nickel, the
`Containerfile` emptied, and the Rust channel deleted.

The last-but-one earned its keep immediately. The original
`Containerfile`-has-a-`FROM` control had a `pass` branch and no `fail` branch, so
an empty file produced neither — a check that could not fail, in a script written
specifically to catch checks that cannot fail. The mutant is now permanent.

## What this does not check

- It does not predict which version Guix's packages carry. That depends on the
  Guix revision and would be wrong on most machines; a guess presented as a
  check is the defect `docs/DOCUMENTATION-CLAIMS.md` exists to prevent.
- It does not run `guix`. It checks the *declaration* against the other two
  declarations. The Guix expression itself is verified by whoever next runs
  `guix shell -f guix.scm`, and until then it is marked unverified here rather
  than assumed good.
