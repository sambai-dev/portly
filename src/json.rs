//! Machine-readable snapshots for scripting (`portly --once --json`).
//!
//! The schema below is a public contract: snake_case field names in serde
//! struct order, pretty-printed with a 2-space indent, no per-row timestamps
//! beyond what the table already shows. Renames here must update the README
//! schema block and the golden-shape test at the bottom of this file.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::model::{Model, PortEntry};

/// One stable JSON object describing everything `--once` can see.
#[derive(Debug, Serialize)]
pub struct Snapshot {
    /// RFC3339 UTC instant the snapshot was captured (millisecond precision).
    pub captured_at: String,
    /// Active substring filter (always null today; reserved for scripting).
    pub filter: Option<String>,
    pub counts: Counts,
    /// Every visible row, in render order — the JSON twin of the table.
    pub listeners: Vec<Listener>,
    /// Host processes folded per PID, sorted by PID.
    pub processes: Vec<Process>,
    /// Containers folded per ID, sorted by ID.
    pub containers: Vec<Container>,
}

#[derive(Debug, Serialize)]
pub struct Counts {
    pub listeners: usize,
    pub processes: usize,
    pub containers: usize,
}

/// Same fields the rendered row shows: PORT PROTO PID PROCESS CPU% MEM SRC.
/// Container-backed rows carry their display name in `process` and a null
/// `pid`, exactly like the table's `-`.
#[derive(Debug, Serialize)]
pub struct Listener {
    pub port: u16,
    pub proto: String,
    pub pid: Option<u32>,
    pub process: String,
    pub cpu_pct: Option<f32>,
    pub mem_bytes: Option<u64>,
    pub source: String,
}

/// Same fields a process-backed row shows, minus its per-port columns.
#[derive(Debug, Serialize)]
pub struct Process {
    pub pid: u32,
    pub process: String,
    pub cpu_pct: Option<f32>,
    pub mem_bytes: Option<u64>,
}

/// Same fields a container-backed row carries: identity, runtime state, stats.
#[derive(Debug, Serialize)]
pub struct Container {
    pub id: String,
    pub name: String,
    pub state: Option<String>,
    pub cpu_pct: Option<f32>,
    pub mem_bytes: Option<u64>,
}

/// Snapshot stamped with the current clock.
pub fn snapshot(m: &Model) -> Snapshot {
    snapshot_at(m, rfc3339_now())
}

/// Pure builder; the injectable timestamp keeps golden tests deterministic.
pub fn snapshot_at(m: &Model, captured_at: String) -> Snapshot {
    let mut processes: BTreeMap<u32, Process> = BTreeMap::new();
    let mut containers: BTreeMap<String, Container> = BTreeMap::new();
    let mut listeners = Vec::new();

    for &i in &m.visible() {
        let e = &m.entries[i];
        listeners.push(listener_row(e));
        match (&e.container, e.pid) {
            (Some((id, name)), _) => {
                containers
                    .entry(id.clone())
                    .and_modify(|c| {
                        c.cpu_pct = max_f32(c.cpu_pct, e.cpu);
                        c.mem_bytes = max_u64(c.mem_bytes, e.mem_bytes);
                    })
                    .or_insert_with(|| Container {
                        id: id.clone(),
                        name: name.clone(),
                        state: e.container_state.clone(),
                        cpu_pct: e.cpu,
                        mem_bytes: e.mem_bytes,
                    });
            }
            (None, Some(pid)) => {
                processes
                    .entry(pid)
                    .and_modify(|p| {
                        p.cpu_pct = max_f32(p.cpu_pct, e.cpu);
                        p.mem_bytes = max_u64(p.mem_bytes, e.mem_bytes);
                    })
                    .or_insert_with(|| Process {
                        pid,
                        process: e.display_name(),
                        cpu_pct: e.cpu,
                        mem_bytes: e.mem_bytes,
                    });
            }
            // Kernel-reported sockets without an owner: listener-only rows,
            // exactly what the table shows them as.
            (None, None) => {}
        }
    }

    Snapshot {
        captured_at,
        filter: m.filter.clone(),
        counts: Counts {
            listeners: listeners.len(),
            processes: processes.len(),
            containers: containers.len(),
        },
        listeners,
        processes: processes.into_values().collect(),
        containers: containers.into_values().collect(),
    }
}

fn listener_row(e: &PortEntry) -> Listener {
    Listener {
        port: e.port,
        proto: e.proto.to_string(),
        pid: e.pid,
        process: e.display_name(),
        cpu_pct: e.cpu,
        mem_bytes: e.mem_bytes,
        source: e.source.to_string(),
    }
}

/// Rows sharing an owner carry identical stats; the fold is a NaN-safe max so
/// ordering can never leak into the output values.
fn max_f32(a: Option<f32>, b: Option<f32>) -> Option<f32> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (x, y) => x.or(y),
    }
}

fn max_u64(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (x, y) => x.or(y),
    }
}

// -------------------------------------------------------- timestamps ------

pub fn rfc3339_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    rfc3339(now.as_secs(), now.subsec_millis())
}

/// RFC3339 UTC with millisecond precision, dependency-free.
fn rfc3339(unix_secs: u64, millis: u32) -> String {
    let days = (unix_secs / 86_400) as i64;
    let secs_of_day = unix_secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        secs_of_day / 3_600,
        (secs_of_day % 3_600) / 60,
        secs_of_day % 60
    )
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 → (y, m, d).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let y = yoe + era * 400;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// -------------------------------------------------------------- tests -----

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Protocol, SortKey, Source};
    use serde_json::Value;

    fn host(port: u16, pid: Option<u32>, name: &str) -> PortEntry {
        PortEntry {
            port,
            proto: Protocol::Tcp,
            pid,
            process: Some(name.to_string()),
            cmdline: None,
            cpu: Some(2.5),
            mem_bytes: Some(12_582_912),
            source: Source::Proc,
            container: None,
            container_state: None,
        }
    }

    fn container(port: u16, id: &str, name: &str, state: &str) -> PortEntry {
        PortEntry {
            port,
            proto: Protocol::Tcp,
            pid: None,
            process: None,
            cmdline: None,
            cpu: Some(1.25),
            mem_bytes: Some(356_515_840),
            source: Source::Docker,
            container: Some((id.to_string(), name.to_string())),
            container_state: Some(state.to_string()),
        }
    }

    const CAPTURED_AT: &str = "2026-08-24T10:30:00.000Z";

    fn fixture() -> Model {
        let mut m = Model::new();
        m.sort = SortKey::Port;
        m.entries = vec![
            host(3000, Some(4121), "node"),
            container(5432, "abc123", "pg-dev", "running"),
        ];
        m
    }

    /// (a) Output must parse via serde_json and expose exactly the six
    /// documented top-level keys with the documented types.
    #[test]
    fn json_parses_with_six_typed_top_level_keys() {
        let text =
            serde_json::to_string_pretty(&snapshot_at(&fixture(), CAPTURED_AT.into())).unwrap();
        let v: Value = serde_json::from_str(&text).expect("output must be valid JSON");
        let obj = v.as_object().expect("top level must be an object");
        assert_eq!(obj.len(), 6, "exactly six top-level keys: {text}");
        for key in [
            "captured_at",
            "filter",
            "counts",
            "listeners",
            "processes",
            "containers",
        ] {
            assert!(
                obj.contains_key(key),
                "missing documented key {key:?}: {text}"
            );
        }

        let ts = obj["captured_at"].as_str().expect("captured_at string");
        assert!(ts.ends_with('Z') && ts.contains('T'), "RFC3339 UTC: {ts}");
        assert!(obj["filter"].is_null(), "unfiltered run: {text}");

        for k in ["listeners", "processes", "containers"] {
            assert!(obj["counts"][k].is_u64(), "counts.{k} must be numeric");
            assert!(v[k].is_array(), "{k} must be an array");
        }
        for (count, arr) in [
            ("listeners", &v["listeners"]),
            ("processes", &v["processes"]),
            ("containers", &v["containers"]),
        ] {
            assert_eq!(
                obj["counts"][count].as_u64(),
                Some(arr.as_array().unwrap().len() as u64),
                "counts.{count} must match the array length"
            );
        }
    }

    #[test]
    fn same_pid_rows_fold_into_one_process() {
        let mut m = Model::new();
        m.entries = vec![
            host(3000, Some(7), "node"),
            host(8080, Some(7), "node"),
            host(9000, Some(9), "pg"),
        ];
        let snap = snapshot_at(&m, CAPTURED_AT.into());
        assert_eq!(snap.counts.listeners, 3);
        assert_eq!(snap.counts.processes, 2);
        assert_eq!(snap.counts.containers, 0);
        let pids: Vec<u32> = snap.processes.iter().map(|p| p.pid).collect();
        assert_eq!(pids, vec![7, 9], "sorted by PID");
    }

    #[test]
    fn multiport_container_folds_into_one_container_entry() {
        let mut m = Model::new();
        m.entries = vec![
            container(8080, "id1", "api", "running"),
            container(8443, "id1", "api", "running"),
        ];
        let snap = snapshot_at(&m, CAPTURED_AT.into());
        assert_eq!((snap.counts.listeners, snap.counts.containers), (2, 1));
        assert_eq!(
            snap.counts.processes, 0,
            "docker rows never become processes"
        );
        assert_eq!(snap.containers[0].id, "id1");
        assert_eq!(snap.containers[0].state.as_deref(), Some("running"));
    }

    #[test]
    fn ownerless_kernel_rows_stay_listener_only() {
        let mut m = Model::new();
        let mut e = host(5353, None, "");
        e.process = None;
        e.source = Source::Kernel;
        m.entries = vec![e];
        let snap = snapshot_at(&m, CAPTURED_AT.into());
        assert_eq!(snap.listeners[0].source, "kernel");
        assert_eq!(snap.listeners[0].pid, None);
        assert_eq!(
            snap.listeners[0].process, "(unknown)",
            "table fallback name"
        );
        assert_eq!(snap.counts.processes, 0);
    }

    #[test]
    fn filter_field_passes_through_from_the_model() {
        let mut m = fixture();
        m.filter = Some("pg".into());
        assert_eq!(
            snapshot_at(&m, CAPTURED_AT.into()).filter.as_deref(),
            Some("pg")
        );
    }

    /// (c) Golden-ish guard: field names AND their order are locked to the
    /// documented schema, so accidental renames fail this test loudly.
    #[test]
    fn golden_serialized_shape_matches_documented_schema() {
        let text =
            serde_json::to_string_pretty(&snapshot_at(&fixture(), CAPTURED_AT.into())).unwrap();
        assert_eq!(text, GOLDEN.trim());
    }

    const GOLDEN: &str = r#"{
  "captured_at": "2026-08-24T10:30:00.000Z",
  "filter": null,
  "counts": {
    "listeners": 2,
    "processes": 1,
    "containers": 1
  },
  "listeners": [
    {
      "port": 3000,
      "proto": "tcp",
      "pid": 4121,
      "process": "node",
      "cpu_pct": 2.5,
      "mem_bytes": 12582912,
      "source": "proc"
    },
    {
      "port": 5432,
      "proto": "tcp",
      "pid": null,
      "process": "pg-dev",
      "cpu_pct": 1.25,
      "mem_bytes": 356515840,
      "source": "docker"
    }
  ],
  "processes": [
    {
      "pid": 4121,
      "process": "node",
      "cpu_pct": 2.5,
      "mem_bytes": 12582912
    }
  ],
  "containers": [
    {
      "id": "abc123",
      "name": "pg-dev",
      "state": "running",
      "cpu_pct": 1.25,
      "mem_bytes": 356515840
    }
  ]
}"#;

    #[test]
    fn rfc3339_formats_known_instants() {
        assert_eq!(rfc3339(0, 0), "1970-01-01T00:00:00.000Z");
        assert_eq!(rfc3339(1_000_000_000, 500), "2001-09-09T01:46:40.500Z");
        assert_eq!(rfc3339(1_709_164_800, 250), "2024-02-29T00:00:00.250Z");
    }

    #[test]
    fn rfc3339_now_is_well_formed_utc() {
        let ts = rfc3339_now();
        assert_eq!(ts.len(), 24, "YYYY-MM-DDTHH:MM:SS.mmmZ: {ts}");
        assert!(ts.ends_with('Z'));
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[10..11], "T");
    }
}
