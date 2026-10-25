# Toolchain setup — system-wide, and the permanent fix for mise in new repos

This note covers three things: how to put every IDApTIK toolchain on the machine
so all users get it, the permanent fix for the mise problems that show up in
every new repository, and the in-repo protection mechanism (`mise trust`).

The design is ADR-0001's: `mise` pins the non-Rust toolchains, `rustup` keeps
Rust via `rust-toolchain.toml`, and Idris2 is provisioned out-of-band because it
bootstraps through Chez Scheme.

## 1. The permanent mise fix

Three layers. Do all three once; after that, new repos just work.

### 1a. One shared mise install, visible to every user

Install mise once, point it at a shared data directory, and activate it from a
`profile.d` script so login shells for every account pick it up:

```sh
sudo npm install -g mise            # or: curl https://mise.run | sh
sudo mkdir -p /opt/mise-data
cat <<'EOF' | sudo tee /etc/profile.d/mise.sh
# mise: shared, system-wide toolchain manager (all users, one tool cache)
export MISE_DATA_DIR=/opt/mise-data
export MISE_BIN_PATH=/opt/mise-data/shims
eval "$(mise activate bash)"
EOF
sudo chmod 644 /etc/profile.d/mise.sh
sudo chmod -R a+rX /opt/mise-data

# prove it as someone else
sudo -u nobody bash -lc 'command -v mise && mise --version'
```

`MISE_DATA_DIR` is the part that makes it *shared*: tools installed by one user
land in `/opt/mise-data` and are immediately visible to the next one, instead of
each account building its own copy under `~/.local/share/mise`.

For fish, the activation line is `eval (mise activate fish)`; for zsh,
`eval "$(mise activate zsh)"`.

### 1b. Trust each repository once — this is the in-repo protection

```
$ mise trust --help
Mark a config file as trusted

Trust lets mise load the parts of a config file that can run code or change
your environment. Without it, mise prompts, skips the file, or fails with an
untrusted-config error when it cannot prompt.
```

That is the thing: **`mise trust`**. A `mise.toml` is not data — it can run
arbitrary shell via `[env]` templates and `[tasks]`, so mise refuses to load the
parts that can execute until you mark the directory trusted. That is deliberate
protection against a cloned repository shipping a hostile config.

`mise trust` is per-path and remembered, so it is a one-time action per repo:

```sh
cd ~/IDApTIK && mise trust      # "mise trusted ~/IDApTIK"
```

Note the asymmetry in the help text: commands that run project code
(`mise run`, `mise install`, `mise exec`, `mise build`, `mise watch`) trust their
active config *automatically*. So `mise install` works without an explicit
`mise trust`; it is the shell-activation path that needs it. `just setup` runs
`mise trust` first precisely to cover that.

Set `paranoid = true` if you want trust required even for configs that contain
only plain version strings.

### 1c. Make new repos self-provision — the SessionStart hook

ADR-0001 records this as the deliberate opt-in:

> Wiring `scripts/bootstrap.sh` into a SessionStart hook for web sessions is
> left as an opt-in the maintainer can add deliberately.

`scripts/bootstrap.sh` already does the right thing: it ensures mise is present,
trusts the repo, installs the *fast prebuilt* tools (Zig, Just, Nickel) and the
pinned Rust toolchain, and deliberately skips the slow Erlang/Elixir source
builds. Wiring it into a hook means a freshly cloned repo provisions itself the
first time a session starts, instead of every contributor reinventing it.

For Claude Code, that is a `SessionStart` entry in `~/.claude/settings.json`:

```json
{
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "bash -lc 'cd \"$REPO_ROOT\" && bash scripts/bootstrap.sh'"
          }
        ]
      }
    ]
  }
}
```

Substitute the hook shape for whatever agent or shell you actually use; the
point is that `scripts/bootstrap.sh` is the single idempotent entry point.

### Other mise entry points worth knowing

| Command | What it is |
| --- | --- |
| `mise doctor` | Checks the mise setup itself for problems. |
| `mise bootstrap` | Sets up the machine from your mise config. |
| `mise generate` | Generates CI workflows, git hooks, install scripts, task stubs. |
| `mise config ls` | Lists the config files mise is loading, lowest to highest precedence. |
| `mise settings set KEY VALUE` | Writes a setting to the **global** config; `--local` writes to the project. |

Global settings live in `~/.config/mise/config.toml`. The ones that matter here
are `auto_install` (default `true` — install missing tools on activation),
`paranoid` (default `false`), `yes`, `verbose`, `activate_aggressive`, `pin`.

## 2. System-wide Idris2

Idris2 is deliberately *not* in `mise.toml`: it bootstraps through Chez Scheme +
GMP, so forcing it into `mise install` would break machines without Chez. It is
provisioned on demand:

```sh
cd ~/IDApTIK
sudo apt-get install -y chezscheme libgmp3-dev build-essential
just install-idris2
# which runs scripts/install-idris2.sh → idris2-pack
# then ensure ~/.pack/bin is on PATH
echo 'export PATH="$HOME/.pack/bin:$PATH"' | sudo tee -a /etc/profile.d/idris2.sh
```

For a shared install, set `IDRIS2_PREFIX` to a world-readable location when
bootstrapping so every user sees the same compiler.

## 3. System-wide Elixir and Erlang

These are in `mise.toml` and are built **from source on first install** — the
justfile says so explicitly, and it takes a while. Once built they are cached in
`MISE_DATA_DIR`, so with the shared directory above the second user pays nothing:

```sh
cd ~/IDApTIK
mise trust
mise install erlang elixir
mise exec erlang@28.3.1 -- erl -eval 'io:format("~s",[erlang:system_info(otp_release)]), halt().' -noshell
mise exec elixir@1.19.5-otp-28 -- elixir --version
```

`just doctor` is the acceptance check — it verifies the pinned version of every
toolchain and names exactly what is missing.

## 4. The full one-shot

From a fresh clone, on Debian/Ubuntu/WSL2:

```sh
./install.sh              # rustup, Rust, mise tools, Bevy libs (sudo), build, doctor
just bevy-linux-deps      # Bevy's system libraries on Linux
just install-idris2       # Idris2, if you want the ABI model gate
just doctor               # everything present and at the pinned version?
```

`just runtime-doctor` is the cheaper check: it covers only what a player needs to
launch and deliberately does not require the FFI/config development toolchains.

## 5. What this sandbox could and could not do

Recorded so the commands above are not taken on faith:

* **mise** — installed system-wide from the npm package (`jdx/mise`, SLSA
  provenance), shared data dir `/opt/mise-data`, activated via
  `/etc/profile.d/mise.sh`. Verified as an unprivileged user.
* **`mise trust`** — run and confirmed: `mise trusted ~/IDApTIK`.
* **`mise install erlang elixir`** — **failed here**, and not because of the
  commands. mise needs `mise-versions.jdx.dev`, `raw.githubusercontent.com`
  (for `kerl`) and `builds.hex.pm`, all of which this sandbox blocks. Building
  from source is also impossible here: `autoconf`, `m4` and the ncurses headers
  are absent and `deb.debian.org` is unreachable.
* **Idris2** — **not installed here**. `scripts/install-idris2.sh` fetches
  `idris2-pack` from `raw.githubusercontent.com`, which is blocked.
* **Erlang/Elixir** — **not installed here**, for the reasons above.

So the commands in §2 and §3 are written from the project's own scripts and
ADR-0001, and the mise machinery in §1 is verified; the two source builds are
unverified in this environment and need a machine with normal internet access.
