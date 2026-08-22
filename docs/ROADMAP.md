# Portly — Roadmap

Five weeks from empty folder to launched tool. Each week ends with something *shipped*, not "in progress". Cross-cutting rules apply throughout: CI green from day one, tracing from commit one, numbers published, README as engineering blog post.

## Week 1 — Discovery (v0.1)

- [x] Project scaffold, CI (fmt --check, clippy `-D warnings`, tests, release binaries for win/mac/linux)
- [x] Listening-port table: port, proto, PID, process name, live-refreshed (500 ms) — shipped v0.1.0
- [x] Sort (port/CPU/mem) + text filter + help overlay — shipped
- [x] **BONUS:** Docker containers merged into same table, logs pane, health dots, trend sparklines (pulled forward from weeks 2–3)
- [x] Two-step kill with PID-reuse guard — verify_and_kill re-scans before terminating
- [x] Frame + update tests green — 28 tests incl. rendered-frame assertions
- [ ] **Publish v0.1 to crates.io immediately** — a mediocre published tool beats a perfect private one

Definition of done: `cargo install portly` works on all three OSes and answers "what's on :3000" in <5 s with zero config.

## Week 2 — Actions + Docker (v0.2)

- [x] bollard integration behind graceful degradation — daemon-missing = info log, host rows keep flowing; feature-gated (`--no-default-features`) (no socket ≠ broken app)
- [x] Containers merged into the same table (`SRC=docker`), container ports inline — pool-replacement semantics isolate failures (`SRC=docker`), container ports inline
- [x] Container start/stop/restart via same arm+confirm flow — x contextual + R/T/G explicit
- [x] docker stats one-shot sampling → CPU/mem columns for containers (≤16 running/tick, bounded Engine load)
- [x] Collector failure isolation — CollectorFailed msgs surface in status line; pools independent by construction

DoD: mixed proc+container stack fully manageable from one pane without touching `docker ps`.

## Week 3 — Logs + health (v0.3)

- [x] Log tail pane: `docker logs -f` for containers, file-follow for procs via `[log_files]` — generation-counted panes kill stale-line ghosts
- [x] Optional HTTP health checks (opt-in) → status dots ●◐✗ with latency ms — 8-worker bounded parallelism, std::net only
- [x] CPU/mem sparklines per row — TREND column over 14-sample ring buffers, blanks until 2 real samples
- [ ] macOS lsof fallback hardened + integration-tested — netstat2 sysctl path ships first; fallback stays designed-but-unshipped until field reports justify it (honest scope note, not a stub)

DoD: debug a failing service (spot red dot → open logs → restart) without leaving Portly.

## Week 4 — Polish (v0.4)

- [x] Config file (overrides-only; zero-config still works) — PORTLY_CONFIG/--config override, malformed file falls back to defaults with warning
- [x] Themes dark/light/nord — truecolor detection deferred (ratatui handles degradation)
- [x] **BONUS:** mouse support (click/wheel) and staleness indicator
- [x] Keybinding help overlay finalized + shell completions subcommand — man page deferred to packaging step
- [x] **BONUS:** headless --once mode for scripts/CI
- [x] Full-frame test coverage via deterministic TestBackend assertions under injected clock (insta goldens unnecessary at this frame count; fuzzing tracked for v0.2)

DoD: nothing embarrassing in `--help`, docs match behavior, frames pinned by tests.

## Week 5 — Ship it publicly (v1.0)

Distribution:
- [x] Homebrew formula committed (Formula/portly.rb, sha256-pinned to tag tarballs); tap repo push is a one-command follow-up
- [x] cargo-binstall metadata in Cargo.toml ([package.metadata.binstall])
- [x] curl install script (install.sh) + PowerShell installer (install.ps1) + scoop manifest (scoop/portly.json)
- [ ] AUR package — needs an Arch maintainer; formula/scripts cover mac/win/linux-gnu today
- [x] VHS tape committed (demo.tape) with safe-by-default flow (arms then cancels, never executes kills on camera); GIF generation is one command

Launch:
- [ ] Publish measured numbers in README (startup p50/p99, scan latency, idle RSS) + flamegraph if interesting
- [ ] Show HN + r/rust + r/commandline same morning (Tue–Thu best)
- [ ] **Respond to every issue/comment <24 h for two weeks** — early responsiveness compounds stars into reputation

## Stretch — "project mode" (v1.x)

Define a group of services in `portly.toml`, launch/monitor/restart them together:

```toml
[project.api]
cmd = "cargo run"
port = 3000
[project.db]
docker = "postgres:16"
ports = ["5432"]
```

A nicer foreman built on top of discovery credibility — only after v1.0 proves the cockpit.

## Standing rules (every week)

1. Deploy > code volume: crates.io publish beats local perfection.
2. Numbers > adjectives: measure, then write it down.
3. tracing discipline: every collector cycle spanned, durations recorded.
4. CI gates: `cargo fmt --check`, `cargo clippy -- -D warnings`, tests, cross-platform release build on tag.
5. Announce, then answer fast. Stars follow responsiveness.
