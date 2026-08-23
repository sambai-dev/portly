use portly::collectors;
use portly::config;
#[cfg(feature = "docker")]
use portly::docker;
use portly::health;
use portly::logs;
use portly::model;
use portly::view;

use std::collections::{BTreeSet, HashMap};
use std::io::stdout;
use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clap::{CommandFactory, Parser, Subcommand};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use config::Config;
use model::{ContainerAction, Effect, Key, Model, MouseMsg, Msg};

#[derive(Parser)]
#[command(
    name = "portly",
    version,
    about = "What's running on your ports? Dev servers, DBs, containers — one live pane.",
    long_about = "Portly discovers every listening port on your machine, maps each to its \
                  owning process or container, streams logs and health next to it, and lets \
                  you act with two keystrokes. Zero config."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Collector refresh interval in milliseconds (overrides config)
    #[arg(long)]
    interval_ms: Option<u64>,

    /// Print one snapshot table and exit (headless / scripting)
    #[arg(long)]
    once: bool,

    /// Config file path (default: $PORTLY_CONFIG or system config dir)
    #[arg(short, long)]
    config: Option<std::path::PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// Generate shell completions
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

fn main() -> color_eyre::Result<()> {
    let cli = Cli::parse();
    color_eyre::install()?;
    init_tracing();

    if let Some(Command::Completions { shell }) = cli.command {
        let mut cmd = Cli::command();
        clap_complete::generate(shell, &mut cmd, "portly", &mut std::io::stdout());
        return Ok(());
    }

    let mut cfg = Config::load_with(cli.config.as_deref());
    if let Some(ms) = cli.interval_ms {
        cfg.interval_ms = ms.clamp(100, 60_000);
    }

    if cli.once {
        // Audit U2: headless consumers parse stdout; a failed scan must be
        // one plain stderr line plus a non-zero exit — never a color-eyre
        // multi-page report.
        if let Err(err) = run_once_mode(&cfg) {
            eprintln!("{}", once_failure_line(&err));
            std::process::exit(1);
        }
        return Ok(());
    }

    // Audit D1: the alt-screen dashboard assumes a human on a TTY; piped or
    // redirected stdout would collect escape-sequence garbage forever (the
    // input loop never sees EOF). Refuse instead of hanging.
    if let Some(reason) = dashboard_tty_blocker(stdout().is_terminal()) {
        eprintln!("portly: {reason}");
        std::process::exit(2);
    }

    run_tui(cfg)
}

/// Pure TTY-branch decision (unit-tested): returns the stderr explanation when
/// the interactive dashboard must refuse to start.
fn dashboard_tty_blocker(is_terminal: bool) -> Option<&'static str> {
    (!is_terminal)
        .then_some("live dashboard needs a terminal; use `portly --once` for a single snapshot")
}

// ------------------------------------------------------------ headless ----
/// Errors surface via [`once_failure_line`] at the call site, so a scan
/// failure can never fall through to color-eyre.
fn run_once_mode(cfg: &Config) -> std::io::Result<()> {
    let started = Instant::now();
    #[cfg_attr(not(feature = "docker"), allow(unused_mut))]
    let mut entries = collectors::run_once(&cfg.ignore_ports, &cfg.labels)?;

    #[cfg(feature = "docker")]
    if cfg.docker.enabled {
        if let Some(mut containers) = docker_once_blocking() {
            containers.retain(|e| !cfg.ignored(e.port));
            entries.extend(containers);
        }
    }

    let mut m = Model::new();
    m.entries = entries;
    m.sort = cfg.sort;
    print!("{}", view::snapshot_table(&m));
    tracing::info!(
        duration_ms = started.elapsed().as_millis() as u64,
        services = m.entries.len(),
        "once-mode scan"
    );
    Ok(())
}

#[cfg(feature = "docker")]
fn docker_once_blocking() -> Option<Vec<model::PortEntry>> {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            warn_docker_skipped(&format!("runtime unavailable ({err})"));
            return None;
        }
    };
    // A hung daemon must never hang the headless path — but silence would make
    // a failed source indistinguishable from zero containers (audit D2), so
    // every skip explains itself on stderr. stdout stays table-only.
    rt.block_on(async {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            match docker::connect() {
                Ok(docker) => match docker::list_entries(&docker).await {
                    Ok(entries) => Some(entries),
                    Err(err) => {
                        warn_docker_skipped(&format!("list failed ({err})"));
                        None
                    }
                },
                Err(_) => {
                    warn_docker_skipped("connection failed");
                    None
                }
            }
        })
        .await
        .unwrap_or_else(|_| {
            tracing::warn!("docker list timed out in --once mode");
            warn_docker_skipped("timed out after 5s");
            None
        })
    })
}

/// Audit D2: one concise stderr line naming the skipped source; also traced.
#[cfg(feature = "docker")]
fn warn_docker_skipped(reason: &str) {
    tracing::warn!("{}", docker_skip_line(reason));
    eprintln!("{}", docker_skip_line(reason));
}

/// Pure message builder (unit-tested).
#[cfg(feature = "docker")]
fn docker_skip_line(reason: &str) -> String {
    format!("portly: docker: skipping containers ({reason})")
}

/// Pure message builder (unit-tested): audit U2 — one plain stderr line for a
/// failed `--once` scan.
fn once_failure_line(err: &std::io::Error) -> String {
    format!("portly: scan failed: {err}")
}

// ------------------------------------------------------------------ TUI ---

fn run_tui(cfg: Config) -> color_eyre::Result<()> {
    // Arm the restoring panic hook BEFORE touching terminal state so any
    // failure below (or a panic anywhere later) still hands back a sane tty.
    set_panic_hook();
    enable_raw_mode()?;
    if let Err(err) = execute!(stdout(), EnterAlternateScreen) {
        restore_terminal();
        return Err(err.into());
    }
    // Mouse capture is best-effort polish; failure must not kill the app.
    let _ = execute!(stdout(), EnableMouseCapture);

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Msg>();
    let interval = Duration::from_millis(cfg.interval_ms);

    let _host_collector = collectors::spawn_collector(
        tx.clone(),
        interval,
        cfg.ignore_ports.clone(),
        cfg.labels.clone(),
    );

    #[cfg(feature = "docker")]
    let _docker_collector = if cfg.docker.enabled {
        docker::spawn_collector(tx.clone(), interval.mul_f64(2.0), cfg.ignore_ports.clone());
        Some(())
    } else {
        None
    };

    let health_feed: Option<health::PortFeed> = if cfg.health.enabled {
        health::spawn_prober(tx.clone(), cfg.health.clone()).map(|(feed, _handle)| feed)
    } else {
        None
    };

    let exit_code = tui_loop(rx, tx.clone(), cfg, health_feed);

    restore_terminal();
    exit_code
}

fn tui_loop(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<Msg>,
    tx: tokio::sync::mpsc::UnboundedSender<Msg>,
    cfg: Config,
    health_feed: Option<health::PortFeed>,
) -> color_eyre::Result<()> {
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut app = Model::new();
    app.interval = Duration::from_millis(cfg.interval_ms);
    app.sort = cfg.sort;
    app.health_enabled = cfg.health.enabled;
    app.cfg_log_files = cfg.log_files.clone();

    // gen → stop-flag registry for log followers.
    let mut followers: HashMap<u64, Arc<AtomicBool>> = HashMap::new();
    let theme = cfg.theme;

    loop {
        // 1. Input (250ms poll keeps keystrokes snappy between async msgs).
        if event::poll(Duration::from_millis(250))? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => {
                    if let Some(msg) = map_key(k) {
                        if apply(&mut terminal, &mut app, &theme, msg, &tx, &mut followers)? {
                            return Ok(());
                        }
                    }
                }
                Event::Mouse(me) if app.mouse => {
                    let msg = match me.kind {
                        MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                            Some(MouseMsg::Click(me.row))
                        }
                        MouseEventKind::ScrollUp => Some(MouseMsg::ScrollUp),
                        MouseEventKind::ScrollDown => Some(MouseMsg::ScrollDown),
                        _ => None,
                    };
                    if let Some(m) = msg {
                        if apply(
                            &mut terminal,
                            &mut app,
                            &theme,
                            Msg::Mouse(m),
                            &tx,
                            &mut followers,
                        )? {
                            return Ok(());
                        }
                    }
                }
                _ => {}
            }
        }
        // 2. Collector output.
        while let Ok(msg) = rx.try_recv() {
            let was_scan = matches!(msg, Msg::ScanResult(_));
            if apply(&mut terminal, &mut app, &theme, msg, &tx, &mut followers)? {
                return Ok(());
            }
            // Feed the prober AFTER the model ingested the scan so it always
            // probes the current port set, never a cycle-stale one.
            if was_scan {
                note_health_targets(&app, &health_feed);
            }
        }
        // 3. Render (pure), then feed row geometry back for mouse hit-tests.
        terminal.draw(|f| view::render(f, &app, &theme, Instant::now()))?;
        if let Some(g) = view::take_geometry() {
            if g != app.table_area {
                let fx = model::update(&mut app, Msg::FrameGeometry(g.0, g.1, g.2, g.3));
                debug_assert!(fx.is_empty());
            }
        }
    }
}

fn note_health_targets(app: &Model, feed: &Option<health::PortFeed>) {
    if let Some(feed) = feed {
        let ports: BTreeSet<u16> = app
            .entries
            .iter()
            .filter(|e| e.proto == model::Protocol::Tcp)
            .map(|e| e.port)
            .collect();
        *feed.lock().unwrap_or_else(|p| p.into_inner()) = ports;
    }
}

/// Execute one message + its side effects; returns true to exit the app.
fn apply(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut Model,
    theme: &config::Theme,
    msg: Msg,
    tx: &tokio::sync::mpsc::UnboundedSender<Msg>,
    followers: &mut HashMap<u64, Arc<AtomicBool>>,
) -> color_eyre::Result<bool> {
    for effect in model::update(app, msg) {
        match effect {
            Effect::Quit => return Ok(true),
            Effect::Kill { pid, port } => match collectors::process::verify_and_kill(pid, port) {
                Ok(()) => {
                    tracing::info!(pid, port, "process terminated");
                    app.pending_action = None;
                    app.status = format!("terminated pid {pid} on :{port}");
                }
                Err(err) => {
                    tracing::warn!(pid, port, error = %err, "kill aborted");
                    app.pending_action = None;
                    app.last_error = Some(err);
                }
            },
            Effect::ContainerAction { id, action } => {
                spawn_container_action(tx.clone(), id, action);
            }
            Effect::OpenLogs(target) => {
                let stop = Arc::new(AtomicBool::new(false));
                followers.insert(app.log_gen, stop.clone());
                logs::spawn_tailer(tx.clone(), app.log_gen, target, stop);
            }
            Effect::CloseLogs(gen) => {
                followers.retain(|g, flag| {
                    if *g < gen {
                        flag.store(true, Ordering::Relaxed);
                        false
                    } else {
                        true
                    }
                });
            }
        }
    }
    terminal.draw(|f| view::render(f, app, theme, Instant::now()))?;
    Ok(false)
}

fn spawn_container_action(
    tx: tokio::sync::mpsc::UnboundedSender<Msg>,
    id: String,
    action: ContainerAction,
) {
    let result = std::thread::Builder::new()
        .name("portly-action".into())
        .spawn(move || {
            #[cfg(feature = "docker")]
            {
                let outcome: Result<String, String> = (|| {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|e| e.to_string())?;
                    rt.block_on(async {
                        let docker = docker::connect()?;
                        docker::perform(&docker, &id, action).await
                    })
                })();
                let _ = tx.send(Msg::ActionDone(outcome));
            }
            #[cfg(not(feature = "docker"))]
            {
                let _ = (id, action);
                let _ = tx.send(Msg::ActionDone(Err("built without docker support".into())));
            }
        });
    if let Err(e) = result {
        tracing::warn!(error = %e, "failed to spawn action worker");
    }
}

fn map_key(k: KeyEvent) -> Option<Msg> {
    use KeyCode::*;
    let key = match k.code {
        Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => Key::ForceQuit,
        Up => Key::Up,
        Down => Key::Down,
        PageUp => Key::PageUp,
        PageDown => Key::PageDown,
        Enter => Key::Enter,
        Esc => Key::Esc,
        Backspace => Key::Backspace,
        Char(c) => Key::Char(c),
        _ => return None,
    };
    Some(Msg::Key(key))
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), LeaveAlternateScreen, DisableMouseCapture);
}

fn set_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        default_hook(info);
    }));
}

/// tracing writes to a file (never stderr — that would corrupt the TUI).
/// Override location with PORTLY_LOG, verbosity with RUST_LOG.
fn init_tracing() {
    let path = std::env::var_os("PORTLY_LOG")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("portly.log"));
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_writer(FileWriter(Arc::new(Mutex::new(file))))
        .try_init();
}

struct FileWriter(Arc<Mutex<std::fs::File>>);

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for FileWriter {
    type Writer = WriterGuard<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        WriterGuard(
            self.0
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
    }
}

struct WriterGuard<'a>(std::sync::MutexGuard<'a, std::fs::File>);

impl std::io::Write for WriterGuard<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

#[cfg(test)]
mod d1_tty_guard_tests {
    use super::*;

    #[test]
    fn non_terminal_stdout_blocks_dashboard() {
        assert!(dashboard_tty_blocker(false).is_some());
    }

    #[test]
    fn terminal_stdout_runs_dashboard() {
        assert_eq!(dashboard_tty_blocker(true), None);
    }

    #[test]
    fn blocker_message_advertises_once_mode() {
        let msg = dashboard_tty_blocker(false).unwrap();
        assert!(msg.contains("--once"), "must offer the escape hatch: {msg}");
        assert!(msg.starts_with("live dashboard"), "{msg}");
    }
}

#[cfg(all(test, feature = "docker"))]
mod d2_docker_skip_tests {
    use super::*;

    #[test]
    fn skip_line_names_source_and_reason() {
        let line = docker_skip_line("connection failed");
        assert_eq!(
            line,
            "portly: docker: skipping containers (connection failed)"
        );
    }

    #[test]
    fn skip_line_is_single_line() {
        for reason in [
            "connection failed",
            "timed out after 5s",
            "list failed (boom)",
        ] {
            let line = docker_skip_line(reason);
            assert!(!line.contains('\n'), "one line only: {line:?}");
            assert!(line.starts_with("portly: docker:"), "{line}");
        }
    }
}

#[cfg(test)]
mod u2_once_failure_tests {
    use super::*;

    #[test]
    fn failure_line_is_single_plain_prefixed_line() {
        let err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let line = once_failure_line(&err);
        assert!(line.starts_with("portly: scan failed: "), "{line}");
        assert!(!line.contains('\n'), "one line only: {line:?}");
        assert!(line.contains("denied"), "must carry the error text: {line}");
    }
}
