pub mod process;
pub mod sockets;

use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;

use crate::model::Msg;

/// Spawn the socket+process collector on a dedicated OS thread.
///
/// One worker per source keeps failure isolated: if this scan errors we send
/// `CollectorFailed` and keep going; nothing else in the app depends on it.
/// Docker and health collectors join the same mpsc channel in v0.2/v0.3
/// (they move onto tokio tasks when streaming arrives).
pub fn spawn_collector(tx: UnboundedSender<Msg>, interval: Duration) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("portly-collector".into())
        .spawn(move || {
            tracing::debug!(
                interval_ms = interval.as_millis() as u64,
                "collector started"
            );
            let mut index = process::ProcIndex::new();
            loop {
                std::thread::sleep(interval);
                let started = Instant::now();
                match sockets::scan_sockets() {
                    Ok(mut entries) => {
                        index.enrich(&mut entries);
                        let found = entries.len();
                        let duration_ms = started.elapsed().as_millis() as u64;
                        tracing::debug!(found, duration_ms, "scan complete");
                        if tx.send(Msg::ScanResult(entries)).is_err() {
                            break; // UI gone; collector exits quietly
                        }
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "socket scan failed");
                        if tx.send(Msg::CollectorFailed(err.to_string())).is_err() {
                            break;
                        }
                    }
                }
            }
            tracing::debug!("collector stopped");
        })
        .expect("failed to spawn collector thread")
}
