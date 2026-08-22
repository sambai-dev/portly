# Portly

> **What's running on :3000 again?** Portly answers that in one glance — and kills, restarts, or inspects it in one more.

Portly is a terminal UI cockpit for your local dev machine. It discovers every listening port and maps each one to the owning process or Docker container — dev servers, databases, message queues — then streams live CPU/memory/health next to each entry. No config file, no daemon, no setup: useful five seconds after install.

```
┌─ Portly ─ listening sockets ────────────────────────────────┐
│  PORT   PROTO  PID    PROCESS          CPU%   MEM     SRC    │
│▶ 3000   tcp    4121   node (vite)      2.1   182MB   proc  │
│  5432   tcp    889    postgres         0.4   96MB    proc  │
│  8080   tcp    7742   com.docker.b...  1.2   340MB   docker│
│  6379   tcp    1188   redis-server     0.1   12MB    proc  │
│                                                             │
│ 7 services · refresh 500ms · [x]kill [s]ort [/]filter [?]help │
└──────────────────────────────────────────────────────────────┘
```

*(demo GIF placeholder — generated with VHS before public launch)*

---

## Why this exists

Every developer runs `lsof -i :3000` / `netstat -ano | findstr 3000` / `docker ps` several times a day, then hops between tmux panes to tail logs. The information exists; it's just scattered across four tools with four output formats.

Portly unifies discovery → context → action in one pane:

| You want to… | Before | With Portly |
|---|---|---|
| Find what owns port 5432 | `lsof`/`netstat` + manual PID join | read row 2 |
| Kill the zombie vite server | find PID, `kill -9`, hope | highlight, `k`, Enter |
| See which containers expose ports | `docker ps --format …` | same table |
| Watch CPU/mem of your stack | `htop` + mental filtering | sparkline column |

## Landscape (measured Aug 2026)

No established tool combines port discovery + Docker + logs/health + actions. Adjacent tools each cover a slice:

| Tool | ★ | What it covers | What it misses |
|---|---|---|---|
| lazydocker | 52.6k | container TUI: logs/restart/metrics | host processes, ports |
| k9s | 34.4k | Kubernetes pods→logs→actions | local machine |
| ctop | 17.8k | container metrics (unmaintained) | actions, host procs |
| killport | 1.8k | CLI kill-by-port | no view, no monitoring |
| process-compose | 2.7k | YAML-defined procs + TUI | doesn't discover existing services |
| btop / bottom | 34.1k / 13.9k | system resources | no port↔service mapping |

Full competitive analysis and technical feasibility notes: [`docs/RESEARCH.md`](docs/RESEARCH.md).

## Architecture

Elm-style unidirectional state over a collector layer of independent tokio tasks:

```
┌───────────────────────────────────────────────┐
│ TUI (ratatui) — Elm architecture              │
│  Model (app state) ◀── Msg ◀── event streams  │
│  update : Model -> Msg -> (Model, [Effect])   │
│  view   : &Model -> Frame (pure)              │
└──────────────┬────────────────────────────────┘
               │ commands (kill, quit)
┌──────────────▼────────────────────────────────┐
│ Collector layer (tokio tasks, one per source) │
│  - socket scan → PID map (netstat2 /proc fast)│
│  - Docker API (containers, ports, stats)      │
│  - process stats (CPU/mem via sysinfo)        │
│  - optional: HTTP health probes per service   │
└──────────────┬────────────────────────────────┘
               │ mpsc Msg stream
        (merged into the TUI event loop)
```

Details, data model, and testing strategy: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## Design decisions (with tradeoffs)

1. **Elm-style unidirectional state.** `update()` is pure and returns *effects* (`Kill(pid)`, `Quit`) that the runtime executes outside the model. That makes every keybinding testable without spawning processes, and rendered frames snapshot-testable. *Tradeoff:* more ceremony than direct mutation; worth it for a tool whose core interactions destroy processes.

2. **Zero-config discovery is the product thesis.** If you need a TOML file listing your services, you've rebuilt foreman. Portly must show value with zero flags. *Tradeoff:* heuristic labeling can misidentify; we mitigate by always showing raw evidence (PID, command line, container ID) next to any inferred name.

3. **Portable base, platform fast paths.** `netstat2` gives socket→PID on all three OSes out of the box; Linux gets an optional `/proc` fast path, macOS falls back to parsing `lsof`. Windows is first-class from day one (it's where most devs actually run dev servers). *Tradeoff:* abstraction boundary adds a small indirection layer; documented in ARCHITECTURE.

4. **tracing everywhere, from commit one.** Each collector cycle emits a span with duration and result counts; structured logs go to a file so they never corrupt the TUI. This is exactly the observability discipline Rust teams probe for in interviews.

## Performance targets (published after v0.1)

| Metric | Target | Measured |
|---|---|---|
| Startup to first frame | < 100 ms | TODO (hyperfine) |
| Refresh tick p99 | ≤ 500 ms budget, scan < 50 ms | TODO |
| Idle RSS | < 30 MB | TODO |
| Kill action round-trip | < 200 ms perceived | TODO |
| Render allocations | O(visible rows) only | TODO (flamegraph) |

## Honest limitations

- UDP owner mapping is best-effort on all platforms (kernel limitations); TCP listeners are the reliable core.
- Docker integration requires access to the Docker socket/pipe; no root escalation tricks.
- Process names come from the OS; a process listening on a socket may have exited between scan and display — Portly guards against PID reuse at action time.
- macOS uses `lsof` parsing until a maintained sysctl PCB-list binding exists (see RESEARCH §feasibility).
- CPU% needs two samples ≥200 ms apart (sysinfo constraint); first tick shows blanks rather than fake numbers.

## Install (planned at launch)

```sh
brew install portly-tap/portly/portly   # Homebrew tap
cargo binstall portly                    # prebuilt binaries
cargo install portly                     # from source
# scoop / winget / AUR: tracked in ROADMAP week 5
```

## Keybindings (v0.1)

| Key | Action |
|---|---|
| `↑`/`↓`, `j`/`k` | move selection |
| `/` | filter by text (process, port, container) |
| `s` | cycle sort: port → CPU → memory |
| `x` | arm kill (shows confirm banner) |
| `Enter` | execute armed action |
| `Esc` | cancel / clear filter |
| `?` | help overlay |
| `q` or `Ctrl-C` | quit |

## Development

```sh
cargo build            # debug build
cargo test             # unit + frame tests
cargo clippy -- -D warnings
cargo fmt --check
RUST_LOG=debug portly  # tracing to portly.log
```

CI enforces fmt + clippy `-D warnings` + tests and publishes release binaries for Linux (gnu+musl), macOS (aarch64+x86_64), and Windows on every tag. License: MIT. Roadmap: [`docs/ROADMAP.md`](docs/ROADMAP.md).
