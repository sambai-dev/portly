pub mod process;
pub mod sockets;

pub use process::ProcIndex;

use std::collections::{BTreeSet, HashMap};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tokio::sync::mpsc::UnboundedSender;

use crate::model::{Msg, PortEntry};

/// Spawn the socket+process collector on a dedicated OS thread.
///
/// One worker per source keeps failure isolated: if this scan errors we send
/// `CollectorFailed` and keep going. Docker and health collectors join the
/// same mpsc channel from their own threads. If the thread itself cannot be
/// spawned, log-and-degrade (`None`) instead of panicking — audit U3.
#[allow(clippy::too_many_arguments)]
pub fn spawn_collector(
    tx: UnboundedSender<Msg>,
    interval: Duration,
    ignored: BTreeSet<u16>,
    labels: HashMap<u16, String>,
) -> Option<JoinHandle<()>> {
    let result = std::thread::Builder::new()
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
                match scan_and_enrich(&mut index, &ignored, &labels) {
                    Ok(entries) => {
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
        });
    match result {
        Ok(handle) => Some(handle),
        Err(e) => {
            tracing::warn!(error = %e, "failed to spawn collector thread; live scans disabled");
            None
        }
    }
}

fn scan_and_enrich(
    index: &mut process::ProcIndex,
    ignored: &BTreeSet<u16>,
    labels: &HashMap<u16, String>,
) -> std::io::Result<Vec<PortEntry>> {
    let mut entries = sockets::scan_sockets()?;
    entries.retain(|e| !ignored.contains(&e.port));
    index.enrich(&mut entries, labels);
    Ok(entries)
}

/// One headless pass for `portly --once` and benchmarks.
pub fn run_once(
    ignored: &BTreeSet<u16>,
    labels: &HashMap<u16, String>,
) -> std::io::Result<Vec<PortEntry>> {
    let mut index = ProcIndex::new();
    scan_and_enrich(&mut index, ignored, labels)
}

/// Same as [`run_once`] but reuses an existing [`ProcIndex`] so callers
/// benchmarking or polling keep CPU-sample continuity across calls.
pub fn run_once_into(
    index: &mut ProcIndex,
    ignored: &BTreeSet<u16>,
    labels: &HashMap<u16, String>,
) -> std::io::Result<usize> {
    Ok(scan_and_enrich(index, ignored, labels)?.len())
}
