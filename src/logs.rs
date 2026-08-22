//! Log tailing: docker log streams for containers and file-follow for
//! processes. Each pane session gets a generation number plus a stop flag;
//! the UI drops lines from stale generations and followers exit when told.

use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::model::LogTarget;

pub type Tx = tokio::sync::mpsc::UnboundedSender<crate::model::Msg>;
pub type StopFlag = Arc<AtomicBool>;

/// Spawn a follower for `target`. Lines are sent as
/// `Msg::LogLine { gen, line }` until the stream ends or `stop` is set.
pub fn spawn_tailer(tx: Tx, gen: u64, target: LogTarget, stop: StopFlag) {
    match target {
        LogTarget::Container { id, name } => {
            #[cfg(feature = "docker")]
            crate::docker::spawn_log_follow(tx, gen, id, name, stop);
            #[cfg(not(feature = "docker"))]
            {
                let _ = (tx, gen, id, name, stop);
            }
        }
        LogTarget::File { path, .. } => spawn_file_follow(tx, gen, path, stop),
    }
}

fn stopped(stop: &StopFlag) -> bool {
    stop.load(Ordering::Relaxed)
}

fn spawn_file_follow(tx: Tx, gen: u64, path: PathBuf, stop: StopFlag) {
    let result = std::thread::Builder::new()
        .name("portly-file-tail".into())
        .spawn(move || {
            // Start at EOF: a logs pane shows live activity, not history.
            let mut offset = match std::fs::metadata(&path) {
                Ok(meta) => meta.len(),
                Err(e) => {
                    let _ = tx.send(crate::model::Msg::LogLine {
                        gen,
                        line: format!("cannot stat {}: {e}", path.display()),
                    });
                    0
                }
            };
            let mut carry = String::new();
            loop {
                if stopped(&stop) {
                    return;
                }
                match std::fs::File::open(&path) {
                    Ok(mut file) => {
                        if file.seek(SeekFrom::Start(offset)).is_err() {
                            std::thread::sleep(Duration::from_millis(250));
                            continue;
                        }
                        let mut buf = Vec::new();
                        match file.read_to_end(&mut buf) {
                            Ok(0) => {}
                            Ok(n) => {
                                offset += n as u64;
                                carry.push_str(&String::from_utf8_lossy(&buf));
                                while let Some(pos) = carry.find('\n') {
                                    let line: String = carry.drain(..=pos).collect();
                                    let line = line.trim_end_matches(['\r', '\n']);
                                    if tx
                                        .send(crate::model::Msg::LogLine {
                                            gen,
                                            line: line.to_string(),
                                        })
                                        .is_err()
                                    {
                                        return;
                                    }
                                }
                                if carry.len() > 64 * 1024 {
                                    // Pathological long line: flush it anyway.
                                    if tx
                                        .send(crate::model::Msg::LogLine {
                                            gen,
                                            line: carry.clone(),
                                        })
                                        .is_err()
                                    {
                                        return;
                                    }
                                    carry.clear();
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(crate::model::Msg::LogLine {
                                    gen,
                                    line: format!("read error: {e}"),
                                });
                            }
                        }
                    }
                    Err(_) => {
                        // File rotated away; wait for it to reappear.
                    }
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        });
    if let Err(e) = result {
        tracing::warn!(error = %e, "failed to spawn file tailer");
    }
}
