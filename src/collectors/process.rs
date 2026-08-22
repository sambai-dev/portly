use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use sysinfo::{Pid, ProcessesToUpdate, System};

use crate::collectors::sockets;
use crate::model::{PortEntry, Source};

/// sysinfo needs two refreshes at least this far apart before per-process
/// CPU% is meaningful; until then we render blanks rather than fake numbers.
const CPU_SAMPLE_INTERVAL: Duration = Duration::from_millis(200);

pub struct ProcIndex {
    system: System,
    last_refresh: Option<Instant>,
    seen: HashSet<u32>,
}

impl Default for ProcIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcIndex {
    pub fn new() -> Self {
        Self {
            system: System::new_all(),
            last_refresh: None,
            seen: HashSet::new(),
        }
    }

    /// Fill process name / memory / CPU for every entry with a visible PID.
    /// Config labels fill in friendly names only where the OS gives us none —
    /// raw evidence always wins over inference.
    pub fn enrich(&mut self, entries: &mut [PortEntry], labels: &HashMap<u16, String>) {
        let now = Instant::now();
        let cpu_valid = self
            .last_refresh
            .is_some_and(|t| now.duration_since(t) >= CPU_SAMPLE_INTERVAL);

        self.system.refresh_processes(ProcessesToUpdate::All);
        let procs = self.system.processes();

        for e in entries.iter_mut() {
            let Some(pid) = e.pid else { continue };
            let Some(p) = procs.get(&Pid::from_u32(pid)) else {
                continue;
            };

            e.process = Some(
                p.name()
                    .to_string_lossy()
                    .trim_end_matches(".exe")
                    .to_string(),
            );
            e.mem_bytes = Some(p.memory());
            e.cpu = if cpu_valid && self.seen.contains(&pid) {
                Some(p.cpu_usage())
            } else {
                None
            };
            if e.cmdline.is_none() {
                e.cmdline = p
                    .cmd()
                    .first()
                    .map(|c| c.to_string_lossy().chars().take(80).collect());
            }
            if e.pid.is_some() {
                e.source = Source::Proc;
            }
            if e.process.is_none() {
                if let Some(label) = labels.get(&e.port) {
                    e.process = Some(label.clone());
                }
            }
            self.seen.insert(pid);
        }
        self.last_refresh = Some(now);
    }
}

/// Kill a PID only after re-verifying it still owns the expected port —
/// guards against terminating an unrelated process that reused the PID.
pub fn verify_and_kill(pid: u32, port: u16) -> Result<(), String> {
    let entries = sockets::scan_sockets().map_err(|e| e.to_string())?;
    if !entries.iter().any(|e| e.pid == Some(pid) && e.port == port) {
        return Err(format!("pid {pid} no longer owns :{port}; kill aborted"));
    }

    let system = System::new_all();
    match system.process(Pid::from_u32(pid)) {
        None => Err(format!("pid {pid} already exited")),
        Some(p) => {
            if p.kill() {
                Ok(())
            } else {
                Err(format!("failed to terminate pid {pid}"))
            }
        }
    }
}
