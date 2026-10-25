#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Runtime smoke test: does the real TUI binary boot, stay up, and quit cleanly?

`crates/idaptik-tui/tests/headless_smoke.rs` already proves the TTY-free paths
(`--headless`, `--replay`, `--export`) against the built binary. What nothing
covered is the *interactive* path: `app::run` takes the terminal, enters raw
mode, enters the alternate screen and then loops until it is told to quit. That
is the code a player actually runs, and a regression there is invisible to a
test that never allocates a terminal.

So this harness does the smallest thing that would notice such a regression:

  1. **Boots.**  Start the binary on a pseudo-terminal and require it to still be
     running after `--settle` seconds. A crash, a panic, or an immediate `Err`
     from `Terminal::new` fails here.
  2. **Paints.**  Require the alternate-screen escape (`ESC [ ? 1049 h`) in the
     captured output, i.e. `execute!(.., EnterAlternateScreen)` ran and ratatui
     drew at least one frame.
  3. **Quits cleanly on request.**  Send Ctrl-C (`app::run` maps it to
     `Intent::Quit`) and require exit status 0 within `--quit-timeout` seconds.
     A hang, or a panic during teardown, fails here.
  4. **Gives the terminal back.**  Require the alternate-screen *leave* escape,
     so the shell the player returns to is not left in a broken state.

Why a real pty rather than `script(1)`: `script` inherits the window size from
its own stdin, and on a CI runner that is not a terminal, which leaves the pty
0x0. ratatui reasonably draws nothing into a zero-sized frame, so the paint
check would fail for a reason that has nothing to do with the application. A
`TIOCSWINSZ` on the slave sets a size the application can actually use.

`--self-test` proves the gate can fail. It runs this same harness against
synthetic apps that misbehave in each of the four ways above and requires the
matching verdict for each. A smoke test that passes against a broken app is
worse than no smoke test, and this repository has been bitten by exactly that
("a gate that cannot fail" -- see the note in secret-scanner.yml) before.
"""

from __future__ import annotations

import argparse
import fcntl
import os
import pty
import select
import shutil
import struct
import subprocess
import sys
import tempfile
import termios
import time
from dataclasses import dataclass

# `execute!(stdout, EnterAlternateScreen)` / `LeaveAlternateScreen`.
ENTER_ALT = b"\x1b[?1049h"
LEAVE_ALT = b"\x1b[?1049l"
# `app::run` maps Ctrl-C to `Intent::Quit`; raw mode means it arrives as a byte
# rather than as SIGINT, so the app tears the terminal down itself.
QUIT_KEY = b"\x03"

ROWS, COLS = 40, 120


@dataclass
class Verdict:
    ok: bool
    reason: str
    output: bytes
    returncode: int | None


def _set_window_size(fd: int) -> None:
    # (rows, cols, xpixel, ypixel) -- a size the app can lay out in.
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))


def _drain(fd: int, deadline: float, sink: bytearray, proc: subprocess.Popen) -> None:
    """Read whatever the child emits until `deadline`, without blocking."""
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return
        ready, _, _ = select.select([fd], [], [], min(remaining, 0.05))
        if not ready:
            if proc.poll() is not None:
                # Drain what is left in the buffer, then stop.
                try:
                    while True:
                        chunk = os.read(fd, 65536)
                        if not chunk:
                            return
                        sink += chunk
                except OSError:
                    return
            continue
        try:
            chunk = os.read(fd, 65536)
        except OSError:
            # EIO is the normal way a pty master reports "child is gone".
            return
        if not chunk:
            return
        sink += chunk


def run_app(
    argv: list[str],
    settle: float = 3.0,
    quit_timeout: float = 10.0,
    env: dict[str, str] | None = None,
) -> Verdict:
    """Drive one binary on a pty and report whether it behaved like an app."""
    master, slave = pty.openpty()
    _set_window_size(slave)

    child_env = dict(os.environ if env is None else env)
    child_env.setdefault("TERM", "xterm-256color")
    # Keep the child from inheriting anything that would suppress rendering.
    child_env.pop("NO_COLOR", None)

    captured = bytearray()
    try:
        proc = subprocess.Popen(
            argv,
            stdin=slave,
            stdout=slave,
            stderr=slave,
            env=child_env,
            close_fds=True,
        )
    except OSError as exc:
        os.close(master)
        os.close(slave)
        return Verdict(False, f"could not start {argv[0]}: {exc}", b"", None)
    os.close(slave)

    try:
        # 1. Boots and stays up.
        _drain(master, time.monotonic() + settle, captured, proc)
        rc = proc.poll()
        if rc is not None:
            _drain(master, time.monotonic() + 0.2, captured, proc)
            return Verdict(
                False,
                f"exited during the {settle:g}s settle window (status {rc}); "
                "an app that is meant to stay up must still be running",
                bytes(captured),
                rc,
            )

        # 2. Paints.
        if ENTER_ALT not in captured:
            return Verdict(
                False,
                "no alternate-screen escape on stdout: the app never took the "
                "terminal or never drew a frame",
                bytes(captured),
                None,
            )

        # 3. Quits cleanly when asked.
        try:
            os.write(master, QUIT_KEY)
        except OSError as exc:
            return Verdict(False, f"could not send quit: {exc}", bytes(captured), None)
        _drain(master, time.monotonic() + quit_timeout, captured, proc)
        try:
            rc = proc.wait(timeout=max(0.1, quit_timeout))
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=5)
            return Verdict(
                False,
                f"still running {quit_timeout:g}s after Ctrl-C; the quit intent "
                "is not reaching run_loop",
                bytes(captured),
                None,
            )
        if rc != 0:
            return Verdict(
                False, f"exited {rc} after Ctrl-C; teardown is not clean", bytes(captured), rc
            )

        # 4. Gives the terminal back.
        if LEAVE_ALT not in captured:
            return Verdict(
                False,
                "exited 0 but never left the alternate screen; the terminal is "
                "left in a broken state",
                bytes(captured),
                rc,
            )
        return Verdict(True, "booted, painted, and quit cleanly", bytes(captured), rc)
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait(timeout=5)
        os.close(master)


# --- self-test: prove the gate can fail -------------------------------------

_STUB_TEMPLATE = """#!{python}
import os, sys, time, tty
mode = {mode!r}
enter = b"\\x1b[?1049h"
leave = b"\\x1b[?1049l"
# `app::run` calls crossterm's `enable_raw_mode()` before it reads a key, which
# is what makes Ctrl-C arrive as a byte instead of being eaten by the line
# discipline (and possibly delivered as SIGINT). A stub that skipped this would
# not be emulating the app, and would fail the quit check for its own reasons.
try:
    tty.setraw(0)
except Exception:
    pass
if mode == "never-paints":
    os.read(0, 1)
    sys.exit(0)
os.write(1, enter)
if mode == "exits-early":
    sys.exit(0)
if mode == "ignores-quit":
    while True:
        time.sleep(0.1)
if mode == "broken-teardown":
    while True:
        b = os.read(0, 1)
        if b == b"\\x03":
            sys.exit(3)
while True:
    b = os.read(0, 1)
    if b == b"\\x03":
        os.write(1, leave)
        sys.exit(0)
"""


def self_test() -> int:
    """Require the harness to pass a good app and reject each bad one."""
    python = sys.executable or shutil.which("python3") or "/usr/bin/env python3"
    cases = [
        ("good", True),
        ("exits-early", False),
        ("never-paints", False),
        ("ignores-quit", False),
        ("broken-teardown", False),
    ]
    failures = 0
    with tempfile.TemporaryDirectory() as tmp:
        for mode, want_pass in cases:
            path = os.path.join(tmp, f"stub-{mode}.py")
            with open(path, "w", encoding="utf-8") as fh:
                fh.write(_STUB_TEMPLATE.format(python=python, mode=mode))
            os.chmod(path, 0o755)
            verdict = run_app([path], settle=0.6, quit_timeout=1.5)
            if verdict.ok == want_pass:
                print(f"self-test: {mode:16s} -> {'pass' if verdict.ok else 'fail'}  ok")
            else:
                failures += 1
                print(
                    f"self-test: {mode:16s} -> {'pass' if verdict.ok else 'fail'}  "
                    f"UNEXPECTED (wanted {'pass' if want_pass else 'fail'}): "
                    f"{verdict.reason}",
                    file=sys.stderr,
                )
    if failures:
        print(f"self-test: {failures} case(s) behaved unexpectedly", file=sys.stderr)
        return 1
    print(f"self-test: {len(cases)} cases behaved as expected")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--bin",
        default="target/debug/idaptik-tui",
        help="TUI binary to drive (default: target/debug/idaptik-tui)",
    )
    parser.add_argument(
        "--settle",
        type=float,
        default=3.0,
        help="seconds the app must survive before we accept that it booted",
    )
    parser.add_argument(
        "--quit-timeout",
        type=float,
        default=10.0,
        help="seconds to wait for a clean exit after Ctrl-C",
    )
    parser.add_argument("--self-test", action="store_true", help="prove the gate can fail")
    parser.add_argument("--verbose", action="store_true", help="dump captured output")
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    binary = args.bin
    if not os.path.isfile(binary):
        print(
            f"runtime-smoke: {binary} not found -- build it first "
            "(cargo build -p idaptik-tui)",
            file=sys.stderr,
        )
        return 1
    if not os.access(binary, os.X_OK):
        print(f"runtime-smoke: {binary} is not executable", file=sys.stderr)
        return 1

    print(f"runtime-smoke: driving {binary} on a {ROWS}x{COLS} pty")
    verdict = run_app([binary, "--seed", "123456"], settle=args.settle, quit_timeout=args.quit_timeout)
    if args.verbose or not verdict.ok:
        rendered = verdict.output.decode("utf-8", "replace")
        # Escape sequences make a raw dump unreadable; show them escaped.
        print(f"--- captured {len(verdict.output)} bytes ---")
        print(repr(rendered[:4000]))
    if verdict.ok:
        print(f"runtime-smoke: ok -- {verdict.reason}")
        return 0
    print(f"runtime-smoke: FAIL -- {verdict.reason}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
