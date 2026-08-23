//! Test-only helper for the U4 kill-guard integration test.
//!
//! Binds a TCP listener on 127.0.0.1:<port> so the process genuinely owns
//! the port, then naps. Never accepts connections; exits on its own after
//! ten minutes as a last-resort orphan guard (tests normally kill it).

use std::net::TcpListener;
use std::time::Duration;

fn main() {
    let port: u16 = std::env::args()
        .nth(1)
        .expect("usage: port_sleeper <port>")
        .parse()
        .expect("port must be a u16");

    let _listener = TcpListener::bind(("127.0.0.1", port)).expect("failed to bind");
    println!("port_sleeper listening on {port}");
    std::thread::sleep(Duration::from_secs(600));
}
