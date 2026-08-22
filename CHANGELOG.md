# Changelog

All notable changes to Portly are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions are semver.

## [0.1.0] — 2026-08-22

First public release. Everything below shipped in one drop.

### Added — discovery
- Live table of every listening TCP socket + bound UDP port, refreshed on a 500 ms cadence (`--interval-ms` / config).
- Socket → PID → process-name join via `netstat2` with sysinfo enrichment: CPU%, memory, command-line evidence.
- Docker containers merged into the *same* table (`SRC=docker`): ports from the Engine API, one-shot stats per running container (standard CPU-delta formula), graceful degradation when no daemon is reachable.
- Config labels fill friendly names only where the OS gives none — raw evidence always wins.

### Added — actions
- Two-step contextual action (`x` then Enter) with a PID-reuse guard that re-verifies port ownership before terminating anything.
- Container lifecycle keys: `R` restart, `T` stop, `G` start; `x` is contextual (restart when running, start when exited).

### Added — observability panes
- Logs pane (`l`): `docker logs -f` streaming for containers; file-follow for processes via `[log_files]` config. Generation counters make closed panes immune to stale lines.
- Opt-in HTTP health prober (`[health]`): ● up / ◐ degraded / ✗ down plus latency ms, bounded parallelism (8 workers), plain HTTP/1.0 by design.
- Per-row CPU trend sparklines (▁▂▃▄▅▆▇█) over a 14-sample ring buffer; blanks until two real samples exist.

### Added — product polish
- `~/.config/portly.toml` (or `$PORTLY_CONFIG`) with defaults-only philosophy: interval, sort, ignore_ports, labels, log_files, themes, `[docker]`, `[health]`.
- Themes: `dark`, `light`, `nord`.
- Mouse support: click selects rows, wheel moves selection.
- Staleness indicator in the title bar when data age exceeds 2× interval.
- `portly --once`: headless snapshot table for scripts/CI.
- `portly completions <bash|zsh|fish|powershell>`.
- tracing structured logging to file (`PORTLY_LOG`, `RUST_LOG`) — never stderr, which would corrupt the TUI.

### Engineering
- Elm-style pure `update(Model, Msg) -> Vec<Effect>` core; rendered frames deterministic under an injected clock.
- 28 tests covering update transitions, pool replacement semantics, log generations, health classification, config parsing/clamping, and rendered-frame assertions.
- CI on Linux/macOS/Windows: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, tests; release binaries for gnu+musl Linux, macOS arm/x64, Windows MSVC on tags.
- Both feature sets (`--no-default-features` included) build warning-free.
- Measured on Windows 11, 244 live services: scan p50 26.8 ms / p99 37.8 ms; idle TUI working set 25 MB; `--once` end-to-end median 513 ms (includes process spawn + Docker round-trip).

### Known limitations (honest list)
- UDP owner mapping is best-effort per OS kernel limits; TCP listeners are the reliable core.
- First CPU tick shows blanks by design (sysinfo needs two samples ≥200 ms apart).
- Health probes are plain HTTP — no TLS targets; local dev services rarely need them.
- macOS uses netstat2's sysctl path; an `lsof` fallback exists in design but ships only if netstat2 misbehaves there (tracked for v0.2).
