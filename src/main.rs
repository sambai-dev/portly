mod collectors;
mod model;
mod view;

use std::io::stdout;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc;

use model::{Effect, Key, Model, Msg};

#[derive(Parser)]
#[command(
    name = "portly",
    version,
    about = "What's running on your ports? Dev servers, DBs, containers — one live pane.",
    long_about = "Portly discovers every listening port on your machine, maps each to its \
                  owning process or container, and lets you inspect or kill it with two \
                  keystrokes. Zero config."
)]
struct Cli {
    /// Collector refresh interval in milliseconds
    #[arg(long, default_value_t = 500)]
    interval_ms: u64,
}

fn main() -> color_eyre::Result<()> {
    let cli = Cli::parse();
    color_eyre::install()?;
    init_tracing();

    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    set_panic_hook();

    let (tx, rx) = mpsc::unbounded_channel::<Msg>();
    let _collector = collectors::spawn_collector(tx, Duration::from_millis(cli.interval_ms));

    run_tui(rx)?;
    restore_terminal();
    Ok(())
}

fn run_tui(mut rx: mpsc::UnboundedReceiver<Msg>) -> color_eyre::Result<()> {
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut app = Model::new();
    loop {
        // 1. Drain keyboard input (250ms poll keeps keystrokes snappy while
        //    collector messages arrive asynchronously between polls).
        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press {
                    if let Some(msg) = map_key(k) {
                        if apply(&mut terminal, &mut app, msg)? {
                            return Ok(());
                        }
                    }
                }
            }
        }
        // 2. Drain everything the collectors produced since last frame.
        while let Ok(msg) = rx.try_recv() {
            if apply(&mut terminal, &mut app, msg)? {
                return Ok(());
            }
        }
        // 3. Pure render from state.
        terminal.draw(|f| view::render(f, &app))?;
    }
}

/// Run one update + effect cycle; returns true when the app should exit.
fn apply(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut Model,
    msg: Msg,
) -> color_eyre::Result<bool> {
    for effect in model::update(app, msg) {
        match effect {
            Effect::Quit => return Ok(true),
            Effect::Kill { pid, port } => match collectors::process::verify_and_kill(pid, port) {
                Ok(()) => {
                    tracing::info!(pid, port, "process terminated");
                    app.pending_kill = None;
                    app.status = format!("terminated pid {pid} on :{port}");
                }
                Err(err) => {
                    tracing::warn!(pid, port, error = %err, "kill aborted");
                    app.pending_kill = None;
                    app.last_error = Some(err);
                }
            },
        }
    }
    terminal.draw(|f| view::render(f, app))?;
    Ok(false)
}

fn map_key(k: KeyEvent) -> Option<Msg> {
    use KeyCode::*;
    let key = match k.code {
        Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => Key::ForceQuit,
        Up => Key::Up,
        Down => Key::Down,
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
    let _ = execute!(stdout(), LeaveAlternateScreen);
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
