//! Opt-in HTTP health prober.
//!
//! Probes `http://127.0.0.1:{port}{path}` for every known listening TCP port
//! and reports ● up / ◐ degraded / ✗ down plus latency in milliseconds.
//! Off by default: scanning must never surprise users with outbound traffic.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::config::HealthConfig;
use crate::model::{Health, Msg};

pub type Tx = tokio::sync::mpsc::UnboundedSender<Msg>;

/// Shared view of the ports worth probing; refreshed by the runtime on every
/// host scan so the prober never probes stale or closed ports.
pub type PortFeed = Arc<Mutex<BTreeSet<u16>>>;

/// Spawn the prober. Returns `None` when the thread cannot be spawned
/// (log-and-degrade, audit U3): the caller treats it as health disabled.
pub fn spawn_prober(tx: Tx, cfg: HealthConfig) -> Option<(PortFeed, JoinHandle<()>)> {
    let feed: PortFeed = Arc::new(Mutex::new(BTreeSet::new()));
    let feed_clone = feed.clone();
    let result = std::thread::Builder::new()
        .name("portly-health".into())
        .spawn(move || {
            tracing::debug!(interval_ms = cfg.interval_ms, path = %cfg.path, "health prober started");
            loop {
                let ports: Vec<u16> =
                    feed_clone.lock().unwrap_or_else(|p| p.into_inner()).iter().copied().collect();
                if !ports.is_empty() {
                    let results = probe_round(&ports, &cfg);
                    if tx.send(Msg::HealthUpdate(results)).is_err() {
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(cfg.interval_ms));
            }
            tracing::debug!("health prober stopped");
        });
    match result {
        Ok(handle) => Some((feed, handle)),
        Err(e) => {
            tracing::warn!(error = %e, "failed to spawn health prober; health checks disabled");
            None
        }
    }
}

fn probe_round(ports: &[u16], cfg: &HealthConfig) -> HashMap<u16, (Health, Option<u16>)> {
    use std::thread;

    let results: Mutex<HashMap<u16, (Health, Option<u16>)>> = Mutex::new(HashMap::new());
    {
        let results = &results;
        thread::scope(|scope| {
            // Bounded parallelism: 8 workers keep a full round well under the
            // timeout even when every target is down.
            const WORKERS: usize = 8;
            let chunk = ports.len().div_ceil(WORKERS);
            for group in ports.chunks(chunk.max(1)) {
                scope.spawn(move || {
                    for &port in group {
                        let (state, ms) = probe_one(port, cfg);
                        results
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .insert(port, (state, ms));
                    }
                });
            }
        });
    }
    results.into_inner().unwrap_or_default()
}

fn probe_one(port: u16, cfg: &HealthConfig) -> (Health, Option<u16>) {
    let started = Instant::now();
    match http_probe(port, &cfg.path, Duration::from_millis(cfg.timeout_ms)) {
        Ok(status) => (
            Health::classify(status),
            Some(started.elapsed().as_millis().min(u16::MAX as u128) as u16),
        ),
        Err(_) => (Health::Down, None),
    }
}

/// Minimal HTTP/1.0 GET — local dev services are plain HTTP; we deliberately
/// avoid a TLS stack since probing localhost over TLS is not the use case.
fn http_probe(port: u16, path: &str, timeout: Duration) -> Result<u16, String> {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let addr = format!("127.0.0.1:{port}");
    let mut stream = TcpStream::connect(&addr).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;

    let request =
        format!("GET {path} HTTP/1.0\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|e| e.to_string())?;

    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
    let head = String::from_utf8_lossy(&buf[..n]);
    // "HTTP/1.1 200 OK" → status code is the second token of the first line.
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| "malformed status line".to_string())?;
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_boundaries() {
        assert_eq!(Health::classify(200), Health::Up);
        assert_eq!(Health::classify(302), Health::Up);
        assert_eq!(Health::classify(404), Health::Degraded);
        assert_eq!(Health::classify(503), Health::Degraded);
        assert_eq!(Health::classify(100), Health::Down);
    }

    #[test]
    fn glyphs_are_distinct() {
        assert_ne!(Health::Up.glyph(), Health::Degraded.glyph());
        assert_ne!(Health::Down.glyph(), Health::Unknown.glyph());
    }

    #[test]
    fn probe_against_closed_port_is_down() {
        // Port 9 is the discard protocol; nothing should listen locally.
        let cfg = HealthConfig {
            enabled: true,
            interval_ms: 1000,
            timeout_ms: 100,
            path: "/".into(),
        };
        let (state, _ms) = probe_one(9, &cfg);
        assert_eq!(state, Health::Down);
    }
}

#[cfg(test)]
mod audit3_tests {
    use super::*;
    use crate::config::HealthConfig;

    #[test]
    fn probe_round_reports_every_requested_port() {
        let cfg = HealthConfig {
            enabled: true,
            interval_ms: 1000,
            timeout_ms: 100,
            path: "/".into(),
        };
        // Ports 9 and 8 are discard/unused; nothing should answer locally.
        let results = probe_round(&[9, 8], &cfg);
        assert_eq!(results.len(), 2);
        assert!(results.contains_key(&9) && results.contains_key(&8));
        assert_eq!(results[&9].0, Health::Down);
    }

    #[test]
    fn probe_round_with_no_ports_is_empty_not_panic() {
        let cfg = HealthConfig {
            enabled: true,
            interval_ms: 1000,
            timeout_ms: 100,
            path: "/".into(),
        };
        assert!(probe_round(&[], &cfg).is_empty());
    }
}
