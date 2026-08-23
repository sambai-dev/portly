//! Audit U4 regression: the kill guard `verify_and_kill` must re-verify that
//! the target PID still owns the expected port immediately before killing, so
//! a recycled PID pointing at an unrelated process is refused, not terminated.
//!
//! A real sacrificial child (`examples/port_sleeper`) binds 127.0.0.1:<port>
//! and naps. The guard is exercised against it directly through the library
//! surface — no TTY, Docker daemon, or elevated privileges required.

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use portly::collectors::process::verify_and_kill;

const LISTEN_DEADLINE: Duration = Duration::from_secs(10);
const EXIT_DEADLINE: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(25);

/// RAII handle: the child is killed and reaped on every exit path.
struct Sleeper(Child);

impl Sleeper {
    fn pid(&self) -> u32 {
        self.0.id()
    }

    fn alive(&mut self) -> bool {
        self.0.try_wait().expect("poll sleeper status").is_none()
    }

    fn wait_exit(&mut self) {
        let started = Instant::now();
        loop {
            match self.0.try_wait().expect("poll sleeper status") {
                Some(_) => return,
                None if started.elapsed() > EXIT_DEADLINE => {
                    panic!("child survived {EXIT_DEADLINE:?} after kill")
                }
                None => thread::sleep(POLL_INTERVAL),
            }
        }
    }
}

impl Drop for Sleeper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Path to the example helper. CARGO_BIN_EXE_portly points at
/// `<target>/<profile>/portly(.exe)`; cargo test also builds examples into
/// `<target>/<profile>/examples/`, so the helper sits next to it.
fn sleeper_exe() -> PathBuf {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_portly"));
    let name = if cfg!(windows) {
        "port_sleeper.exe"
    } else {
        "port_sleeper"
    };
    let path = bin.parent().expect("bin dir").join("examples").join(name);
    assert!(
        path.exists(),
        "port_sleeper helper missing at {} (cargo test builds examples)",
        path.display()
    );
    path
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind :0 to reserve an ephemeral port")
        .local_addr()
        .expect("local addr")
        .port()
}

fn spawn_owner() -> (Sleeper, u16) {
    for _ in 0..5 {
        let port = free_port();
        let mut sleeper = Sleeper(
            Command::new(sleeper_exe())
                .arg(port.to_string())
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn port_sleeper"),
        );
        if wait_until_listening(port) && sleeper.alive() {
            // Settle briefly so the kernel socket table has the PID mapped.
            thread::sleep(Duration::from_millis(50));
            return (sleeper, port);
        }
        // Port was likely stolen between :0 probe and child bind; retry fresh.
    }
    panic!("could not start a port-owning child after 5 attempts");
}

fn wait_until_listening(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let started = Instant::now();
    while started.elapsed() <= LISTEN_DEADLINE {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(250)).is_ok() {
            return true;
        }
        thread::sleep(POLL_INTERVAL);
    }
    false
}

#[test]
fn kills_child_that_still_owns_expected_port() {
    let (mut child, port) = spawn_owner();

    verify_and_kill(child.pid(), port)
        .unwrap_or_else(|e| panic!("guard aborted legitimate owner: {e}"));

    child.wait_exit();
    assert!(
        !child.alive(),
        "owner exited but still reported alive (reap bug)"
    );
}

#[test]
fn aborts_when_pid_no_longer_owns_expected_port() {
    let (mut child, real) = spawn_owner();
    let mut wrong = free_port();
    if wrong == real {
        wrong = free_port();
    }

    let err = verify_and_kill(child.pid(), wrong)
        .expect_err("guard must refuse a PID that does not own the expected port");
    assert!(
        err.contains("kill aborted"),
        "refusal should say why: {err}"
    );

    assert!(
        child.alive(),
        "guard aborted but the child was killed anyway"
    );

    // Sanity: the same live PID passes when pointed at the port it actually
    // owns — proving the refusal above was the ownership check firing, not an
    // environmental failure.
    verify_and_kill(child.pid(), real)
        .unwrap_or_else(|e| panic!("guard refused the true owner afterwards: {e}"));
    child.wait_exit();
}
