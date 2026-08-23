//! Audit D1 regression: bare `portly` with piped (non-TTY) stdout must refuse
//! to start with a non-zero exit instead of hanging on alt-screen escapes.
//!
//! This spawns the real binary with both pipes held by the test process, so no
//! TTY, Docker daemon, or listening port is required.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const HANG_DEADLINE: Duration = Duration::from_secs(15);

#[test]
fn piped_stdout_refuses_dashboard() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_portly"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn portly binary");

    let started = Instant::now();
    let status = loop {
        match child.try_wait().expect("poll child status") {
            Some(status) => break status,
            None if started.elapsed() > HANG_DEADLINE => {
                let _ = child.kill();
                panic!("portly hung for {HANG_DEADLINE:?} with piped stdout (D1 regression)");
            }
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    };

    assert!(
        !status.success(),
        "non-TTY stdout must exit non-zero, got {status}"
    );

    let mut stdout_bytes = String::new();
    child
        .stdout
        .take()
        .expect("stdout pipe")
        .read_to_string(&mut stdout_bytes)
        .expect("drain stdout");
    assert!(
        stdout_bytes.is_empty(),
        "escape bytes leaked to piped stdout: {stdout_bytes:?}"
    );

    let mut stderr_text = String::new();
    child
        .stderr
        .take()
        .expect("stderr pipe")
        .read_to_string(&mut stderr_text)
        .expect("drain stderr");
    assert!(
        stderr_text.contains("--once"),
        "stderr must point at `portly --once`: {stderr_text:?}"
    );
}

/// `--json` is a headless-only output mode: bare `portly --json` must refuse
/// pre-TTY with the working `--json --once` pair, same guard family as above.
#[test]
fn json_without_once_refuses_with_pair_hint() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_portly"))
        .arg("--json")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn portly binary");

    let started = Instant::now();
    let status = loop {
        match child.try_wait().expect("poll child status") {
            Some(status) => break status,
            None if started.elapsed() > HANG_DEADLINE => {
                let _ = child.kill();
                panic!("portly hung for {HANG_DEADLINE:?} with bare --json (guard regression)");
            }
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    };

    assert!(
        !status.success(),
        "bare --json must exit non-zero, got {status}"
    );

    let mut stdout_bytes = String::new();
    child
        .stdout
        .take()
        .expect("stdout pipe")
        .read_to_string(&mut stdout_bytes)
        .expect("drain stdout");
    assert!(
        stdout_bytes.is_empty(),
        "no dashboard output may reach piped stdout: {stdout_bytes:?}"
    );

    let mut stderr_text = String::new();
    child
        .stderr
        .take()
        .expect("stderr pipe")
        .read_to_string(&mut stderr_text)
        .expect("drain stderr");
    assert!(
        stderr_text.contains("--json --once"),
        "stderr must point at `portly --json --once`: {stderr_text:?}"
    );
}
