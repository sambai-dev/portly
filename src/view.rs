use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table},
    Frame,
};

use crate::model::Model;

pub fn render(f: &mut Frame, m: &Model) {
    let [area, status] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(f.area());

    let header = Row::new(["PORT", "PROTO", "PID", "PROCESS", "CPU%", "MEM", "SRC"])
        .style(Style::new().add_modifier(Modifier::BOLD));

    let widths = [
        Constraint::Length(7),
        Constraint::Length(6),
        Constraint::Length(8),
        Constraint::Min(18),
        Constraint::Length(6),
        Constraint::Length(9),
        Constraint::Length(8),
    ];

    let vis = m.visible();
    let rows: Vec<Row> = vis
        .iter()
        .enumerate()
        .map(|(row_i, &i)| {
            let e = &m.entries[i];
            let cells = vec![
                Cell::from(e.port.to_string()),
                Cell::from(e.proto.to_string()),
                Cell::from(e.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into())),
                Cell::from(e.process.clone().unwrap_or_else(|| "(unknown)".into())),
                Cell::from(fmt_cpu(e.cpu)),
                Cell::from(fmt_mem(e.mem_bytes)),
                Cell::from(e.source.to_string()),
            ];
            if row_i == m.selected {
                Row::new(cells).style(Style::new().add_modifier(Modifier::REVERSED))
            } else {
                Row::new(cells)
            }
        })
        .collect();

    let title = format!(
        " Portly · {} service{} · sort:{} {}",
        vis.len(),
        if vis.len() == 1 { "" } else { "s" },
        m.sort,
        if m.pending_kill.is_some() {
            "· KILL ARMED"
        } else {
            ""
        }
    );

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(title));
    f.render_widget(table, area);

    f.render_widget(status_line(m), status);

    if m.pending_kill.is_some() {
        let popup = centered_rect(46, 5, area);
        f.render_widget(Clear, popup);
        f.render_widget(
            Paragraph::new(Line::from(format!(" {} ", m.status)))
                .block(Block::default().borders(Borders::ALL).title(" confirm ")),
            popup,
        );
    }

    if m.show_help {
        let popup = centered_rect(52, 15, area);
        f.render_widget(Clear, popup);
        f.render_widget(
            Paragraph::new(vec![
                Line::from("j / ↓, k / ↑   move selection"),
                Line::from("/              filter (substring)"),
                Line::from("s              cycle sort: port → cpu → mem"),
                Line::from("x              arm kill for selected row"),
                Line::from("enter          execute armed action"),
                Line::from("esc            cancel / clear filter"),
                Line::from("?              toggle this help"),
                Line::from("q              quit"),
            ])
            .block(Block::default().borders(Borders::ALL).title(" keys ")),
            popup,
        );
    }
}

fn status_line(m: &Model) -> Paragraph<'static> {
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
        _ => line.push_str("[j/k] move · [x] kill · [s] sort · [/] filter · [?] help · q quit"),
    }
    Paragraph::new(Line::from(line))
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
        Some(b) => format!("{b}K"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PortEntry, Protocol, Source};
    use ratatui::{backend::TestBackend, Terminal};

    fn entry(port: u16, pid: Option<u32>, name: &str) -> PortEntry {
        PortEntry {
            port,
            proto: Protocol::Tcp,
            pid,
            process: Some(name.to_string()),
            cmdline: None,
            cpu: Some(2.5),
            mem_bytes: Some(12 * 1024 * 1024),
            source: Source::Proc,
        }
    }

    fn frame_text(w: u16, h: u16, m: &Model) -> String {
        let backend = TestBackend::new(w, h);
        let mut term = Terminal::new(backend).expect("terminal");
        term.draw(|f| render(f, m)).expect("draw");
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
            entry(3000, Some(4121), "node"),
            entry(5432, Some(889), "postgres"),
        ];
        let text = frame_text(80, 24, &m);
        for needle in [
            "PORT", "PROTO", "PID", "PROCESS", "3000", "node", "5432", "postgres", "12M", "2.5",
        ] {
            assert!(text.contains(needle), "frame missing {needle:?}\n{text}");
        }
    }

    #[test]
    fn kill_confirmation_renders_banner() {
        let mut m = Model::new();
        m.entries = vec![entry(3000, Some(42), "node")];
        let _ = crate::model::update(&mut m, crate::model::Msg::Key(crate::model::Key::Char('x')));
        assert!(m.pending_kill.is_some());
        let text = frame_text(80, 24, &m);
        assert!(text.contains("KILL ARMED"), "{text}");
        assert!(text.contains("confirm"), "{text}");
    }

    #[test]
    fn help_overlay_lists_keys() {
        let mut m = Model::new();
        m.entries = vec![entry(8080, Some(9), "nginx")];
        let _ = crate::model::update(&mut m, crate::model::Msg::Key(crate::model::Key::Char('?')));
        let text = frame_text(80, 24, &m);
        assert!(
            text.contains("keys") && text.contains("cycle sort"),
            "{text}"
        );
    }

    #[test]
    fn empty_state_is_stable() {
        let text = frame_text(80, 24, &Model::new());
        assert!(text.contains("Portly"));
        assert!(text.contains("move"));
    }
}
