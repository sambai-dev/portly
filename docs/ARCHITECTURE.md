# Portly — Architecture

## Layers

```
┌────────────────────────────────────────────────────┐
│ view.rs      pure fn(model, frame) — no I/O        │
├────────────────────────────────────────────────────┤
│ model.rs     Model + Msg + update()                │
│              update: (Model, Msg) -> (Model, Vec<Effect>) │
├────────────────────────────────────────────────────┤
│ runtime      main loop: input → update → effects   │
│              → collectors → draw                   │
├────────────────────────────────────────────────────┤
│ collectors/  tokio tasks, one per source           │
│  sockets    netstat2 base (+ /proc fast path)     │
│  process    sysinfo CPU/mem enrichment            │
│  docker     bollard streams (week 2)              │
│  health     optional HTTP probes (week 3)         │
├────────────────────────────────────────────────────┤
│ platform/    SocketSource trait + OS backends     │
└────────────────────────────────────────────────────┘
```

Communication is a single `tokio::sync::mpsc::UnboundedSender<Msg>`. Collectors push `Msg::ScanResult(..)` / `Msg::StatsTick(..)`; the runtime pushes keyboard `Msg::Key(..)`; ticks come from the interval task. One channel = one merge point, trivially testable.

## Elm core

```rust
pub struct Model {
    pub entries: Vec<PortEntry>,   // last known world state
    pub selected: usize,
    pub sort: SortKey,             // Port | Cpu | Mem | Process
    pub filter: Option<String>,
    pub pending_kill: Option<u32>, // two-step confirm
    pub show_help: bool,
    pub last_scan: Option<Instant>,
}

pub enum Msg {
    Key(KeyCode),
    ScanResult(Vec<PortEntry>),
    Tick,
    Quit,
}

pub enum Effect {
    Kill(u32),
    Quit,
}
```

Rules:
- `update(&mut Model, Msg) -> Vec<Effect>` is **pure** (deterministic, no side effects). Effects run in the runtime after update returns.
- `view` renders only from `Model`; identical models render byte-identical frames. This is what makes snapshot tests possible.
- Selection is an index into the *visible* (filtered+sorted) list; re-sorting clamps it to the same entry by stable key (PID+port), never just position.

## Data model

```rust
pub struct PortEntry {
    pub port: u16,
    pub proto: Protocol,          // Tcp | Udp
    pub pid: Option<u32>,         // None = owner not visible to us
    pub process: Option<String>,  // exe basename
    pub cmdline: Option<String>,  // truncated evidence
    pub cpu: Option<f32>,         // None until second sample
    pub mem_bytes: Option<u64>,
    pub source: Source,           // Proc | Docker | Kernel
    pub container_id: Option<String>,
}
```

`Option` fields are rendered as blanks — the UI must never invent zeros.

## Platform abstraction

```rust
pub trait SocketSource {
    fn listening_sockets(&mut self) -> std::io::Result<Vec<RawSocket>>;
}
```

| Platform | Backend | Notes |
|---|---|---|
| Windows | netstat2 (`GetExtendedTcp/UdpTable`) | first-class from day one |
| Linux | netstat2 (netlink sock-diag); optional `procfs` fast path behind feature flag | /proc join O(procs×fds), fine at dev scale |
| macOS | netstat2 where possible; `lsof -iTCP -sTCP:LISTEN -P -n` parse fallback | libproc lacks socket tables |

Backends are selected at startup; CI runs per-OS integration tests against the real backend on each runner.

## Collectors

One tokio task per source, each owning its cadence and pushing merged `Msg`s:

1. **sockets** — scan every `interval_ms` (default 500). Emits full replacement sets (not diffs): simpler, idempotent, tables are small (<100 rows typical).
2. **process** — sysinfo refresh ≥200 ms after previous sample so CPU% is valid; joins stats onto existing entries rather than emitting its own list.
3. **docker** *(week 2)* — bollard `containers_list` + streaming `stats`/`logs`; container ports merged into the same table with `source: Docker`.
4. **health** *(week 3)* — opt-in HTTP probes per port with short timeout; emits status dots.

Failure isolation: a collector that errors sends `Msg::CollectorFailed(source, err)` once, then backs off exponentially; the UI shows a degraded badge for that source while everything else keeps working. Docker missing must never blank the whole table.

## Runtime loop

```
loop {
    if event::poll(250ms)? { map KeyEvent → Msg::Key }        // input
    while let Ok(msg) = rx.try_recv() { effects += update() } // drain
    for e in effects { execute(e) }                            // kill/quit
    draw(view(model))                                          // render
}
```

250 ms poll keeps keystrokes snappy while collector msgs arrive asynchronously between polls.

## Testing strategy

- **update tests:** pure transitions — filter, sort cycling, selection clamp, kill-arm→confirm→effect, Esc cancel. No terminal needed.
- **frame tests:** ratatui `TestBackend(80×24)` renders a seeded model; assert on buffer text (deterministic, self-contained). insta available for full-frame goldens when UI stabilizes.
- **collector contract tests:** fake `SocketSource` returning canned rows drives the same code path real backends use.
- **platform matrix:** GitHub runners linux/macos/windows execute backend-specific integration tests.

## Observability

- `tracing` spans per scan: `collector=sockets duration_ms=12 found=9`.
- Output goes to a log file (never stderr — it would corrupt the TUI). `RUST_LOG` controls level; default `warn`, `debug` for development.
- Perf SLOs (startup <100 ms, scan <50 ms p99, idle RSS <30 MB) measured with hyperfine + RSS sampler and published in README post-v0.1.

## Security & safety

- Kills always require arm + confirm; no auto-kill anywhere, ever.
- PID-reuse guard: before executing `Effect::Kill(pid)`, runtime re-verifies the PID still owns the expected port; aborts otherwise.
- Portly reads system tables and talks to Docker API only; it does not open listening sockets itself.
- Health probes are strictly opt-in.

## Config sketch (week 4)

`~/.config/portly/config.toml` (`%APPDATA%\portly\config.toml` on Windows)
```toml
interval_ms = 500
sort = "port"
[health]           # off by default
enabled = false
timeout_ms = 750
[docker]
enabled = true
```

Zero-config remains the thesis: the file exists only to *override* good defaults, never required.

---

## Addendum — v0.1.0 shipped modules

The tables above describe the plan; these are the as-built additions.

### Docker collector (`docker.rs`, feature-gated)

Owns its rows exclusively: `Msg::Containers` replaces only rows where
`source == Docker`; host rows survive any Docker failure by construction
(pool-replacement truth table in `model::replace_pool`). Runs on a dedicated
OS thread hosting a current-thread tokio runtime; discovery at 2× the host
interval; one-shot stats per running container capped at 16/tick using the
Engine-API CPU-delta formula; all option structs come from the OpenAPI-generated
`bollard::query_parameters` builders (the legacy `container::*Options` are
deprecated in bollard 0.19).

### Health prober (`health.rs`)

Off by default. A shared `Arc<Mutex<BTreeSet<u16>>>` port feed is refreshed by
the runtime on every host scan; the prober rounds through it every
`[health].interval_ms` with 8 scoped workers and plain HTTP/1.0 GETs
(std::net only — no TLS dependency for localhost probes). Results flow as
`Msg::HealthUpdate(HashMap<u16, (Health, Option<u16 ms>)>)`.

### Log followers (`logs.rs` + docker log follow)

Every pane session mints a monotonically increasing generation. Lines arrive
as `Msg::LogLine { gen, line }`; the model drops lines whose gen is stale, so
a closed pane cannot be resurrected by an in-flight follower. Followers exit
via `Arc<AtomicBool>` stop flags checked between chunks/reads.

### Mouse & geometry

crossterm mouse capture feeds `Msg::Mouse(Click(y)|ScrollUp|ScrollDown)`.
Rendering stays pure: each frame publishes the table-body rect through a
thread-local slot (`view::take_geometry`), which the runtime converts into
`Msg::FrameGeometry` for hit-testing and viewport-height tracking — geometry
flows *through* the Elm loop, never around it. The table's scroll offset lives
in the model; `update()` re-clamps it after every message so the selected row
is always inside the rendered window (the wheel moves the selection, which
drags the view — it never scrolls independently).

### Headless mode

`--once` runs one synchronous scan (+ optional Docker list) and prints the
aligned table from `view::snapshot_table`. This doubles as the benchmarking
entry point (`examples/bench.rs` measures steady-state cadence over n runs).
