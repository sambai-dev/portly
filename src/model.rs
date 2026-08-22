use std::fmt;

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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    Proc,
    Kernel,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Proc => write!(f, "proc"),
            Self::Kernel => write!(f, "kernel"),
        }
    }
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

#[derive(Clone, Debug)]
pub enum Key {
    Up,
    Down,
    Enter,
    Esc,
    Backspace,
    Char(char),
    ForceQuit,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Key(Key),
    ScanResult(Vec<PortEntry>),
    CollectorFailed(String),
}

pub enum Effect {
    Kill { pid: u32, port: u16 },
    Quit,
}

pub struct Model {
    pub entries: Vec<PortEntry>,
    pub selected: usize,
    pub sort: SortKey,
    pub filter: Option<String>,
    pub filter_input: bool,
    pub pending_kill: Option<(u32, u16)>,
    pub show_help: bool,
    pub status: String,
    pub last_error: Option<String>,
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
            pending_kill: None,
            show_help: false,
            status: String::new(),
            last_error: None,
        }
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
                        "{} {} {} {}",
                        e.port,
                        e.proto,
                        e.pid.map(|p| p.to_string()).unwrap_or_default(),
                        e.process.clone().unwrap_or_default(),
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
}

/// Elm-style update: pure transition from `(Model, Msg)` to mutated `Model`
/// plus a list of side effects the runtime executes outside the state.
pub fn update(model: &mut Model, msg: Msg) -> Vec<Effect> {
    let mut effects = Vec::new();
    match msg {
        Msg::ScanResult(new_entries) => replace_entries(model, new_entries),
        Msg::CollectorFailed(err) => model.last_error = Some(err),
        Msg::Key(k) => effects.extend(handle_key(model, k)),
    }
    effects
}

fn replace_entries(model: &mut Model, new_entries: Vec<PortEntry>) {
    // Keep the same logical row selected across rescans by identity
    // (pid, port, proto), falling back to the previous cursor position.
    let sel_key: Option<(Option<u32>, u16, Protocol)> =
        model.visible().get(model.selected).map(|&i| {
            let e = &model.entries[i];
            (e.pid, e.port, e.proto)
        });
    model.entries = new_entries;
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
            }
            Key::Char(c) => {
                model.filter.get_or_insert_with(String::new).push(c);
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
        Key::Char('x') => arm_kill(model),
        Key::Enter => {
            if let Some((pid, port)) = model.pending_kill.take() {
                model.status = format!("killing pid {pid} on :{port}...");
                effects.push(Effect::Kill { pid, port });
            }
        }
        Key::Esc => {
            model.pending_kill = None;
            model.filter = None;
            model.last_error = None;
            model.status.clear();
        }
        _ => {}
    }
    effects
}

fn arm_kill(model: &mut Model) {
    let vis = model.visible();
    if let Some(&i) = vis.get(model.selected) {
        let entry = &model.entries[i];
        match entry.pid {
            Some(pid) => {
                model.pending_kill = Some((pid, entry.port));
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(port: u16, pid: Option<u32>, name: &str) -> PortEntry {
        PortEntry {
            port,
            proto: Protocol::Tcp,
            pid,
            process: Some(name.to_string()),
            cmdline: None,
            cpu: Some(1.0),
            mem_bytes: Some(1000),
            source: Source::Proc,
        }
    }

    #[test]
    fn filter_matches_port_process_pid() {
        let mut m = Model::new();
        m.entries = vec![
            entry(3000, Some(1), "node"),
            entry(5432, Some(2), "postgres"),
        ];
        m.filter = Some("5432".into());
        assert_eq!(m.visible(), vec![1]);
        m.filter = Some("NODE".into());
        assert_eq!(m.visible(), vec![0]);
    }

    #[test]
    fn sort_cpu_descends_and_blanks_last() {
        let mut m = Model::new();
        let mut a = entry(1, Some(1), "a");
        a.cpu = Some(9.0);
        let mut b = entry(2, Some(2), "b");
        b.cpu = Some(0.5);
        let mut c = entry(3, Some(3), "c");
        c.cpu = None;
        m.entries = vec![b, c, a];
        m.sort = SortKey::Cpu;
        let vis: Vec<u16> = m.visible().iter().map(|&i| m.entries[i].port).collect();
        assert_eq!(vis, vec![1, 2, 3]);
    }

    #[test]
    fn selection_survives_rescan_by_identity() {
        let mut m = Model::new();
        m.entries = vec![
            entry(3000, Some(1), "node"),
            entry(5432, Some(2), "postgres"),
        ];
        m.selected = 1;
        let shuffled = vec![
            entry(8080, Some(3), "nginx"),
            entry(5432, Some(2), "postgres"),
        ];
        let fx = update(&mut m, Msg::ScanResult(shuffled));
        assert!(fx.is_empty());
        let vis: Vec<Option<u32>> = m.visible().iter().map(|&i| m.entries[i].pid).collect();
        assert_eq!(vis, vec![Some(2), Some(3)]);
        assert_eq!(m.selected, 0);
    }

    #[test]
    fn kill_is_two_step_and_produces_effect() {
        let mut m = Model::new();
        m.entries = vec![entry(3000, Some(42), "node")];
        let fx = update(&mut m, Msg::Key(Key::Enter));
        assert!(fx.is_empty(), "enter without arming must not kill");
        let _ = update(&mut m, Msg::Key(Key::Char('x')));
        assert!(m.pending_kill.is_some());
        let fx = update(&mut m, Msg::Key(Key::Enter));
        assert!(matches!(fx.first(), Some(Effect::Kill { pid: 42, .. })));
        assert!(m.pending_kill.is_none());
    }

    #[test]
    fn esc_cancels_armed_kill() {
        let mut m = Model::new();
        m.entries = vec![entry(3000, Some(7), "node")];
        let _ = update(&mut m, Msg::Key(Key::Char('x')));
        let fx = update(&mut m, Msg::Key(Key::Esc));
        assert!(fx.is_empty());
        assert!(m.pending_kill.is_none());
        let fx = update(&mut m, Msg::Key(Key::Enter));
        assert!(fx.is_empty());
    }

    #[test]
    fn force_quit_wins_over_every_mode() {
        for setup in [
            |m: &mut Model| m.filter_input = true,
            |m: &mut Model| m.show_help = true,
            |_m: &mut Model| {},
        ] {
            let mut m = Model::new();
            setup(&mut m);
            let fx = update(&mut m, Msg::Key(Key::ForceQuit));
            assert!(matches!(fx.first(), Some(Effect::Quit)));
        }
    }
}
