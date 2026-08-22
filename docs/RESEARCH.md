# Portly — Research & Competitive Analysis

Researched 2026-08-22. Star counts pulled live from the GitHub API that day; treat as point-in-time.

## 1. The gap

The daily question *"what's running on :3000/:5432/:8080?"* is answered today by stitching together `lsof`/`netstat`, `docker ps`, `htop`, and terminal tabs. **No established tool presents ports → owners → health → actions as one live view.**

Adjacent incumbents each hold one slice of the job:

### Container-focused (missing host processes/ports)
| Tool | ★ | Notes |
|---|---|---|
| lazydocker | 52.6k | The UX archetype for "lazy" TUIs; containers/compose only |
| k9s | 34.4k | Same interaction model for K8s; proves demand for "navigator" TUIs |
| ctop | 17.8k | Top-like container metrics; effectively unmaintained since ~2024 |
| dozzle | 14.2k | Real-time container logs but web UI, not TUI |
| Portainer | 38.3k | Full web GUI; heavyweight, not keyboard-first |
| dry / dockly | ~3k each (approx.) | Smaller Docker TUIs |

### System monitors (no service semantics)
| Tool | ★ | Notes |
|---|---|---|
| btop++ | 34.1k | Resources + signals, no port↔service mapping |
| sniffnet | 40.6k | Traffic monitor (GUI/iced); validates Rust network-tooling appeal |
| bandwhich | 11.9k | Per-process bandwidth TUI; closest Rust sibling in spirit |
| bottom (`btm`) | 13.9k | Cross-platform Rust monitor |
| nethogs | 3.7k | Linux per-proc bandwidth |

### Demand signals for the exact workflow
- `jkfran/killport` (**1.8k★**) — CLI-only, kills by port incl. containers; heavy demand for even a non-visual version.
- npm `kill-port`, `fkill`, GitHub topic `kill-port` — recurring EADDRINUSE pain across ecosystems.
- `process-compose` (**2.7k★**) — YAML-defined process supervisor *with* a real TUI, health checks, restarts. Closest cousin on the management side, but it only manages what *you declared*, never discovers what's already running.

### Direct micro-competitors (all < 6 months old, tiny traction)
| Project | ★ | Created | Read |
|---|---|---|---|
| subh05sus/porthole | 1 | Aug 2026 | Go/bubbletea "port kill switch + service viewer" |
| y-tretyakov/portarium | 4 | Jul 2026 | Rust/Tauri developer port manager |
| enrell/port | 2 | Apr 2026 | Linux TUI open-ports + kill |
| lsport | ? | 2026 | Rust crate, identify-and-kill by port |
| portrm (portrm.dev) | ? | 2026 | Rust CLI for EADDRINUSE conflicts |

**Interpretation:** the idea is "in the air" — multiple independent attempts within months — but nobody has shipped a polished, cross-platform, well-distributed implementation. That's both validation and a warning: window is open now, and distribution quality (GIF, install matrix, issue responsiveness) will decide who wins the niche, not feature count.

## 2. Technical feasibility (verified against crate sources/docs)

### Socket → PID
- **Linux:** `procfs::net::tcp()/tcp6()/udp()/udp6()` expose entries with `inode`; the crate's own netstat example builds `HashMap<inode, Stat>` by scanning `all_processes()` → `fd()` → `FDTarget::Socket(inode)`. Works, but the join is ours and is O(processes × fds). Fine at dev-machine scale (~10⁴ fds).
- **Portable alternative (chosen as base):** `netstat2::get_sockets_info()` returns sockets with `associated_pids` using low-level OS APIs (netlink sock-diag on Linux, `GetExtendedTcp/UdpTable` on Windows, libinfo/sysctl paths on macOS). One API, three platforms — de-risks Windows support which most competitors ignore entirely.
- **macOS caveat:** the `libproc` crate covers pid→path/name/rusage but does **not** expose socket tables. Standard practice (what killport et al. do): parse `lsof -iTCP -sTCP:LISTEN -P -n`. Low-level sysctl PCB enumeration exists but is fiddly and under-maintained — treat `lsof` parse as the supported path, revisit later.

### Process stats
- `sysinfo::Process::cpu_usage() -> f32` (%), `memory()` bytes, `kill()` cross-platform, `kill_with(Signal)` unix-only.
- Documented constraint: accurate CPU requires two refreshes ≥ `MINIMUM_CPU_UPDATE_INTERVAL` (~200 ms) apart. First tick renders blanks, not zeros — honest UI beats fake numbers.

### Docker
- `bollard` confirmed to cover the full needed surface: `stats(name, StatsOptions{stream})` → `Stream<ContainerStatsResponse>`; streaming `logs()`; `restart_container`, `kill_container(signal)`, `stop_container`, `inspect_container`, `top_processes`, `wait_container`.
- Permissions: needs Docker socket (unix) or named pipe (Windows); document requirement, never auto-elevate.

### Health probes (week 3)
- Plain `reqwest`/`hyper` GET with short timeout per enabled service; status dots derived from HTTP code + latency. Optional feature, off by default so scanning never surprises users with outbound traffic.

## 3. Positioning statement

> Portly is **k9s for localhost**: a navigator for the services already running on your machine — discovered automatically, not declared in YAML. It complements (not replaces) foreman/process-compose; "project mode" (stretch) later adopts their declare-then-launch workflow once discovery credibility is established.

## 4. Risks & mitigations

| Risk | Mitigation |
|---|---|
| Micro-competitor ships polish first | Week-1 crates.io publish; VHS GIF; respond to issues < 24 h for 2 weeks |
| Heuristics mislabel services | Always show raw evidence columns; labels are additive, never replace PIDs |
| Docker socket perms confuse users | Detect and degrade gracefully: docker column shows hint, rest keeps working |
| Platform divergence rots | `SocketSource` trait + per-platform tests in CI matrix (linux/mac/windows) |
| Scope creep toward full monitor | Hard line: Portly is about *services on ports*, not general system stats |

## 5. README conventions of winning TUI tools (from lazygit/lazydocker/btop)

1. Hero GIF near the top (VHS-generated) — non-negotiable.
2. Badges row (CI, crates/homebrew version, packaging).
3. Narrative "elevator pitch" pain story before features.
4. Wide install matrix (brew tap, binstall, scoop/choco, AUR, cargo install).
5. Keybindings section or dedicated docs page.
6. Requirements/prereqs (Docker API floor, terminal capabilities).
7. FAQ + explicit alternatives section naming competitors.
