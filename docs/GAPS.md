# Gap Analysis — Portly vs. mature 2026 products

Researched 2026-08-22 against current releases of the tools users actually compare Portly to:
lazydocker (52.6k★), k9s (34.4k★), btop++ (34.1k★), process-compose (2.7k★), and three
2025–2026 entrants in the exact niche: **Homedash** (Go/BubbleTea homelab dashboard),
**statui** (Rust/ratatui HTTP health TUI), **corsa** (Rust dev-process dashboard with MCP),
**lerd** (Laravel ops TUI). Each gap below is mapped to what we shipped to close it.

| # | Gap observed in mature products | Evidence | Portly closure |
|---|---|---|---|
| 1 | Container lifecycle actions (start/stop/restart) from the same table | Homedash `s/S/R` keys; lazydocker core feature | `x`/`T`/`G` arm contextual actions; Enter confirms (src/docker.rs, model.rs) |
| 2 | Log pane with follow mode | Homedash "log follow mode on entry"; lerd logs tab; corsa real-time logs | `l` opens bottom log pane: docker logs stream for containers, file tailer for procs via `[log_files]` (src/logs.rs) |
| 3 | Health checks with state dots + latency | statui (latency sparklines), Homedash `health:unhealthy`, corsa healthCheck.url | opt-in prober: ● up / ◐ degraded / ✗ down + ms (src/health.rs) |
| 4 | Per-row CPU trend visualization | Homedash CPU/RAM sparklines; statui visual sparklines | TREND column, ▁▂▃▅▇ glyphs over ring buffer (view.rs) |
| 5 | TOML config + themes, zero-config default preserved | Homedash YAML w/ unknown-field rejection; statui layered TOML; corsa themes | `portly.toml`: interval, ignore_ports, labels, log_files, [docker], [health]; themes dark/light/nord (config.rs) |
| 6 | Mouse click + wheel navigation | Homedash "click and scroll navigation"; lerd wheel scrolls cards | crossterm mouse capture: click selects the row under the cursor (scroll-offset aware), wheel moves the selection and the viewport scrolls to keep it visible |
| 7 | Data staleness indicator when refresh falls behind | Homedash "freshness indicators… mark stale snapshots" | title shows data age; ">2×interval" marks STALE (view.rs, passed clock keeps render pure) |
| 8 | Headless / non-TTY behavior for scripting & CI | lerd exits non-zero when stdout isn't a TTY; Homedash --test-mode | `portly --once` prints snapshot table and exits; doubles as bench harness input |
| 9 | Field-aware filtering (`state:running` style) | Homedash filter tokens | v0.1 substring filter covers port/proto/pid/name/container; field tokens deferred to v0.2 (documented honestly here, not hidden) |
| 10 | Distribution kit parity (brew/scoop/binstall/GIF/install script) | every mature tool ships all of these | install.sh/.ps1, Formula/portly.rb (real sha256 of tag tarball), scoop manifest, demo.tape for VHS, release CI matrix |

Deliberately **not** copied (scope discipline): weather panels, compose-stack grouping,
MCP/AI integration, in-TUI installers. Portly's thesis is discovery of *what is already
running*; those features belong to different products.
