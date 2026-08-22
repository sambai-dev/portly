# Portly — Roadmap

Five weeks from empty folder to launched tool. Each week ends with something *shipped*, not "in progress". Cross-cutting rules apply throughout: CI green from day one, tracing from commit one, numbers published, README as engineering blog post.

## Week 1 — Discovery (v0.1)

- [x] Project scaffold, CI (fmt --check, clippy `-D warnings`, tests, release binaries for win/mac/linux)
- [ ] Listening-port table: port, proto, PID, process name, live-refreshed (500 ms)
- [ ] Sort (port/CPU/mem) + text filter + help overlay
- [ ] Two-step kill with PID-reuse guard
- [ ] Frame + update tests green
- [ ] **Publish v0.1 to crates.io immediately** — a mediocre published tool beats a perfect private one

Definition of done: `cargo install portly` works on all three OSes and answers "what's on :3000" in <5 s with zero config.

## Week 2 — Actions + Docker (v0.2)

- [ ] bollard integration behind graceful degradation (no socket ≠ broken app)
- [ ] Containers merged into the same table (`SRC=docker`), container ports inline
- [ ] Container start/stop/restart via same arm+confirm flow
- [ ] `docker stats` streaming → CPU/mem columns for containers
- [ ] Collector failure isolation + degraded badges

DoD: mixed proc+container stack fully manageable from one pane without touching `docker ps`.

## Week 3 — Logs + health (v0.3)

- [ ] Log tail pane: `docker logs -f` for containers, file-follow for procs writing logs
- [ ] Optional HTTP health checks (opt-in) → status dots ●●○ with latency
- [ ] CPU/mem sparklines per row
- [ ] macOS `lsof` fallback hardened + integration-tested

DoD: debug a failing service (spot red dot → open logs → restart) without leaving Portly.

## Week 4 — Polish (v0.4)

- [ ] Config file `~/.config/portly.toml` (overrides-only; zero-config still works)
- [ ] Themes + truecolor detection
- [ ] Keybinding help overlay finalized; man page; shell completions
- [ ] insta full-frame golden suite; fuzz update() against random Msg sequences

DoD: nothing embarrassing in `--help`, docs match behavior, frames pinned by tests.

## Week 5 — Ship it publicly (v1.0)

Distribution:
- [ ] Homebrew tap (`brew install portly-tap/portly/portly`)
- [ ] cargo-binstall metadata in repo
- [ ] curl install script + scoop manifest + AUR package
- [ ] **VHS demo GIF at top of README** (non-negotiable for TUI tools)

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
