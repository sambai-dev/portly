//! Scan benchmark: measures the collector pipeline (socket scan → PID join →
//! process enrichment) over many iterations and reports percentiles.
//!
//! Run: `cargo run --release --example bench [iterations]`

use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

use portly::collectors::ProcIndex;

fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * q).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(200);

    println!("portly bench: {n} iterations of sockets+process scan");
    let mut samples_ms: Vec<f64> = Vec::with_capacity(n);
    let mut last_found = 0usize;

    let ignored = BTreeSet::new();
    let labels: HashMap<u16, String> = HashMap::new();

    // Warmup seeds sysinfo so later samples exercise the steady-state path
    // (two refreshes >=200ms apart), matching the live collector's cadence.
    let mut index = ProcIndex::new();
    if let Err(e) = portly::collectors::run_once_into(&mut index, &ignored, &labels) {
        eprintln!("warmup scan failed: {e}");
        std::process::exit(1);
    }

    for _ in 0..n {
        let t0 = Instant::now();
        match portly::collectors::run_once_into(&mut index, &ignored, &labels) {
            Ok(found) => {
                last_found = found;
                samples_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
            }
            Err(e) => {
                eprintln!("scan failed: {e}");
                std::process::exit(1);
            }
        }
        // Respect the >=200ms CPU-sample cadence the live collector uses so
        // numbers reflect production pacing, not back-to-back refreshes.
        std::thread::sleep(Duration::from_millis(220));
    }

    samples_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let min = samples_ms[0];
    let max = samples_ms[samples_ms.len() - 1];
    let mean = samples_ms.iter().sum::<f64>() / samples_ms.len() as f64;

    println!("found={} services", last_found);
    println!(
        "min={min:.2}ms p50={:.2}ms p95={:.2}ms p99={:.2}ms max={max:.2}ms mean={mean:.2}ms",
        percentile(&samples_ms, 0.50),
        percentile(&samples_ms, 0.95),
        percentile(&samples_ms, 0.99),
    );
}
