use std::time::Instant;

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table},
    Frame,
};

use crate::config::Theme;
use crate::model::{body_capacity, clamp_view_start, Health, Model};

const SPARK_GLYPHS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

pub fn render(f: &mut Frame, m: &Model, theme: &Theme, now: Instant) {
    let log_open = m.log_target_title.is_some();
    let log_h = if log_open { area_height(f.area()) } else { 0 };
    let [table_area, log_area, status] = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(log_h),
        Constraint::Length(1),
    ])
    .areas(f.area());

    // Report the body geometry so mouse clicks can hit-test rows (flows back
    // through Msg::FrameGeometry — rendering itself stays pure).
    report_geometry(table_area);

    let header = Row::new([
        "PORT", "PROTO", "PID", "PROCESS", "TREND", "CPU%", "MEM", "HEALTH", "SRC",
    ])
    .style(Style::new().fg(theme.header).add_modifier(Modifier::BOLD));

    let widths = [
        Constraint::Length(7),
        Constraint::Length(6),
        Constraint::Length(8),
        Constraint::Min(16),
        Constraint::Length(11),
        Constraint::Length(6),
        Constraint::Length(8),
        Constraint::Length(8),
        Constraint::Length(7),
    ];

    let vis = m.visible();
    let selected_port = vis.get(m.selected).map(|&i| m.entries[i].port);
    // Render only the viewport window: `start` is re-clamped against THIS
    // frame's table rect (not the model's possibly one-frame-old geometry), so
    // the selected row is visible even right after a terminal resize.
    let capacity = body_capacity(table_area.height);
    let start = clamp_view_start(m.scroll, m.selected, capacity);
    let end = (start + capacity).min(vis.len());
    let rows: Vec<Row> = vis[start..end]
        .iter()
        .enumerate()
        .map(|(row_i, &i)| {
            let e = &m.entries[i];
            let cells = vec![
                Cell::from(e.port.to_string()),
                Cell::from(e.proto.to_string()),
                Cell::from(e.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into())),
                Cell::from(e.display_name()),
                Cell::from(trend_glyphs(m, e)),
                Cell::from(fmt_cpu(e.cpu)),
                Cell::from(fmt_mem(e.mem_bytes)),
                health_cell(m, e.port, selected_port == Some(e.port), theme),
                Cell::from(e.source.to_string()),
            ];
            if start + row_i == m.selected {
                Row::new(cells).style(Style::new().add_modifier(Modifier::REVERSED))
            } else {
                Row::new(cells)
            }
        })
        .collect();

    let mut title_spans = vec![
        Span::styled(
            " Portly ",
            Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "· {} service{} · sort:{}",
            vis.len(),
            if vis.len() == 1 { "" } else { "s" },
            m.sort,
        )),
    ];
    if let Some(age) = m.data_age_secs(now) {
        let stale = age * 1000 > m.interval.as_millis() as u64 * 2;
        title_spans.push(Span::styled(
            format!(" · data {age}s{}", if stale { " STALE" } else { "" }),
            Style::new().fg(if stale { theme.crit } else { theme.muted }),
        ));
    }
    if m.pending_action.is_some() {
        title_spans.push(Span::styled(
            " · ACTION ARMED ",
            Style::new().fg(theme.crit).add_modifier(Modifier::BOLD),
        ));
    }

    let table = Table::new(rows, widths).header(header).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(theme.border))
            .title(Line::from(title_spans)),
    );
    f.render_widget(table, table_area);

    f.render_widget(status_line(m, theme), status);

    if let Some(title) = &m.log_target_title {
        render_logs(f, m, theme, title, log_area);
    }

    if let Some(armed) = &m.pending_action {
        let popup = centered_rect(52, 5, table_area);
        f.render_widget(Clear, popup);
        f.render_widget(
            Paragraph::new(Line::from(format!(" {} ", armed.label)))
                .style(Style::new().fg(theme.crit))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::new().fg(theme.crit))
                        .title(" confirm "),
                ),
            popup,
        );
    }

    if m.show_help {
        let popup = centered_rect(58, 15, table_area);
        f.render_widget(Clear, popup);
        let keys: Vec<Line> = vec![
            Line::from("j / k, ↑ / ↓    move selection (view follows)"),
            Line::from("click / wheel   select row / move selection"),
            Line::from("/               filter (substring)"),
            Line::from("s               cycle sort: port → cpu → mem"),
            Line::from("l               toggle logs pane for selection"),
            Line::from("pgup / pgdn     scroll logs (when open)"),
            Line::from("x               arm contextual action"),
            Line::from("R / T / G       restart / stop / start container"),
            Line::from("enter           execute armed action"),
            Line::from("esc             cancel / clear filter"),
            Line::from("?               toggle this help"),
            Line::from("q               quit"),
        ];
        f.render_widget(
            Paragraph::new(keys).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::new().fg(theme.border))
                    .title(" keys "),
            ),
            popup,
        );
    }
}

fn area_height(total: Rect) -> u16 {
    (total.height.saturating_sub(2) / 3).clamp(4, 14)
}

thread_local! {
    static LAST_GEOMETRY: std::cell::Cell<Option<(u16, u16, u16, u16)>> = const { std::cell::Cell::new(None) };
}

fn report_geometry(area: Rect) {
    LAST_GEOMETRY.with(|g| g.set(Some((area.x, area.y, area.width, area.height))));
}

/// Drain the geometry reported by the last render, to be fed back into the
/// event loop as `Msg::FrameGeometry`.
pub fn take_geometry() -> Option<(u16, u16, u16, u16)> {
    LAST_GEOMETRY.with(|g| g.take())
}

fn trend_glyphs(m: &Model, e: &crate::model::PortEntry) -> String {
    match m.trend.get(&e.key()) {
        Some(buf) if buf.len() >= 2 => buf
            .iter()
            .rev()
            .take(10)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|v| {
                let level = ((v.clamp(0.0, 100.0) / 100.0) * 7.99) as usize;
                SPARK_GLYPHS[level.min(7)]
            })
            .collect(),
        _ => "\u{b7}\u{b7}\u{b7}".to_string(), // ··· until two samples exist
    }
}

fn health_cell(m: &Model, port: u16, _selected: bool, theme: &Theme) -> Cell<'static> {
    if !m.health_enabled {
        return Cell::from("-");
    }
    let (state, ms) = m
        .health
        .get(&port)
        .copied()
        .unwrap_or((Health::Unknown, None));
    let text = match ms {
        Some(v) => format!("{} {: >3}ms", state.glyph(), v),
        None => format!("{}  ", state.glyph()),
    };
    let color = match state {
        Health::Up => theme.ok,
        Health::Degraded => theme.warn,
        Health::Down => theme.crit,
        Health::Unknown => theme.muted,
    };
    Cell::from(text).style(Style::new().fg(color))
}

fn render_logs(f: &mut Frame, m: &Model, theme: &Theme, title: &str, area: Rect) {
    let inner_height = area.height.saturating_sub(2) as usize;
    let total = m.log_lines.len();
    let end = total.saturating_sub(m.log_scroll);
    let start = end.saturating_sub(inner_height);
    let lines: Vec<Line> = m
        .log_lines
        .iter()
        .skip(start)
        .take(end - start)
        .map(|l| Line::from(l.clone()))
        .collect();
    let follow_hint = if m.log_scroll == 0 {
        "following"
    } else {
        "scrolled"
    };
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::new().fg(theme.border))
                .title(format!(
                    " logs · {title} · [{follow_hint}] pgup/pgdn · esc close "
                )),
        ),
        area,
    );
}

fn status_line<'a>(m: &Model, theme: &'a Theme) -> Paragraph<'a> {
    let mut line = String::new();
    if m.filter_input {
        line.push_str(&format!(
            "filter: {}█ ",
            m.filter.clone().unwrap_or_default()
        ));
    }
    match (&m.last_error, &m.status) {
        (Some(err), _) => line.push_str(&format!("error: {err}")),
        (None, s) if !s.is_empty() => line.push_str(s),
        _ => line.push_str("[j/k] move · [x] act · [R/T/G] container · [l] logs · [s] sort · [/] filter · [?] help · q quit"),
    }
    let style = if m.last_error.is_some() {
        Style::new().fg(theme.crit)
    } else {
        Style::new()
    };
    Paragraph::new(Line::from(line)).style(style)
}

fn fmt_cpu(cpu: Option<f32>) -> String {
    match cpu {
        Some(v) => format!("{v:.1}"),
        None => "-".to_string(),
    }
}

fn fmt_mem(mem: Option<u64>) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    match mem {
        None => "-".to_string(),
        Some(b) if b >= MB => format!("{:.0}M", b as f64 / MB as f64),
        Some(b) if b >= KB => format!("{}K", b / KB),
        Some(b) => format!("{b}B"),
    }
}

fn centered_rect(percent_x: u16, height: u16, r: Rect) -> Rect {
    let width = r.width.saturating_mul(percent_x) / 100;
    let x = r.x + (r.width - width) / 2;
    let y = r.y + (r.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width,
        height: height.min(r.height),
    }
}

/// Plain-text table for `portly --once` (headless mode, scripting).
pub fn snapshot_table(m: &Model) -> String {
    let header = format!(
        "{:<7} {:<5} {:<8} {:<24} {:<7} {:<9} {:<7}",
        "PORT", "PROTO", "PID", "PROCESS", "CPU%", "MEM", "SRC"
    );
    let mut out = String::new();
    out.push_str(&header);
    out.push('\n');
    for &i in &m.visible() {
        let e = &m.entries[i];
        out.push_str(&format!(
            "{:<7} {:<5} {:<8} {:<24} {:<7} {:<9} {:<7}\n",
            e.port,
            e.proto,
            e.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
            truncate(&e.display_name(), 24),
            fmt_cpu(e.cpu),
            fmt_mem(e.mem_bytes),
            e.source,
        ));
    }
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}\u{2026}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PortEntry, Protocol, Source};
    use ratatui::{backend::TestBackend, Terminal};

    fn host(port: u16, pid: Option<u32>, name: &str) -> PortEntry {
        PortEntry {
            port,
            proto: Protocol::Tcp,
            pid,
            process: Some(name.to_string()),
            cmdline: None,
            cpu: Some(2.5),
            mem_bytes: Some(12 * 1024 * 1024),
            source: Source::Proc,
            container: None,
            container_state: None,
        }
    }

    fn frame_text(w: u16, h: u16, m: &Model) -> String {
        let backend = TestBackend::new(w, h);
        let mut term = Terminal::new(backend).expect("terminal");
        term.draw(|f| render(f, m, &Theme::dark(), Instant::now()))
            .expect("draw");
        let buf = term.backend().buffer();
        let mut out = String::new();
        for y in 0..h {
            for x in 0..w {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn frame_shows_header_and_rows() {
        let mut m = Model::new();
        m.entries = vec![
            host(3000, Some(4121), "node"),
            host(5432, Some(889), "postgres"),
        ];
        let text = frame_text(110, 30, &m);
        for needle in [
            "PORT", "PROTO", "PID", "PROCESS", "TREND", "HEALTH", "3000", "node", "5432",
            "postgres", "12M", "2.5",
        ] {
            assert!(text.contains(needle), "frame missing {needle:?}\n{text}");
        }
    }

    #[test]
    fn docker_rows_render_container_name_and_src() {
        let mut m = Model::new();
        let mut e = host(8080, None, "");
        e.process = None;
        e.source = Source::Docker;
        e.container = Some(("abc123".into(), "api".into()));
        e.container_state = Some("running".into());
        m.entries = vec![e];
        let text = frame_text(110, 30, &m);
        assert!(text.contains("api"), "{text}");
        assert!(text.contains("docker"), "{text}");
    }

    #[test]
    fn armed_action_renders_banner_and_title_marker() {
        let mut m = Model::new();
        m.entries = vec![host(3000, Some(42), "node")];
        let _ = crate::model::update(&mut m, crate::model::Msg::Key(crate::model::Key::Char('x')));
        assert!(m.pending_action.is_some());
        let text = frame_text(110, 30, &m);
        assert!(text.contains("ACTION ARMED"), "{text}");
        assert!(text.contains("confirm"), "{text}");
    }

    #[test]
    fn help_overlay_lists_keys() {
        let mut m = Model::new();
        m.entries = vec![host(8080, Some(9), "nginx")];
        let _ = crate::model::update(&mut m, crate::model::Msg::Key(crate::model::Key::Char('?')));
        let text = frame_text(110, 30, &m);
        assert!(
            text.contains("keys") && text.contains("cycle sort"),
            "{text}"
        );
    }

    #[test]
    fn log_pane_renders_lines_and_follow_state() {
        let mut m = Model::new();
        m.entries = vec![host(3000, Some(1), "node")];
        m.cfg_log_files
            .insert(3000, std::path::PathBuf::from("/tmp/a.log"));
        let _ = crate::model::update(&mut m, crate::model::Msg::Key(crate::model::Key::Char('l')));
        let gen = m.log_gen;
        for i in 0..20 {
            crate::model::update(
                &mut m,
                crate::model::Msg::LogLine {
                    gen,
                    line: format!("line-{i}"),
                },
            );
        }
        let text = frame_text(110, 30, &m);
        assert!(text.contains("logs"), "{text}");
        assert!(text.contains("following"), "{text}");
        assert!(text.contains("line-19"), "{text}");
    }

    #[test]
    fn empty_state_is_stable() {
        let text = frame_text(110, 30, &Model::new());
        assert!(text.contains("Portly"));
        assert!(text.contains("move"));
    }

    #[test]
    fn snapshot_table_is_aligned_headless_output() {
        let mut m = Model::new();
        m.entries = vec![host(3000, Some(4121), "node")];
        let table = snapshot_table(&m);
        assert!(table.starts_with("PORT"));
        assert!(table.lines().count() == 2);
        assert!(table.contains("node"));
    }

    /// Audit probe reproduced: 100 rows, 24-line terminal, 60x Down. The
    /// selected deep row must be rendered — before the scroll fix it was not.
    fn hundred_rows_24line_term() -> Model {
        let mut m = Model::new();
        m.entries = (0..100u16)
            .map(|i| host(3000 + i, Some(i as u32), &format!("svc{}", 3000 + i)))
            .collect();
        m.table_area = (0, 0, 110, 23); // matches frame_text(110, 24) geometry
        m
    }

    fn drive_down(m: &mut Model, n: usize) {
        for _ in 0..n {
            let _ = crate::model::update(m, crate::model::Msg::Key(crate::model::Key::Down));
        }
    }

    #[test]
    fn selected_deep_row_is_rendered_after_60_downs() {
        let mut m = hundred_rows_24line_term();
        drive_down(&mut m, 60);
        assert_eq!(m.selected, 60);
        let text = frame_text(110, 24, &m);
        assert!(
            text.contains("svc3060"),
            "selected row must be visible in the frame\n{text}"
        );
    }

    #[test]
    fn first_row_scrolls_out_of_view_when_selection_passes_it() {
        let mut m = hundred_rows_24line_term();
        drive_down(&mut m, 60);
        let text = frame_text(110, 24, &m);
        assert!(
            !text.contains("svc3000"),
            "top row must have scrolled out of the viewport\n{text}"
        );
        // The window is rows 41..=60: svc3041 is the new top line.
        assert!(text.contains("svc3041"), "{text}");
    }

    #[test]
    fn scrolling_back_up_brings_first_row_into_view_again() {
        let mut m = hundred_rows_24line_term();
        drive_down(&mut m, 60);
        for _ in 0..60 {
            let _ = crate::model::update(&mut m, crate::model::Msg::Key(crate::model::Key::Up));
        }
        let text = frame_text(110, 24, &m);
        assert!(text.contains("svc3000"), "{text}");
        assert!(!text.contains("svc3060"), "{text}");
    }
}

#[cfg(test)]
mod audit_tests {
    use super::*;

    #[test]
    fn mem_tiers_never_mislabel_bytes_as_k() {
        assert_eq!(fmt_mem(Some(500)), "500B");
        assert_eq!(fmt_mem(Some(1024)), "1K");
        assert_eq!(fmt_mem(Some(500_000)), "488K");
        assert_eq!(fmt_mem(Some(12 * 1024 * 1024)), "12M");
        assert_eq!(fmt_mem(None), "-");
    }
}
