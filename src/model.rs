use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Protocol {
    Tcp,
    Udp,
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tcp => write!(f, "tcp"),
            Self::Udp => write!(f, "udp"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Source {
    Proc,
    Kernel,
    Docker,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Proc => write!(f, "proc"),
            Self::Kernel => write!(f, "kernel"),
            Self::Docker => write!(f, "docker"),
        }
    }
}

/// Which collector produced a batch; used to replace one pool of rows without
/// touching the others (a slow Docker daemon must never blank host rows).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pool {
    Host,
    Docker,
}

#[derive(Clone, Debug)]
pub struct PortEntry {
    pub port: u16,
    pub proto: Protocol,
    pub pid: Option<u32>,
    pub process: Option<String>,
    pub cmdline: Option<String>,
    pub cpu: Option<f32>,
    pub mem_bytes: Option<u64>,
    pub source: Source,
    /// (id, name) when the row is backed by a container.
    pub container: Option<(String, String)>,
    /// running / exited / paused … as reported by the runtime.
    pub container_state: Option<String>,
}

impl PortEntry {
    pub fn key(&self) -> String {
        match &self.container {
            Some((id, _)) => format!("c:{id}:{}", self.port),
            None => format!("p:{}:{}:{}", self.pid.unwrap_or(0), self.port, self.proto),
        }
    }

    pub fn display_name(&self) -> String {
        if let Some((_, name)) = &self.container {
            return name.clone();
        }
        match (&self.process, &self.cmdline) {
            (Some(p), _) => p.clone(),
            (None, Some(c)) => c.clone(),
            (None, None) => "(unknown)".to_string(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortKey {
    Port,
    Cpu,
    Mem,
}

impl SortKey {
    pub fn next(self) -> Self {
        match self {
            Self::Port => Self::Cpu,
            Self::Cpu => Self::Mem,
            Self::Mem => Self::Port,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "port" => Some(Self::Port),
            "cpu" => Some(Self::Cpu),
            "mem" | "memory" => Some(Self::Mem),
            _ => None,
        }
    }
}

impl fmt::Display for SortKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Port => write!(f, "port"),
            Self::Cpu => write!(f, "cpu"),
            Self::Mem => write!(f, "mem"),
        }
    }
}

// ------------------------------------------------------------- health ------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Health {
    Unknown,
    Up,
    Degraded,
    Down,
}

impl Health {
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Unknown => "·",
            Self::Up => "\u{25cf}",       // ●
            Self::Degraded => "\u{25d0}", // ◐
            Self::Down => "\u{2717}",     // ✗
        }
    }

    pub fn classify(status: u16) -> Self {
        match status {
            200..=399 => Self::Up,
            400..=599 => Self::Degraded,
            _ => Self::Down,
        }
    }
}

// ------------------------------------------------------------ messages -----

#[derive(Clone, Debug)]
pub enum Key {
    Up,
    Down,
    Enter,
    Esc,
    Backspace,
    PageUp,
    PageDown,
    Char(char),
    ForceQuit,
}

#[derive(Clone, Copy, Debug)]
pub enum MouseMsg {
    Click(u16),
    ScrollUp,
    ScrollDown,
}

#[cfg_attr(not(feature = "docker"), allow(dead_code))]
#[derive(Clone, Debug)]
pub enum Msg {
    Key(Key),
    Mouse(MouseMsg),
    ScanResult(Vec<PortEntry>),
    Containers(Vec<PortEntry>),
    ContainerStats {
        id: String,
        cpu: f32,
        mem_bytes: u64,
    },
    HealthUpdate(HashMap<u16, (Health, Option<u16>)>),
    CollectorFailed(String),
    LogLine {
        gen: u64,
        line: String,
    },
    FrameGeometry(u16, u16, u16), // x, y, width of the table body area
    ActionDone(Result<String, String>),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ContainerAction {
    Start,
    Stop,
    Restart,
}

impl fmt::Display for ContainerAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start => write!(f, "start"),
            Self::Stop => write!(f, "stop"),
            Self::Restart => write!(f, "restart"),
        }
    }
}

#[derive(Clone, Debug)]
pub enum LogTarget {
    Container { id: String, name: String },
    File { port: u16, path: std::path::PathBuf },
}

#[derive(Clone, Debug)]
pub struct ArmedAction {
    pub label: String,
    pub effect: Effect,
}

#[derive(Clone, Debug)]
pub enum Effect {
    Kill { pid: u32, port: u16 },
    ContainerAction { id: String, action: ContainerAction },
    OpenLogs(LogTarget),
    CloseLogs(u64),
    Quit,
}

pub const TREND_LEN: usize = 14;

pub struct Model {
    pub entries: Vec<PortEntry>,
    pub selected: usize,
    pub sort: SortKey,
    pub filter: Option<String>,
    pub filter_input: bool,
    pub pending_action: Option<ArmedAction>,
    pub show_help: bool,
    pub status: String,
    pub last_error: Option<String>,
    pub last_scan: Option<Instant>,
    pub interval: Duration,
    pub health_enabled: bool,
    pub health: HashMap<u16, (Health, Option<u16>)>,
    pub trend: HashMap<String, VecDeque<f32>>,
    pub log_target_title: Option<String>,
    pub log_gen: u64,
    pub log_lines: VecDeque<String>,
    pub log_scroll: usize,
    pub table_area: (u16, u16, u16),
    pub mouse: bool,
    /// port → file mapping from `[log_files]` config, used by the logs pane.
    pub cfg_log_files: HashMap<u16, std::path::PathBuf>,
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}

impl Model {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            selected: 0,
            sort: SortKey::Port,
            filter: None,
            filter_input: false,
            pending_action: None,
            show_help: false,
            status: String::new(),
            last_error: None,
            last_scan: None,
            interval: Duration::from_millis(500),
            health_enabled: false,
            health: HashMap::new(),
            trend: HashMap::new(),
            log_target_title: None,
            log_gen: 0,
            log_lines: VecDeque::new(),
            log_scroll: 0,
            table_area: (0, 0, 80),
            mouse: true,
            cfg_log_files: HashMap::new(),
        }
    }

    pub fn open_logs_for(&mut self, target: &LogTarget) -> Option<Effect> {
        let title = match target {
            LogTarget::Container { name, .. } => format!("container {name}"),
            LogTarget::File { port, path } => format!(":{port} {}", path.display()),
        };
        self.log_gen += 1;
        self.log_lines.clear();
        self.log_scroll = 0;
        self.log_target_title = Some(title);
        Some(Effect::OpenLogs(match target {
            LogTarget::Container { id, name } => LogTarget::Container {
                id: id.clone(),
                name: name.clone(),
            },
            LogTarget::File { port, path } => LogTarget::File {
                port: *port,
                path: path.clone(),
            },
        }))
    }

    pub fn close_logs(&mut self) -> Option<Effect> {
        self.log_target_title.as_ref()?;
        self.log_target_title = None;
        self.log_gen += 1;
        self.log_lines.clear();
        self.log_scroll = 0;
        Some(Effect::CloseLogs(self.log_gen))
    }

    /// Indices into `entries` that survive the current filter, ordered by the
    /// current sort key. The single source of truth for what is on screen.
    pub fn visible(&self) -> Vec<usize> {
        let needle = self.filter.as_ref().map(|f| f.to_lowercase());
        let mut idx: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| match &needle {
                None => true,
                Some(n) if n.is_empty() => true,
                Some(n) => {
                    let hay = format!(
                        "{} {} {} {} {}",
                        e.port,
                        e.proto,
                        e.pid.map(|p| p.to_string()).unwrap_or_default(),
                        e.display_name(),
                        e.container_state.clone().unwrap_or_default(),
                    );
                    hay.to_lowercase().contains(n)
                }
            })
            .map(|(i, _)| i)
            .collect();
        idx.sort_by(|&a, &b| {
            let (x, y) = (&self.entries[a], &self.entries[b]);
            match self.sort {
                SortKey::Port => x.port.cmp(&y.port).then(x.proto.cmp(&y.proto)),
                SortKey::Cpu => y
                    .cpu
                    .partial_cmp(&x.cpu)
                    .unwrap_or(std::cmp::Ordering::Equal),
                SortKey::Mem => y.mem_bytes.cmp(&x.mem_bytes),
            }
        });
        idx
    }

    pub fn selected_entry(&self) -> Option<&PortEntry> {
        self.visible().get(self.selected).map(|&i| &self.entries[i])
    }

    /// Data age in whole seconds relative to `now` (injected so rendering
    /// stays deterministic in tests).
    pub fn data_age_secs(&self, now: Instant) -> Option<u64> {
        self.last_scan.map(|t| now.duration_since(t).as_secs())
    }
}

/// Elm-style update: pure transition from `(Model, Msg)` to mutated `Model`
/// plus side effects executed outside state by the runtime.
pub fn update(model: &mut Model, msg: Msg) -> Vec<Effect> {
    let mut effects = Vec::new();
    match msg {
        Msg::ScanResult(new_entries) => replace_pool(model, Pool::Host, new_entries),
        Msg::Containers(new_entries) => replace_pool(model, Pool::Docker, new_entries),
        Msg::ContainerStats { id, cpu, mem_bytes } => {
            apply_container_stats(model, &id, cpu, mem_bytes)
        }
        Msg::HealthUpdate(map) => model.health = map,
        Msg::CollectorFailed(err) => model.last_error = Some(err),
        Msg::LogLine { gen, line } => {
            if gen == model.log_gen && model.log_target_title.is_some() {
                push_log_line(model, line);
            }
        }
        Msg::FrameGeometry(x, y, w) => model.table_area = (x, y, w),
        Msg::Mouse(m) => effects.extend(handle_mouse(model, m)),
        Msg::Key(k) => effects.extend(handle_key(model, k)),
        Msg::ActionDone(result) => match result {
            Ok(desc) => {
                model.status = desc;
                model.last_error = None;
            }
            Err(err) => model.last_error = Some(err),
        },
    }
    effects
}

fn handle_mouse(model: &mut Model, m: MouseMsg) -> Vec<Effect> {
    match m {
        MouseMsg::ScrollUp => handle_key(model, Key::Up),
        MouseMsg::ScrollDown => handle_key(model, Key::Down),
        MouseMsg::Click(y) => {
            let (_ax, ay, _aw) = model.table_area;
            let vis_len = model.visible().len();
            // Rows start two lines below the panel top (border + header).
            if y >= ay + 2 {
                let row = (y - ay - 2) as usize;
                if row < vis_len {
                    model.selected = row;
                }
            }
            Vec::new()
        }
    }
}

fn apply_container_stats(m: &mut Model, id: &str, cpu: f32, mem_bytes: u64) {
    // A multi-port container owns several rows; every one of them gets the
    // sample and its own trend entry.
    let mut updated_keys: Vec<String> = Vec::new();
    for e in m.entries.iter_mut() {
        if let Some((cid, _)) = &e.container {
            if cid == id {
                e.cpu = Some(cpu);
                e.mem_bytes = Some(mem_bytes);
                updated_keys.push(e.key());
            }
        }
    }
    for key in updated_keys {
        let buf = m.trend.entry(key).or_default();
        buf.push_back(cpu);
        while buf.len() > TREND_LEN {
            buf.pop_front();
        }
    }
}

fn replace_pool(model: &mut Model, pool: Pool, new_entries: Vec<PortEntry>) {
    let sel_key: Option<(Option<u32>, u16, Protocol)> =
        model.visible().get(model.selected).map(|&i| {
            let e = &model.entries[i];
            (e.pid, e.port, e.proto)
        });

    let is_docker = matches!(pool, Pool::Docker);
    // Drop rows owned by this pool (they're being replaced), keep the other
    // pool's rows: replacing host rows must never wipe container rows.
    // Truth table: P=Host keeps source==Docker; P=Docker keeps source!=Docker.
    model
        .entries
        .retain(|e| (e.source == Source::Docker) != is_docker);
    model.entries.extend(new_entries);

    if pool == Pool::Host {
        model.last_scan = Some(Instant::now());
    }
    // Append the latest CPU sample to each row of this pool's trend buffer.
    let samples: Vec<(String, f32)> = model
        .entries
        .iter()
        .filter(|e| (e.source == Source::Docker) == is_docker && e.cpu.is_some())
        .map(|e| (e.key(), e.cpu.unwrap_or(0.0)))
        .collect();
    for (key, cpu) in samples {
        let buf = model.trend.entry(key).or_default();
        buf.push_back(cpu);
        while buf.len() > TREND_LEN {
            buf.pop_front();
        }
    }

    let vis = model.visible();
    let matched = sel_key.and_then(|k| {
        vis.iter().position(|&i| {
            let e = &model.entries[i];
            (e.pid, e.port, e.proto) == k
        })
    });
    model.selected = match matched {
        Some(pos) => pos,
        None => model.selected.min(vis.len().saturating_sub(1)),
    };
}

/// Keep the selection cursor inside the visible list after the visible set
/// shrinks (filter typed/backspaced). Without this the highlight vanishes
/// and `x` silently stops working.
fn clamp_selection(m: &mut Model) {
    let len = m.visible().len();
    if m.selected >= len {
        m.selected = len.saturating_sub(1);
    }
}

fn push_log_line(m: &mut Model, line: String) {
    if m.log_lines.len() >= 500 {
        m.log_lines.pop_front();
    }
    m.log_lines.push_back(line);
    if m.log_scroll > 0 {
        // Keep the same viewport anchored while history streams past.
        m.log_scroll = (m.log_scroll + 1).min(m.log_lines.len());
    }
}

fn handle_key(model: &mut Model, key: Key) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Key::ForceQuit = key {
        return vec![Effect::Quit];
    }
    if model.filter_input {
        match key {
            Key::Esc => {
                model.filter_input = false;
                model.filter = None;
            }
            Key::Enter | Key::Up | Key::Down => model.filter_input = false,
            Key::Backspace => {
                if let Some(f) = model.filter.as_mut() {
                    f.pop();
                    if f.is_empty() {
                        model.filter = None;
                    }
                }
                clamp_selection(model);
            }
            Key::Char(c) => {
                model.filter.get_or_insert_with(String::new).push(c);
                clamp_selection(model);
            }
            _ => {}
        }
        return effects;
    }
    if model.show_help {
        match key {
            Key::Esc | Key::Enter | Key::Char('?') | Key::Char('q') => model.show_help = false,
            _ => {}
        }
        return effects;
    }
    if model.log_target_title.is_some() {
        match key {
            Key::Esc | Key::Char('l') | Key::Char('q') => {
                if let Some(fx) = model.close_logs() {
                    effects.push(fx);
                }
            }
            Key::PageUp => {
                model.log_scroll = (model.log_scroll + 10).min(model.log_lines.len());
            }
            Key::PageDown => {
                model.log_scroll = model.log_scroll.saturating_sub(10);
            }
            Key::Up => {
                model.log_scroll = (model.log_scroll + 1).min(model.log_lines.len());
            }
            Key::Down => {
                model.log_scroll = model.log_scroll.saturating_sub(1);
            }
            _ => {}
        }
        return effects;
    }
    match key {
        Key::Char('q') => effects.push(Effect::Quit),
        Key::Char('?') => model.show_help = true,
        Key::Char('/') => model.filter_input = true,
        Key::Char('s') => model.sort = model.sort.next(),
        Key::Up => model.selected = model.selected.saturating_sub(1),
        Key::Down => {
            let len = model.visible().len();
            model.selected = (model.selected + 1).min(len.saturating_sub(1));
        }
        Key::Char('l') => {
            if let Some(entry) = model.selected_entry().cloned() {
                let target = log_target_for(model, &entry);
                match target {
                    Some(t) => {
                        if let Some(fx) = model.open_logs_for(&t) {
                            effects.push(fx);
                        }
                    }
                    None => {
                        model.status =
                            "no logs available: containers stream docker logs; processes need [log_files] config"
                                .to_string();
                    }
                }
            }
        }
        Key::Char('x') => arm_contextual(model),
        Key::Char('R') => arm_container(model, ContainerAction::Restart),
        Key::Char('T') => arm_container(model, ContainerAction::Stop),
        Key::Char('G') => arm_container(model, ContainerAction::Start),
        Key::Enter => {
            if let Some(armed) = model.pending_action.take() {
                match armed.effect {
                    Effect::Kill { pid, port } => {
                        model.status = format!("killing pid {pid} on :{port}...");
                        effects.push(Effect::Kill { pid, port });
                    }
                    Effect::ContainerAction { id, action } => {
                        model.status = format!("{action} container {id}...");
                        effects.push(Effect::ContainerAction { id, action });
                    }
                    _ => {}
                }
            }
        }
        Key::Esc => {
            model.pending_action = None;
            model.filter = None;
            model.last_error = None;
            model.status.clear();
        }
        _ => {}
    }
    effects
}

fn log_target_for(m: &Model, e: &PortEntry) -> Option<LogTarget> {
    if let Some((id, name)) = &e.container {
        return Some(LogTarget::Container {
            id: id.clone(),
            name: name.clone(),
        });
    }
    m.cfg_log_files.get(&e.port).map(|path| LogTarget::File {
        port: e.port,
        path: path.clone(),
    })
}

fn arm_contextual(model: &mut Model) {
    let vis = model.visible();
    let Some(&i) = vis.get(model.selected) else {
        return;
    };
    let entry = &model.entries[i];
    if let Some((id, name)) = &entry.container {
        let action = if entry.container_state.as_deref() == Some("running") {
            ContainerAction::Restart
        } else {
            ContainerAction::Start
        };
        model.pending_action = Some(ArmedAction {
            label: format!("{action} container {name}?"),
            effect: Effect::ContainerAction {
                id: id.clone(),
                action,
            },
        });
        model.status = format!("{} {name}? [enter] confirm · [esc] cancel", action);
        return;
    }
    match entry.pid {
        Some(pid) => {
            model.pending_action = Some(ArmedAction {
                label: format!("kill pid {pid} on :{}?", entry.port),
                effect: Effect::Kill {
                    pid,
                    port: entry.port,
                },
            });
            model.status = format!(
                "kill pid {pid} on :{}? [enter] confirm · [esc] cancel",
                entry.port
            );
        }
        None => {
            model.status = "no owning process visible for this socket".to_string();
        }
    }
}

fn arm_container(model: &mut Model, action: ContainerAction) {
    let vis = model.visible();
    let Some(&i) = vis.get(model.selected) else {
        return;
    };
    let entry = &model.entries[i];
    let Some((id, name)) = &entry.container else {
        model.status = format!("{action} applies to containers only");
        return;
    };
    if action != ContainerAction::Start && entry.container_state.as_deref() != Some("running") {
        model.status = format!("container {name} is not running");
        return;
    }
    model.pending_action = Some(ArmedAction {
        label: format!("{action} container {name}?"),
        effect: Effect::ContainerAction {
            id: id.clone(),
            action,
        },
    });
    model.status = format!("{action} {name}? [enter] confirm · [esc] cancel");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(port: u16, pid: Option<u32>, name: &str) -> PortEntry {
        PortEntry {
            port,
            proto: Protocol::Tcp,
            pid,
            process: Some(name.to_string()),
            cmdline: None,
            cpu: Some(1.0),
            mem_bytes: Some(1000),
            source: Source::Proc,
            container: None,
            container_state: None,
        }
    }

    fn container(port: u16, id: &str, name: &str, state: &str) -> PortEntry {
        PortEntry {
            port,
            proto: Protocol::Tcp,
            pid: None,
            process: None,
            cmdline: None,
            cpu: Some(2.0),
            mem_bytes: Some(4096),
            source: Source::Docker,
            container: Some((id.to_string(), name.to_string())),
            container_state: Some(state.to_string()),
        }
    }

    #[test]
    fn filter_matches_port_process_pid_and_state() {
        let mut m = Model::new();
        m.entries = vec![host(3000, Some(1), "node"), host(5432, Some(2), "postgres")];
        m.filter = Some("5432".into());
        assert_eq!(m.visible(), vec![1]);
        m.filter = Some("NODE".into());
        assert_eq!(m.visible(), vec![0]);
        m.filter = Some("running".into());
        assert!(m.visible().is_empty());
    }

    #[test]
    fn pools_replace_independently() {
        let mut m = Model::new();
        update(&mut m, Msg::ScanResult(vec![host(3000, Some(1), "node")]));
        update(
            &mut m,
            Msg::Containers(vec![container(5432, "abc", "db", "running")]),
        );
        assert_eq!(m.entries.len(), 2);

        // A docker rescan must not wipe host rows and vice versa.
        update(
            &mut m,
            Msg::Containers(vec![container(6379, "def", "cache", "exited")]),
        );
        let ports: Vec<u16> = m.visible().iter().map(|&i| m.entries[i].port).collect();
        assert_eq!(ports, vec![3000, 6379]);

        // A host rescan fully replaces the host pool: :3000 is gone because
        // the new snapshot no longer reports it; container rows survive.
        update(&mut m, Msg::ScanResult(vec![host(8080, Some(9), "nginx")]));
        let ports: Vec<u16> = m.visible().iter().map(|&i| m.entries[i].port).collect();
        assert_eq!(ports, vec![6379, 8080]);
    }

    #[test]
    fn sort_cpu_descends_and_blanks_last() {
        let mut m = Model::new();
        let mut a = host(1, Some(1), "a");
        a.cpu = Some(9.0);
        let mut b = host(2, Some(2), "b");
        b.cpu = Some(0.5);
        let mut c = host(3, Some(3), "c");
        c.cpu = None;
        m.entries = vec![b, c, a];
        m.sort = SortKey::Cpu;
        let vis: Vec<u16> = m.visible().iter().map(|&i| m.entries[i].port).collect();
        assert_eq!(vis, vec![1, 2, 3]);
    }

    #[test]
    fn selection_survives_rescan_by_identity_across_pools() {
        let mut m = Model::new();
        update(
            &mut m,
            Msg::ScanResult(vec![host(3000, Some(1), "node"), host(5432, Some(2), "pg")]),
        );
        m.selected = 1;
        update(
            &mut m,
            Msg::Containers(vec![container(8080, "x", "web", "running")]),
        );
        // pg still selected even though a new pool appeared above/below.
        assert_eq!(m.selected_entry().unwrap().process.as_deref(), Some("pg"));
    }

    #[test]
    fn contextual_arm_proc_kill_vs_container_restart() {
        let mut m = Model::new();
        update(&mut m, Msg::ScanResult(vec![host(3000, Some(42), "node")]));
        update(
            &mut m,
            Msg::Containers(vec![
                container(8080, "id1", "api", "running"),
                container(9090, "id2", "worker", "exited"),
            ]),
        );
        // select proc row
        m.sort = SortKey::Port;
        let vis = m.visible();
        m.selected = vis
            .iter()
            .position(|&i| m.entries[i].pid == Some(42))
            .unwrap();
        let _ = update(&mut m, Msg::Key(Key::Char('x')));
        let armed = m.pending_action.as_ref().unwrap();
        assert!(matches!(armed.effect, Effect::Kill { pid: 42, .. }));

        // select running container -> restart; exited -> start
        let idx_running = m
            .visible()
            .iter()
            .position(|&i| m.entries[i].port == 8080)
            .unwrap();
        m.selected = idx_running;
        let _ = update(&mut m, Msg::Key(Key::Char('x')));
        assert!(matches!(
            m.pending_action.as_ref().unwrap().effect,
            Effect::ContainerAction {
                action: ContainerAction::Restart,
                ..
            }
        ));
        let fx = update(&mut m, Msg::Key(Key::Enter));
        assert!(matches!(
            fx.first(),
            Some(Effect::ContainerAction {
                action: ContainerAction::Restart,
                ..
            })
        ));

        let idx_exited = m
            .visible()
            .iter()
            .position(|&i| m.entries[i].port == 9090)
            .unwrap();
        m.selected = idx_exited;
        let _ = update(&mut m, Msg::Key(Key::Char('x')));
        assert!(matches!(
            m.pending_action.as_ref().unwrap().effect,
            Effect::ContainerAction {
                action: ContainerAction::Start,
                ..
            }
        ));
    }

    #[test]
    fn explicit_container_keys_guard_state() {
        let mut m = Model::new();
        update(
            &mut m,
            Msg::Containers(vec![container(8080, "id1", "api", "exited")]),
        );
        let _ = update(&mut m, Msg::Key(Key::Char('T')));
        assert!(m.pending_action.is_none(), "stop on exited must be refused");
        let _ = update(&mut m, Msg::Key(Key::Char('G')));
        assert!(matches!(
            m.pending_action.as_ref().unwrap().effect,
            Effect::ContainerAction {
                action: ContainerAction::Start,
                ..
            }
        ));
    }

    #[test]
    fn kill_is_two_step_and_esc_cancels() {
        let mut m = Model::new();
        m.entries = vec![host(3000, Some(7), "node")];
        let fx = update(&mut m, Msg::Key(Key::Enter));
        assert!(fx.is_empty());
        let _ = update(&mut m, Msg::Key(Key::Char('x')));
        let fx = update(&mut m, Msg::Key(Key::Up));
        let _ = fx;
        let _ = update(&mut m, Msg::Key(Key::Esc));
        assert!(m.pending_action.is_none());
        let fx = update(&mut m, Msg::Key(Key::Enter));
        assert!(fx.is_empty());
        let _ = update(&mut m, Msg::Key(Key::Char('x')));
        let fx = update(&mut m, Msg::Key(Key::Enter));
        assert!(matches!(fx.first(), Some(Effect::Kill { pid: 7, .. })));
    }

    #[test]
    fn force_quit_wins_over_every_mode() {
        for setup in [
            |m: &mut Model| m.filter_input = true,
            |m: &mut Model| m.show_help = true,
            |m: &mut Model| {
                m.log_target_title = Some("t".into());
                m.log_gen = 3;
            },
            |_m: &mut Model| {},
        ] {
            let mut m = Model::new();
            setup(&mut m);
            let fx = update(&mut m, Msg::Key(Key::ForceQuit));
            assert!(matches!(fx.first(), Some(Effect::Quit)));
        }
    }

    #[test]
    fn log_generations_gate_stale_lines() {
        let mut m = Model::new();
        m.entries = vec![host(3000, Some(1), "node")];
        let fx = update(&mut m, Msg::Key(Key::Char('l'))); // no target yet -> hint only? has cfg none + proc => None
        assert!(fx.is_empty(), "proc without log_files config shows hint");

        m.cfg_log_files
            .insert(3000, std::path::PathBuf::from("/tmp/a.log"));
        let fx = update(&mut m, Msg::Key(Key::Char('l')));
        assert!(matches!(fx.first(), Some(Effect::OpenLogs(_))));
        let gen = m.log_gen;
        update(
            &mut m,
            Msg::LogLine {
                gen,
                line: "hello".into(),
            },
        );
        assert_eq!(m.log_lines.len(), 1);

        // close then stale line must be ignored
        let fx = update(&mut m, Msg::Key(Key::Char('l'))); // toggles closed
        assert!(matches!(fx.first(), Some(Effect::CloseLogs(_))));
        update(
            &mut m,
            Msg::LogLine {
                gen,
                line: "ghost".into(),
            },
        );
        assert!(m.log_lines.is_empty());
    }

    #[test]
    fn trend_ring_buffer_caps_length() {
        let mut m = Model::new();
        for i in 0..30u32 {
            let mut e = host(i as u16 % 4, Some(i), "n");
            e.cpu = Some(i as f32);
            replace_pool_pub(&mut m, vec![e]);
        }
        let buf = m.trend.values().next().unwrap();
        assert!(buf.len() <= TREND_LEN);
    }

    fn replace_pool_pub(m: &mut Model, entries: Vec<PortEntry>) {
        update(m, Msg::ScanResult(entries));
    }

    #[test]
    fn health_classify_boundaries() {
        assert_eq!(Health::classify(200), Health::Up);
        assert_eq!(Health::classify(302), Health::Up);
        assert_eq!(Health::classify(404), Health::Degraded);
        assert_eq!(Health::classify(500), Health::Degraded);
        assert_eq!(Health::classify(100), Health::Down);
    }

    #[test]
    fn mouse_click_selects_visible_row() {
        let mut m = Model::new();
        m.table_area = (0, 0, 80);
        m.entries = vec![
            host(1, Some(1), "a"),
            host(2, Some(2), "b"),
            host(3, Some(3), "c"),
        ];
        update(&mut m, Msg::Mouse(MouseMsg::Click(4))); // row index 2
        assert_eq!(m.selected, 2);
        update(&mut m, Msg::Mouse(MouseMsg::Click(0))); // header area: no-op
        assert_eq!(m.selected, 2);
        update(&mut m, Msg::Mouse(MouseMsg::ScrollUp));
        assert_eq!(m.selected, 1);
    }
}

#[cfg(test)]
mod audit_tests {
    use super::*;

    fn container_row(port: u16, id: &str) -> PortEntry {
        PortEntry {
            port,
            proto: Protocol::Tcp,
            pid: None,
            process: None,
            cmdline: None,
            cpu: None,
            mem_bytes: None,
            source: Source::Docker,
            container: Some((id.to_string(), "web".to_string())),
            container_state: Some("running".to_string()),
        }
    }

    #[test]
    fn stats_update_every_row_and_trend_of_multiport_container() {
        let mut m = Model::new();
        m.entries = vec![container_row(8080, "abc"), container_row(8443, "abc")];
        update(
            &mut m,
            Msg::ContainerStats {
                id: "abc".into(),
                cpu: 7.5,
                mem_bytes: 2048,
            },
        );
        for e in &m.entries {
            assert_eq!(e.cpu, Some(7.5));
            assert_eq!(e.mem_bytes, Some(2048));
            assert_eq!(m.trend.get(&e.key()).map(|b| b.len()), Some(1));
        }
    }
}

#[cfg(test)]
mod audit3_tests {
    use super::*;

    fn host(port: u16, pid: Option<u32>, name: &str) -> PortEntry {
        PortEntry {
            port,
            proto: Protocol::Tcp,
            pid,
            process: Some(name.to_string()),
            cmdline: None,
            cpu: Some(1.0),
            mem_bytes: Some(1000),
            source: Source::Proc,
            container: None,
            container_state: None,
        }
    }

    #[test]
    fn shrinking_filter_clamps_selection_into_view() {
        let mut m = Model::new();
        m.entries = vec![
            host(1, Some(1), "a"),
            host(2, Some(2), "b"),
            host(3, Some(3), "c"),
            host(4, Some(4), "d"),
            host(5, Some(5), "e"),
        ];
        m.selected = 4; // cursor on the last row
        let _ = update(&mut m, Msg::Key(Key::Char('/')));
        for ch in "b".chars() {
            let _ = update(&mut m, Msg::Key(Key::Char(ch)));
        }
        // visible is now just row "b" (index 1); cursor must clamp onto it.
        assert_eq!(m.visible().len(), 1);
        assert_eq!(m.selected, 0);
        assert!(m.selected_entry().is_some());
        // commit the filter (exit input mode), then arming must work again
        // instead of silently doing nothing.
        let _ = update(&mut m, Msg::Key(Key::Enter));
        assert!(!m.filter_input);
        let _ = update(&mut m, Msg::Key(Key::Char('x')));
        assert!(m.pending_action.is_some());
    }

    #[test]
    fn clearing_filter_keeps_cursor_in_bounds() {
        let mut m = Model::new();
        m.entries = vec![host(1, Some(1), "a"), host(2, Some(2), "b")];
        m.filter_input = true;
        let _ = update(&mut m, Msg::Key(Key::Char('z'))); // matches nothing
        assert_eq!(m.visible().len(), 0);
        assert_eq!(m.selected, 0); // saturating, never wraps
        let _ = update(&mut m, Msg::Key(Key::Esc)); // clears filter
        assert_eq!(m.visible().len(), 2);
        assert!(m.selected < m.visible().len());
    }
}
