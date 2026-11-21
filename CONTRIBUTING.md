# Contributing to IDApTIK

Thank you for contributing. Please read `CODE_OF_CONDUCT.md`, `GOVERNANCE.md`,
and `AGENTS.md` before making changes.

## Development setup

```sh
git clone https://github.com/catachresis-colloquium/IDApTIK.git
cd IDApTIK
mise trust
just setup
just doctor
```

Rust is pinned by `rust-toolchain.toml`; Erlang, Elixir, Zig, Nickel, and
`just` are pinned by `mise.toml`. TruffleHog is also pinned there, and
`just setup` activates the repository's tracked Git hooks. For an existing
clone, install the added hook with:

```sh
mise install
just install-git-hooks
```

The `pre-push` hook scans the commits about to be sent to the remote and
blocks the push if TruffleHog is unavailable, the scan cannot complete, or a
verified/unverifiable secret is found. It does not scan only the worktree:
committed content is checked before it leaves the machine. Run a complete
local-history scan at any time with `just secret-scan`.

## Project boundaries

- Gameplay truth belongs in `crates/idaptik-core` and must remain deterministic
  and independent of Bevy.
- `crates/idaptik-bevy` is a presentation/input adapter over typed
  `Command`/`Event` and snapshot surfaces.
- `crates/idaptik-net` carries the Phoenix Channels client over burble's
  game-session fabric; multiplayer session relaying lives in burble.
- IDApTIK owns `contracts/idaptik/v1`; Universal Modding Studio consumes it
  without becoming a runtime dependency.
- Ruby, Python, JavaScript, and their package/tooling ecosystems are not part
  of this repository.
- Do not flatten the licence layers: code is AGPL-3.0-or-later, content is
  CC-BY-SA-4.0, and the IDApTIK/Moletaire names and marks remain trademarked.

Changes to `crates/idaptik-ffi`, `crates/idaptik-net`, workflows, licences, or versioned
contracts receive additional scrutiny.

## Checks

Run the gates relevant to your change. Before requesting merge, the baseline
set is:

```sh
just test-ghost
just config-check
just loopback-check
cargo test -p idaptik-bevy
cargo clippy -p idaptik-bevy --all-targets -- -D warnings
```

Checks must fail when required tooling or validation is absent; do not turn a
missing tool into a successful skip.

## Commits and pull requests

Contributions are made under Developer Certificate of Origin 1.1. Sign every
commit:

```sh
git commit -s
```

Use a focused branch and pull request. Explain the user or developer impact,
the relevant architectural boundary, and the commands used to verify the
change. Major or breaking changes require an ADR or RFC under `docs/`.
