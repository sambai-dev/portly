//! Docker integration: container discovery, stats, lifecycle actions, logs.
//!
//! Runs on its own OS thread hosting a current-thread tokio runtime; every
//! failure path degrades gracefully — a missing daemon must never take the
//! whole cockpit down, the host table keeps working.
//!
//! Uses the OpenAPI-generated `bollard::query_parameters` builders (the
//! legacy `bollard::container::*Options` structs are deprecated in 0.19).

use std::collections::{BTreeSet, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bollard::container::LogOutput;
use bollard::query_parameters::{
    ListContainersOptionsBuilder, LogsOptionsBuilder, RestartContainerOptionsBuilder,
    StartContainerOptionsBuilder, StatsOptionsBuilder, StopContainerOptionsBuilder,
};
use bollard::Docker;
use futures_util::StreamExt;

use crate::model::{ContainerAction, Msg, PortEntry, Protocol, Source};

pub type Tx = tokio::sync::mpsc::UnboundedSender<Msg>;
pub type StopFlag = Arc<AtomicBool>;

pub fn connect() -> Result<Docker, String> {
    Docker::connect_with_local_defaults().map_err(|e| format!("no docker daemon reachable: {e}"))
}

/// One discovery pass: every exposed port of every container, one row per
/// port with the owning container attached.
pub async fn list_entries(docker: &Docker) -> Result<Vec<PortEntry>, String> {
    let options = ListContainersOptionsBuilder::new().all(true).build();
    let containers = docker
        .list_containers(Some(options))
        .await
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for c in containers {
        let Some(id) = c.id.clone() else { continue };
        let name = c
            .names
            .as_ref()
            .and_then(|n| n.first())
            .map(|n| n.trim_start_matches('/').to_string())
            .unwrap_or_else(|| id.chars().take(12).collect());
        let state = c.state.map(|s| s.to_string()).filter(|s| !s.is_empty());
        let mut seen: HashSet<u16> = HashSet::new();
        if let Some(ports) = c.ports {
            for p in ports {
                if seen.insert(p.private_port) {
                    out.push(PortEntry {
                        port: p.private_port,
                        proto: Protocol::Tcp,
                        pid: None,
                        process: None,
                        cmdline: None,
                        cpu: None,
                        mem_bytes: None,
                        source: Source::Docker,
                        container: Some((id.clone(), name.clone())),
                        container_state: state.clone(),
                    });
                }
            }
        }
    }
    Ok(out)
}

/// One-shot stats sample → `(cpu_percent, mem_bytes)` using the standard
/// Engine-API delta formula.
pub async fn stats_once(docker: &Docker, id: &str) -> Option<(f32, u64)> {
    let options = StatsOptionsBuilder::new()
        .stream(false)
        .one_shot(true)
        .build();
    let mut stream = docker.stats(id, Some(options));
    let response = stream.next().await?.ok()?;

    let cur = response.cpu_stats.as_ref()?;
    let prev = response.precpu_stats.as_ref();

    let cpus = cur.online_cpus.unwrap_or(1).max(1) as f64;
    let system_now = cur.system_cpu_usage?;
    let system_prev = prev.and_then(|p| p.system_cpu_usage).unwrap_or(system_now);
    let usage_now = cur
        .cpu_usage
        .as_ref()
        .and_then(|u| u.total_usage)
        .unwrap_or(0);
    let usage_prev = prev
        .and_then(|p| p.cpu_usage.as_ref().and_then(|u| u.total_usage))
        .unwrap_or(usage_now);

    let sys_delta = system_now.saturating_sub(system_prev);
    let cpu_delta = usage_now.saturating_sub(usage_prev);
    let cpu_pct = if sys_delta > 0 {
        (cpu_delta as f64 / sys_delta as f64) * cpus * 100.0
    } else {
        0.0
    };
    let mem = response
        .memory_stats
        .as_ref()
        .and_then(|m| m.usage)
        .unwrap_or(0);
    Some((cpu_pct.clamp(0.0, 100.0 * 1024.0) as f32, mem))
}

pub async fn perform(docker: &Docker, id: &str, action: ContainerAction) -> Result<String, String> {
    match action {
        ContainerAction::Start => docker
            .start_container(id, Some(StartContainerOptionsBuilder::new().build()))
            .await
            .map_err(|e| e.to_string())?,
        ContainerAction::Stop => docker
            .stop_container(id, Some(StopContainerOptionsBuilder::new().t(10).build()))
            .await
            .map_err(|e| e.to_string())?,
        ContainerAction::Restart => docker
            .restart_container(id, Some(RestartContainerOptionsBuilder::new().build()))
            .await
            .map_err(|e| e.to_string())?,
    }
    Ok(format!("{action} container {id} ok"))
}

/// Collector loop on its own thread: discover containers each interval and
/// pull a one-shot stat sample per running container. If the thread cannot be
/// spawned, log-and-degrade instead of panicking — audit U3.
pub fn spawn_collector(tx: Tx, interval: Duration, ignored: BTreeSet<u16>) {
    let result = std::thread::Builder::new()
        .name("portly-docker".into())
        .spawn(move || {
            let Ok(docker) = connect() else {
                tracing::info!("docker disabled: daemon not reachable");
                return;
            };
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    tracing::warn!(error = %e, "tokio runtime unavailable for docker");
                    return;
                }
            };
            rt.block_on(async move {
                loop {
                    let started = Instant::now();
                    match list_entries(&docker).await {
                        Ok(entries) => {
                            let entries: Vec<_> = entries
                                .into_iter()
                                .filter(|e| !ignored.contains(&e.port))
                                .collect();
                            let found = entries.len();
                            // One stat sample per container, not per port:
                            // multi-port containers would otherwise trigger
                            // duplicate Engine-API round-trips each tick.
                            let mut seen_ids: HashSet<String> = HashSet::new();
                            let ids: Vec<String> = entries
                                .iter()
                                .filter(|e| e.container_state.as_deref() == Some("running"))
                                .filter_map(|e| e.container.as_ref().map(|(id, _)| id.clone()))
                                .filter(|id| seen_ids.insert(id.clone()))
                                .take(16)
                                .collect();
                            if tx.send(Msg::Containers(entries)).is_err() {
                                break;
                            }
                            for id in ids {
                                if let Some((cpu, mem)) = stats_once(&docker, &id).await {
                                    if tx
                                        .send(Msg::ContainerStats {
                                            id,
                                            cpu,
                                            mem_bytes: mem,
                                        })
                                        .is_err()
                                    {
                                        return;
                                    }
                                }
                            }
                            tracing::debug!(
                                found,
                                duration_ms = started.elapsed().as_millis() as u64,
                                "docker scan complete"
                            );
                        }
                        Err(err) => {
                            tracing::warn!(error = %err, "docker scan failed");
                            if tx
                                .send(Msg::CollectorFailed(format!("docker: {err}")))
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    tokio::time::sleep(interval).await;
                }
            });
        });
    if let Err(e) = result {
        tracing::warn!(error = %e, "failed to spawn docker collector; containers disabled");
    }
}

/// Follow `docker logs -f`, streaming lines tagged with `gen`. Exits when
/// the stream ends, errors, or `stop` flips.
#[allow(unused_variables)]
pub fn spawn_log_follow(tx: Tx, gen: u64, id: String, name: String, stop: StopFlag) {
    #[cfg(feature = "docker")]
    {
        spawn_docker_log_follow_inner(tx, gen, id, stop);
    }
    #[cfg(not(feature = "docker"))]
    {
        let _ = (tx, gen, id, name, stop);
    }
}

#[cfg(feature = "docker")]
fn spawn_docker_log_follow_inner(tx: Tx, gen: u64, id: String, stop: StopFlag) {
    let result = std::thread::Builder::new()
        .name("portly-docker-logs".into())
        .spawn(move || {
            let Ok(docker) = connect() else {
                let _ = tx.send(Msg::LogLine {
                    gen,
                    line: "docker daemon unreachable".into(),
                });
                return;
            };
            let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            rt.block_on(async move {
                let options = LogsOptionsBuilder::new()
                    .follow(true)
                    .stdout(true)
                    .stderr(true)
                    .tail("500")
                    .timestamps(false)
                    .build();
                let mut stream = docker.logs(&id, Some(options));
                while let Some(item) = stream.next().await {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    match item {
                        Ok(chunk) => {
                            let text = match chunk {
                                LogOutput::StdOut { message }
                                | LogOutput::StdErr { message }
                                | LogOutput::Console { message } => {
                                    String::from_utf8_lossy(&message).into_owned()
                                }
                                LogOutput::StdIn { .. } => continue,
                            };
                            for line in text.lines() {
                                if tx
                                    .send(Msg::LogLine {
                                        gen,
                                        line: line.to_string(),
                                    })
                                    .is_err()
                                {
                                    return;
                                }
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Msg::LogLine {
                                gen,
                                line: format!("log stream error: {e}"),
                            });
                            return;
                        }
                    }
                }
            });
        });
    if let Err(e) = result {
        tracing::warn!(error = %e, "failed to spawn docker log follower");
    }
}
