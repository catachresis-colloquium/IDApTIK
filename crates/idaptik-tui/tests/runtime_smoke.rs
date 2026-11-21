// SPDX-License-Identifier: MPL-2.0
//! Runtime smoke test: does the real TUI binary boot, stay up, and quit cleanly?
//!
//! `headless_smoke.rs` already proves the TTY-free paths (`--headless`,
//! `--replay`, `--export`). What nothing else covered is the *interactive* path:
//! `app::run` takes the terminal, enters raw mode, enters the alternate screen
//! and loops until told to quit. That is the code a player actually runs, and a
//! regression there is invisible to a test that never allocates a terminal.
//!
//! This test does the smallest thing that would notice such a regression, on a
//! real pseudo-terminal:
//!
//!   1. Boots.   The binary must still be running after the settle window. A
//!      crash, a panic, or an immediate `Err` from `Terminal::new` fails here.
//!   2. Paints.  The captured output must contain the alternate-screen enter
//!      escape (`ESC [ ? 1049 h`).
//!   3. Quits.   Ctrl-C (`app::run` maps it to `Intent::Quit`) must produce exit
//!      status 0 within the quit timeout. A hang or a teardown panic fails here.
//!   4. Restores. The alternate-screen leave escape must be present, so the shell
//!      the player returns to is not left broken.
//!
//! Why a real pty rather than `script(1)`: `script` inherits the window size
//! from its own stdin, which on a CI runner is not a terminal, leaving the pty
//! 0x0; ratatui draws nothing into a zero-sized frame. We set the size ourselves.
//!
//! The negative controls (`rejects_*`) run the same harness against stub apps
//! that misbehave in each of the four ways above and require the matching
//! verdict, so the gate cannot pass because it stopped looking. A stub needs
//! only `sh`, `stty`, `dd`, `od` and `sleep`; if one is missing the stub fails
//! for the wrong reason and the control test fails. Nothing is skipped.

#![cfg(unix)]

use std::fs::File;
use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rustix::fs::{Mode, OFlags, open};
use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
use rustix::termios::{Winsize, tcsetwinsize};

const ENTER_ALT: &[u8] = b"\x1b[?1049h";
const LEAVE_ALT: &[u8] = b"\x1b[?1049l";
// Raw mode makes Ctrl-C arrive as a byte instead of SIGINT.
const QUIT_KEY: &[u8] = b"\x03";
const ROWS: u16 = 40;
const COLS: u16 = 120;

#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    Ok,
    ExitedEarly(Option<i32>),
    NoPaint,
    IgnoredQuit,
    BadExit(Option<i32>),
    NoLeave,
    Harness(String),
}

struct Run {
    verdict: Verdict,
    output: Vec<u8>,
}

fn harness<T, E: std::fmt::Display>(r: Result<T, E>, what: &str) -> Result<T, Run> {
    r.map_err(|e| Run {
        verdict: Verdict::Harness(format!("{what}: {e}")),
        output: Vec::new(),
    })
}

/// Wait until `deadline`; stop early once the child has exited and the reader
/// has had a moment to drain its buffer.
fn wait_until(child: &mut Child, deadline: Instant) -> Option<std::process::ExitStatus> {
    loop {
        if let Ok(Some(st)) = child.try_wait() {
            thread::sleep(Duration::from_millis(100));
            return Some(st);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn run_app(mut cmd: Command, settle: Duration, quit_timeout: Duration) -> Run {
    match drive(&mut cmd, settle, quit_timeout) {
        Ok(r) | Err(r) => r,
    }
}

fn drive(cmd: &mut Command, settle: Duration, quit_timeout: Duration) -> Result<Run, Run> {
    let master = harness(openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY), "openpt")?;
    harness(grantpt(&master), "grantpt")?;
    harness(unlockpt(&master), "unlockpt")?;
    let name = harness(ptsname(&master, Vec::new()), "ptsname")?;
    let slave = harness(
        open(
            name.as_c_str(),
            OFlags::RDWR | OFlags::NOCTTY,
            Mode::empty(),
        ),
        "open slave",
    )?;
    // A size the app can lay out in (a 0x0 pty makes ratatui draw nothing).
    harness(
        tcsetwinsize(
            &slave,
            Winsize {
                ws_row: ROWS,
                ws_col: COLS,
                ws_xpixel: 0,
                ws_ypixel: 0,
            },
        ),
        "tcsetwinsize",
    )?;

    cmd.env("TERM", "xterm-256color").env_remove("NO_COLOR");
    let child = cmd
        .stdin(harness(slave.try_clone(), "dup stdin")?)
        .stdout(harness(slave.try_clone(), "dup stdout")?)
        .stderr(Stdio::from(slave))
        .spawn();
    // Drop the parent's slave copies (held inside `cmd`) so the master sees EIO
    // when the child is gone.
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = harness(child, "spawn")?;

    let mut writer = File::from(master);
    let mut reader = harness(writer.try_clone(), "dup master")?;
    let captured = Arc::new(Mutex::new(Vec::<u8>::new()));
    let sink = Arc::clone(&captured);
    thread::spawn(move || {
        let mut buf = [0u8; 65536];
        // EIO is the normal way a pty master reports "child is gone".
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            sink.lock().unwrap().extend_from_slice(&buf[..n]);
        }
    });
    let snapshot = || captured.lock().unwrap().clone();
    let finish = |child: &mut Child, v: Verdict| {
        if matches!(child.try_wait(), Ok(None)) {
            let _ = child.kill();
            let _ = child.wait();
        }
        thread::sleep(Duration::from_millis(50));
        Run {
            verdict: v,
            output: snapshot(),
        }
    };

    // 1. Boots and stays up.
    if let Some(st) = wait_until(&mut child, Instant::now() + settle) {
        return Ok(finish(&mut child, Verdict::ExitedEarly(st.code())));
    }
    // 2. Paints.
    if !contains(&snapshot(), ENTER_ALT) {
        return Ok(finish(&mut child, Verdict::NoPaint));
    }
    // 3. Quits cleanly when asked.
    if let Err(e) = writer.write_all(QUIT_KEY) {
        return Ok(finish(
            &mut child,
            Verdict::Harness(format!("send quit: {e}")),
        ));
    }
    let Some(st) = wait_until(&mut child, Instant::now() + quit_timeout) else {
        return Ok(finish(&mut child, Verdict::IgnoredQuit));
    };
    if st.code() != Some(0) {
        return Ok(finish(&mut child, Verdict::BadExit(st.code())));
    }
    // 4. Gives the terminal back.
    if !contains(&snapshot(), LEAVE_ALT) {
        return Ok(finish(&mut child, Verdict::NoLeave));
    }
    Ok(finish(&mut child, Verdict::Ok))
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn report(run: &Run) -> String {
    let text = String::from_utf8_lossy(&run.output);
    let shown: String = text.chars().take(4000).collect();
    format!(
        "{:?}; captured {} bytes: {:?}",
        run.verdict,
        run.output.len(),
        shown
    )
}

// --- the real binary ---------------------------------------------------------

#[test]
fn real_tui_boots_paints_and_quits_cleanly() {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_idaptik-tui"));
    cmd.args(["--seed", "123456"]);
    let run = run_app(cmd, Duration::from_secs(3), Duration::from_secs(10));
    assert_eq!(run.verdict, Verdict::Ok, "{}", report(&run));
}

// --- negative controls: the gate must be able to fail -------------------------

const PRELUDE: &str = r"stty raw -echo; ";
const ENTER: &str = r"printf '\033[?1049h'; ";
const LEAVE: &str = r"printf '\033[?1049l'; ";
// Read one byte from the pty; succeeds only for Ctrl-C (0x03).
const GOT_QUIT: &str = r#"[ "$(dd bs=1 count=1 2>/dev/null | od -An -tx1 | tr -d ' \n')" = 03 ]"#;

fn stub(body: &str) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c").arg(format!("{PRELUDE}{body}"));
    c
}

fn control(body: &str) -> Run {
    run_app(
        stub(body),
        Duration::from_millis(600),
        Duration::from_millis(1500),
    )
}

#[test]
fn control_good_stub_passes() {
    let run = control(&format!(
        "{ENTER}while :; do if {GOT_QUIT}; then {LEAVE}exit 0; fi; done"
    ));
    assert_eq!(run.verdict, Verdict::Ok, "{}", report(&run));
}

#[test]
fn rejects_exit_during_settle() {
    let run = control(&format!("{ENTER}exit 0"));
    assert_eq!(
        run.verdict,
        Verdict::ExitedEarly(Some(0)),
        "{}",
        report(&run)
    );
}

#[test]
fn rejects_never_painting() {
    let run = control("dd bs=1 count=1 >/dev/null 2>&1; exit 0");
    assert_eq!(run.verdict, Verdict::NoPaint, "{}", report(&run));
}

#[test]
fn rejects_ignored_quit() {
    let run = control(&format!("{ENTER}while :; do sleep 1; done"));
    assert_eq!(run.verdict, Verdict::IgnoredQuit, "{}", report(&run));
}

#[test]
fn rejects_unclean_exit_status() {
    let run = control(&format!(
        "{ENTER}while :; do if {GOT_QUIT}; then exit 3; fi; done"
    ));
    assert_eq!(run.verdict, Verdict::BadExit(Some(3)), "{}", report(&run));
}

#[test]
fn rejects_missing_terminal_restore() {
    let run = control(&format!(
        "{ENTER}while :; do if {GOT_QUIT}; then exit 0; fi; done"
    ));
    assert_eq!(run.verdict, Verdict::NoLeave, "{}", report(&run));
}
