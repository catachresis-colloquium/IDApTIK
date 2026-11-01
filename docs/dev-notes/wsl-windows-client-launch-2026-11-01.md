# WSL2 → native Windows client: launch evidence (2026-11-01)

Status: **implemented and fixture-verified in a sandbox; not yet run on a real
WSL2 + Windows GPU host.** This note records what was changed, what was proven,
and what was not, so the next session does not have to guess which claims are
load-bearing.

## Symptom

On the maintainer's WSL2 machine the Linux Bevy client reached the renderer and
then died during presentation (`Fallback system failed to choose present mode`,
`EGL: DRI2: failed to load driver`). The route that worked was the native
Windows client, but it could only be reached by exporting
`IDAPTIK_CLIENT_PLATFORM=windows` and extending `PATH` with
`/mnt/c/Windows/System32` before running a private `launcher-wsl.sh` wrapper.

## Root cause in this repository

Three scripts decided "can this machine start the Windows client?" with

```sh
command -v cmd.exe >/dev/null 2>&1 && command -v wslpath >/dev/null 2>&1
```

- `launcher.sh` (`windows_interop_available`, used by the WSLg Copy Mode
  fallback and by `--flowdiags` for `explorer.exe`)
- `scripts/runtime-doctor.sh` (same function; `--doctor` reported "Windows
  interop … cmd.exe or wslpath is unavailable" on a machine where interop
  worked)
- `install.sh` (section 4b, which therefore provisioned only the Linux client)

WSL does not require `C:\Windows\System32` on `PATH`. A working machine was
being told it had no interop, and the only known cure for the presentation
failure was unreachable from the standard launcher.

## Changes

| File | Change |
|------|--------|
| `scripts/windows-interop.sh` | **New.** Single source for WSL detection, Windows system-directory discovery (explicit override → `/mnt/c/Windows/System32` → `wslpath -u`), tool resolution, Copy Mode detection, and an interop *probe* that executes `cmd.exe /c exit 0` (cached per process). A file that exists but cannot run does not count. |
| `launcher.sh` | Sources the library; `--windows-client` (alias `--wsl-windows`) is now a modifier for `--solo`/`--host`/`--join`/`--auto`; explicit requests are validated against WSL2 + interop with an actionable reason; dry runs print the selected `IDAPTIK_CLIENT_PLATFORM`; the solo launch reads Bevy's `AdapterInfo` line from the client log and reports the adapter, warning when it is software; `--status` repeats it; a client that dies during surface setup prints the `--solo --windows-client` hint when interop is available; `--flowdiags` resolves `explorer.exe` through the library. Version 0.5.0. |
| `scripts/runtime-doctor.sh` | Sources the library; interop failures name the reason; Windows target/linker checks fall back to `mise exec -- rustup` and name `just windows-client-deps`; new `bevy renderer` check reads the adapter Bevy actually selected (the OpenGL `glxinfo` result is now labelled as the GL path, because the two can disagree). |
| `scripts/multiplayer-runtime.sh` | Sources the library and resolves `cmd.exe` once for `tasklist`/`taskkill`, instead of assuming it is on `PATH`; the diagnostics working directory falls back to `/` when `/mnt/c/Windows` is absent. |
| `install.sh` | Uses the shared discovery, so it provisions the Windows target and MinGW linker on a machine whose interop works without `cmd.exe` on `PATH`. |
| `tests/launcher_fixture_test.sh` | Stub `cargo` now materialises the debug client, the release client and the `x86_64-pc-windows-gnu` client; stub `cmd.exe` and `wslpath`; new fixtures for explicit selection, automatic WSLg fallback without `cmd.exe` on `PATH`, non-WSL refusal, missing-interop diagnosis, unrunnable-interop diagnosis, the Windows solo launch, the Windows multiplayer seat, and renderer evidence (including software adapters). |
| `tests/runtime_doctor_test.sh` | Fixtures for interop discovered outside `PATH`, and for the `bevy renderer` pass/warn paths. |
| Docs | `docs/man/idaptik-launcher.1` (0.5.0: `--windows-client`, `RENDERER EVIDENCE`, `IDAPTIK_RENDERER_WAIT`), `README.md`, `docs/MULTIPLAYER-TROUBLESHOOTING.md`, `docs/player-launch-flow.html`, `justfile` recipe comment. |

## Evidence (sandbox, bash only)

```
$ bash tests/launcher_fixture_test.sh
launcher fixtures: all clean and firing fixtures behaved        # exit 0, ~30 s

$ bash tests/runtime_doctor_test.sh
runtime doctor fixtures: PASS                                   # exit 0
```

The launcher fixtures now include:

- `--solo --windows-client` reaching READY through the target-triple binary
  (`target/x86_64-pc-windows-gnu/debug/idaptik-bevy.exe` is the executed path);
- the WSLg Copy Mode failure selecting the Windows client with **no `cmd.exe`
  on `PATH`**, only the discovered system directory;
- `--windows-client` refused outside WSL2;
- interop missing and interop present-but-unrunnable producing distinct,
  actionable diagnoses;
- the multiplayer runtime tracking a Windows seat through a `cmd.exe` that is
  not on `PATH`;
- a solo launch reporting `renderer: Fixture GPU (stub) (Vulkan)`;
- an llvmpipe `AdapterInfo` being reported as SOFTWARE rendering (warning) and
  surviving into `--status` after the game stops.

## Not verified

- **No Rust build/test/clippy evidence.** This sandbox has no `cargo`/`rustc`
  and no egress to `static.rust-lang.org` or `index.crates.io`, so nothing in
  `crates/` was compiled. No Rust source was changed.
- **No real WSL2/Windows host.** Interop discovery, the probe, and the
  target-triple build were exercised against stubs. The real `cmd.exe` probe
  and real MinGW cross-build are untested here.
- **Bevy's `AdapterInfo` log format is assumed** from Bevy's INFO adapter
  logging. If a future Bevy stops logging it, the launcher says "renderer: not
  reported" rather than inventing an answer; `IDAPTIK_RENDERER_WAIT` bounds the
  wait (default 6 s).
- **No GPU.** The llvmpipe/NVIDIA lines come from fixtures, not from a device.
- The `--windows-client` multiplayer path was verified with a stubbed
  `tasklist`/`taskkill`, not with a real detached Windows GUI process.

## Deliberate limits

- The flag is a launch modifier, not a new environment contract; an explicit
  `IDAPTIK_CLIENT_PLATFORM=windows` still works and is validated the same way.
- The launcher does not silently switch clients after a failure. It prints the
  command that would switch, because a silent 6-minute cross-build after a
  crash is worse than one clear line.
- Nothing here changes rendering, simulation, or gameplay truth.
