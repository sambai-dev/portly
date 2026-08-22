# Portly

> **What's running on :3000 again?** Portly answers that in one glance — and inspects, restarts, or kills it in one more.

Portly is a terminal cockpit for your local dev machine. It discovers every listening port and maps each one to the owning process **or Docker container** — dev servers, databases, message queues — then streams CPU trends, memory, health dots, and logs next to each entry, all actionable from one pane. No config file, no daemon, no setup: useful five seconds after install.

```
┌─ Portly · 7 services · sort:port · data 1s ────────────────────────────────────┐
│  PORT   PROTO  PID     PROCESS      TREND     CPU%   MEM    HEALTH   SRC       │
│▶ 3000   tcp    4121    node         ▂▃▅▆▇▇█    2.5   182M   -        proc      │
│  5432   tcp    889     postgres     ▁▁▁▁▁▁▁     0.4    96M   -        proc      │
│  5432   tcp    -       pg-dev       ▃▃▄▄▄▄▄     1.2   340M   -        docker    │
│  6379   tcp    1188    redis        ▁▂▁▁▁▁▁     0.1  1024K   ● 12ms   proc      │
│                                                                                │
│ [j/k] move · [x] act · [R/T/G] container · [l] logs · [s] sort · [/] filter … │
└────────────────────────────────────────────────────────────────────────────────┘
```

*(demo GIF: run `vhs demo.tape` — the tape is committed so anyone can regenerate it)*

---

## Why this exists

Every developer runs `lsof -i :3000` / `netstat -ano | findstr 3000` / `docker ps` several times a day, then hops terminal tabs to tail logs. The information exists; it's scattered across four tools with four output formats. Portly unifies discovery → context → action:

| You want to… | Before | With Portly |
|---|---|---|
| Find what owns port 5432 (host *and* container) | `lsof` + `docker ps` + manual join | read two adjacent rows |
| Kill the zombie vite server | find PID → `kill -9` → hope | `x`, Enter |
| Restart a container mid-debug | `docker restart <id>` | `R`, Enter |
| Watch your stack breathe | `htop` + mental filtering | TREND column |
| Know if it's actually serving | curl in another tab | ●/◐/✗ dot + latency |
| See why it's failing | tmux tab-hopping to logs | `l` |

## Landscape (measured Aug 2026)

No established tool combines port discovery + containers + logs/health + actions. Adjacent tools each hold one slice — full analysis in [`docs/RESEARCH.md`](docs/RESEARCH.md), gap-by-gap closure evidence in [`docs/GAPS.md`](docs/GAPS.md):

| Tool | ★ | Covers | Misses |
|---|---|---|---|
| lazydocker | 52.6k | container TUI | host processes & ports |
| k9s | 34.4k | K8s navigator | local machine |
| ctop | 17.8k | container metrics | unmaintained; host procs |
| killport | 1.8k | CLI kill-by-port | no live view |
| process-compose | 2.7k | YAML-declared procs | can't discover what's already running |
| btop / bottom | 34.1k / 13.9k | system resources | no port↔service semantics |

## Measured numbers

Windows 11 Enterprise · i9-14900KF · 64 GB RAM · rustc 1.98.0 · release build (`lto`, `strip`) · 244 live services incl. a running Postgres container. Reproduce with `cargo run --release --example bench 100`.

| Metric | Target | Measured | Method |
|---|---|---|---|
| Scan p50 (sockets→PID→stats) | < 50 ms | **26.8 ms** | `bench` example, n=100, steady-state cadence |
| Scan p99 | < 50 ms | **37.8 ms** | same |
| Idle TUI working set | < 30 MB | **25 MB** (16 MB private) | WorkingSet64 @ 4 s alive |
| `--once` end-to-end median | — (info) | **512.7 ms** min 493 / max 568 | process spawn → full table printed, incl. Docker round-trip |
| Time-to-first-frame | < 100 ms | instant | TUI paints empty table immediately; data arrives async by design |
| Tests | — | 26 green | update transitions, pools, frames, health, config |

Honest note: the 500 ms once-mode figure is dominated by the Docker Engine round-trip and sysinfo's cold refresh — the interactive TUI never blocks on either.

## Architecture

Elm-style unidirectional state over independent collectors merged on one channel:

```
┌────────────────────────────────────────────────────┐
│ view.rs      pure fn(model, theme, clock) — no I/O │
├────────────────────────────────────────────────────┤
│ model.rs     Model + Msg + update() -> [Effect]    │
├────────────────────────────────────────────────────┤
│ runtime      input → update → effects → render     │
│              mouse capture · geometry feedback     │
├────────────────────────────────────────────────────┤
│ collectors/  sockets+proc (thread)                 │
│ docker.rs    list/stats/actions/logs (tokio)       │
│ health.rs    opt-in prober, 8 workers              │
│ logs.rs      docker follow + file tailers          │
├────────────────────────────────────────────────────┤
│ netstat2 · sysinfo · bollard · std::net           │
└────────────────────────────────────────────────────┘
```

Design decisions with tradeoffs live in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md). Two worth reading:

- **Pool replacement semantics** — a slow/broken Docker daemon can never blank host rows; each collector owns its rows exclusively.
- **Generation-counted log panes** — closing a pane increments a generation; stale followers' lines are dropped at the model boundary, so ghosts are impossible by construction.

## Features

- **Zero-config discovery** — listening ports → owners → stats on first launch, every platform.
- **Containers as first-class rows** — ports, state, CPU/mem via Engine API; actions with arm-and-confirm.
- **Two-step destructive actions** — `x` arms contextually (kill proc / restart-start container); Enter confirms; PID-reuse guard re-verifies ownership before terminating.
- **Logs pane** — `l`: docker log streaming for containers, file-follow for processes via `[log_files]`; pgup/pgdn scroll, follow-state indicator.
- **Health dots** — opt-in `●` up / `◐` degraded / `✗` down + latency ms.
- **Trend sparklines** — per-row ▁▂▃▄▅▆▇█ over a 14-sample ring buffer; blanks until two real samples exist (no fake zeros).
- **Config that only overrides** — `interval_ms`, sort, `ignore_ports`, labels, log files, theme, `[docker]`, `[health]`. Missing file is valid config. See [`config.example.toml`](config.example.toml).
- **Themes** — `dark`, `light`, `nord`.
- **Mouse** — click selects, wheel scrolls.
- **Staleness indicator** — title shows data age and flags STALE past 2× interval.
- **Headless mode** — `portly --once` prints an aligned table for scripts/CI.
- **Shell completions** — `portly completions <bash|zsh|fish|powershell>`.
- **Structured logging** — tracing spans per scan cycle to file (`PORTLY_LOG` path override, `RUST_LOG` verbosity), never stderr.

## Keybindings

| Key | Action |
|---|---|
| `↑`/`↓`, `j`/`k` | move selection |
| click / wheel | select row / scroll |
| `/` | filter by text (port, proto, pid, name, state) |
| `s` | cycle sort: port → cpu → mem |
| `l` | toggle logs pane for selection |
| `pgup`/`pgdn` | scroll logs while open (`esc` closes) |
| `x` | arm contextual action (kill proc · restart/start container) |
| `R` / `T` / `G` | arm restart / stop / start for containers |
| `Enter` | execute armed action |
| `Esc` | cancel armed / clear filter / close pane |
| `?` | help overlay |
| `q`, `Ctrl-C` | quit |

## Install

```sh
# Homebrew (formula committed at Formula/portly.rb — tap instructions below)
brew install sambai-dev/portly/portly

# Prebuilt binaries
cargo binstall portly
curl -fsSL https://raw.githubusercontent.com/sambai-dev/portly/main/install.sh | sh   # mac/linux
irm https://raw.githubusercontent.com/sambai-dev/portly/main/install.ps1 | iex       # windows

# From source
cargo install portly
```

Release CI builds binaries for linux-x86_64 (gnu + musl), macOS (aarch64 + x86_64), and Windows x86_64-msvc on every `v*` tag.

## Scripting

```sh
portly --once                      # snapshot table to stdout
portly --once --interval-ms 200    # faster cold scan
portly completions zsh >> ~/.zshrc # shell completions
PORTLY_CONFIG=./ci.toml portly     # project-local config
```

## Honest limitations

- UDP owner mapping is best-effort per OS kernel limits; TCP listeners are the reliable core (you will see ephemeral UDP noise from chatty apps — `ignore_ports` exists for exactly this).
- First CPU tick renders blanks: sysinfo needs two samples ≥200 ms apart. We show `-`, never fake zeros.
- Health probes are plain HTTP/1.0 against localhost — no TLS by design; local dev services rarely need it.
- Container stats use one-shot sampling at half the host cadence (≤16 running containers sampled per tick) to bound Engine API load.
- Windows-first testing; Linux/macOS verified in CI (build + tests) — field reports welcome.

## Development

```sh
cargo build                          # default features (docker)
cargo build --no-default-features   # lean build without bollard
cargo test                           # 26 tests
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo run                            # the TUI
cargo run --release --example bench 100   # reproduce numbers above
vhs demo.tape                        # regenerate demo.gif
```

CI gates every push on fmt + clippy + tests across Linux/macOS/Windows and publishes release binaries on tags.

## License

MIT — see [LICENSE](LICENSE). Roadmap and week-by-week plan: [`docs/ROADMAP.md`](docs/ROADMAP.md).
