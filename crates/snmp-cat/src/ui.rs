use std::time::SystemTime;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, ConnectionState, FocusedPanel, ResultValue};

/// Render the entire application UI.
pub fn draw(frame: &mut Frame, app: &mut App) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title bar
            Constraint::Min(0),    // main area
            Constraint::Length(1), // status bar
        ])
        .split(frame.area());

    draw_title_bar(frame, outer[0], app);
    draw_main_area(frame, outer[1], app);
    draw_status_bar(frame, outer[2], app);
}

fn draw_title_bar(frame: &mut Frame, area: Rect, app: &App) {
    let conn_span = match &app.connection {
        ConnectionState::Disconnected => {
            Span::styled("[No device]", Style::default().fg(Color::DarkGray))
        }
        ConnectionState::Connecting => {
            Span::styled("[Connecting...]", Style::default().fg(Color::Yellow))
        }
        ConnectionState::Connected { host, version } => Span::styled(
            format!("[{} {}]", host, version),
            Style::default().fg(Color::Green),
        ),
        ConnectionState::Error(e) => {
            Span::styled(format!("[Error: {}]", e), Style::default().fg(Color::Red))
        }
    };

    let title = Line::from(vec![
        Span::styled(
            "snmp-cat",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        conn_span,
    ]);
    frame.render_widget(Paragraph::new(title).centered(), area);
}

fn draw_main_area(frame: &mut Frame, area: Rect, app: &mut App) {
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30), // tree
            Constraint::Percentage(70), // right panels
        ])
        .split(area);

    let right_vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(50), // detail
            Constraint::Percentage(50), // results
        ])
        .split(horizontal[1]);

    draw_tree_panel(frame, horizontal[0], app);
    draw_detail_panel(frame, right_vertical[0], app);
    draw_results_panel(frame, right_vertical[1], app);
}

fn panel_border_style(focused: FocusedPanel, panel: FocusedPanel) -> Style {
    if focused == panel {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn draw_tree_panel(frame: &mut Frame, area: Rect, app: &mut App) {
    let style = panel_border_style(app.focused, FocusedPanel::Tree);
    let block = Block::default()
        .title(" MIB Tree ")
        .borders(Borders::ALL)
        .border_style(style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let viewport_height = inner.height as usize;
    app.tree_state.ensure_visible(viewport_height);

    let visible = app.tree_state.visible_nodes();
    let scroll = app.tree_state.scroll_offset;
    let selected = app.tree_state.selected;

    let mut lines: Vec<Line> = Vec::new();
    let end = (scroll + viewport_height).min(visible.len());

    for (i, &(node_idx, depth)) in visible.iter().enumerate().take(end).skip(scroll) {
        let is_selected = i == selected;

        let node = match app.oid_tree.get(node_idx) {
            Some(n) => n,
            None => continue,
        };

        let has_children = !node.children.is_empty();
        let is_expanded = app.tree_state.is_expanded(node_idx);

        let indent = "  ".repeat(depth);
        let prefix = if has_children {
            if is_expanded { "▾ " } else { "▸ " }
        } else {
            "  "
        };

        let label = if node.name.is_empty() {
            format!("{}", node.subid)
        } else if has_children {
            format!("{}({})", node.name, node.subid)
        } else {
            node.name.clone()
        };

        let text = format!("{}{}{}", indent, prefix, label);

        let line_style = if is_selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else if has_children {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Gray)
        };

        lines.push(Line::from(Span::styled(text, line_style)));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "No MIBs loaded",
            Style::default().fg(Color::DarkGray),
        )));
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_detail_panel(frame: &mut Frame, area: Rect, app: &mut App) {
    let style = panel_border_style(app.focused, FocusedPanel::Detail);
    let block = Block::default()
        .title(" Object Detail ")
        .borders(Borders::ALL)
        .border_style(style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = build_detail_lines(app);
    let total_lines = lines.len();
    let viewport_height = inner.height as usize;

    // Update detail state with actual dimensions for scroll bounds
    app.detail_state.total_lines = total_lines;
    app.detail_state.viewport_height = viewport_height;

    // Clamp scroll offset
    if total_lines > viewport_height {
        if app.detail_state.scroll_offset > total_lines - viewport_height {
            app.detail_state.scroll_offset = total_lines - viewport_height;
        }
    } else {
        app.detail_state.scroll_offset = 0;
    }

    let scroll = app.detail_state.scroll_offset;
    let visible_lines: Vec<Line> = lines
        .into_iter()
        .skip(scroll)
        .take(viewport_height)
        .collect();

    frame.render_widget(Paragraph::new(visible_lines), inner);
}

fn build_detail_lines(app: &App) -> Vec<Line<'static>> {
    let node_idx = match app.tree_state.selected_node() {
        Some(idx) => idx,
        None => {
            return vec![Line::from(Span::styled(
                "Select a node in the MIB tree",
                Style::default().fg(Color::DarkGray),
            ))];
        }
    };

    let node = match app.oid_tree.get(node_idx) {
        Some(n) => n,
        None => return vec![],
    };

    let oid = app
        .oid_tree
        .resolve_oid(node_idx)
        .map(|o| o.to_string())
        .unwrap_or_default();

    let label_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let value_style = Style::default().fg(Color::White);
    let dim_style = Style::default().fg(Color::DarkGray);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("  Name:    ", label_style),
            Span::styled(
                if node.name.is_empty() {
                    format!("{}", node.subid)
                } else {
                    node.name.clone()
                },
                value_style,
            ),
        ]),
        Line::from(vec![
            Span::styled("  OID:     ", label_style),
            Span::styled(oid, value_style),
        ]),
    ];

    if let Some(ref mib_obj) = node.mib_object {
        lines.push(Line::from(vec![
            Span::styled("  Module:  ", label_style),
            Span::styled(mib_obj.module.clone(), value_style),
        ]));

        if let Some(ref syntax) = mib_obj.syntax {
            lines.push(Line::from(vec![
                Span::styled("  Syntax:  ", label_style),
                Span::styled(format!("{:?}", syntax), value_style),
            ]));
        }

        if let Some(ref access) = mib_obj.access {
            lines.push(Line::from(vec![
                Span::styled("  Access:  ", label_style),
                Span::styled(format!("{:?}", access), value_style),
            ]));
        }

        if let Some(ref status) = mib_obj.status {
            lines.push(Line::from(vec![
                Span::styled("  Status:  ", label_style),
                Span::styled(format!("{:?}", status), value_style),
            ]));
        }

        if let Some(ref index_clause) = mib_obj.index_clause {
            lines.push(Line::from(vec![
                Span::styled("  Index:   ", label_style),
                Span::styled(index_clause.join(", "), value_style),
            ]));
        }

        // For table/row objects with children, show column list
        if !node.children.is_empty() {
            let child_names: Vec<String> = node
                .children
                .iter()
                .filter_map(|&child_idx| {
                    app.oid_tree.get(child_idx).map(|child| {
                        if child.name.is_empty() {
                            format!("{}", child.subid)
                        } else {
                            child.name.clone()
                        }
                    })
                })
                .collect();

            if !child_names.is_empty() {
                lines.push(Line::from(Span::raw("")));
                lines.push(Line::from(Span::styled("  Children:", label_style)));
                for name in &child_names {
                    lines.push(Line::from(Span::styled(
                        format!("    {}", name),
                        value_style,
                    )));
                }
            }
        }

        if let Some(ref desc) = mib_obj.description {
            lines.push(Line::from(Span::raw("")));
            let desc = desc.trim_matches('"').trim();
            for line in desc.lines() {
                lines.push(Line::from(Span::styled(
                    format!("  {}", line.trim()),
                    dim_style,
                )));
            }
        }
    } else {
        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(
            "  (no MIB object data)",
            dim_style,
        )));
    }

    lines
}

fn draw_results_panel(frame: &mut Frame, area: Rect, app: &mut App) {
    let style = panel_border_style(app.focused, FocusedPanel::Results);
    let block = Block::default()
        .title(" Query Results ")
        .borders(Borders::ALL)
        .border_style(style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let viewport_height = inner.height as usize;
    app.results_state.viewport_height = viewport_height;

    if app.results_state.entries.is_empty() {
        let placeholder = Line::from(Span::styled(
            "SNMP query results will appear here.",
            Style::default().fg(Color::DarkGray),
        ));
        frame.render_widget(Paragraph::new(vec![placeholder]), inner);
        return;
    }

    let lines = build_results_lines(app);
    let total_lines = lines.len();
    app.results_state.total_lines = total_lines;

    // Auto-scroll to bottom on new entries
    if app.results_state.auto_scroll && total_lines > viewport_height {
        app.results_state.scroll_offset = total_lines - viewport_height;
    }

    // Clamp scroll offset
    if total_lines > viewport_height {
        if app.results_state.scroll_offset > total_lines - viewport_height {
            app.results_state.scroll_offset = total_lines - viewport_height;
        }
    } else {
        app.results_state.scroll_offset = 0;
    }

    let scroll = app.results_state.scroll_offset;
    let visible_lines: Vec<Line> = lines
        .into_iter()
        .skip(scroll)
        .take(viewport_height)
        .collect();

    frame.render_widget(Paragraph::new(visible_lines), inner);
}

fn build_results_lines(app: &App) -> Vec<Line<'static>> {
    let header_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let oid_style = Style::default().fg(Color::Cyan);
    let value_style = Style::default().fg(Color::White);
    let error_style = Style::default().fg(Color::Red);
    let dim_style = Style::default().fg(Color::DarkGray);

    let mut lines = Vec::new();

    for (i, entry) in app.results_state.entries.iter().enumerate() {
        if i > 0 {
            lines.push(Line::from(Span::styled("─".repeat(40), dim_style)));
        }

        let time_str = format_timestamp(entry.timestamp);
        lines.push(Line::from(vec![
            Span::styled(format!("[{}] ", time_str), dim_style),
            Span::styled(format!("{}", entry.operation), header_style),
            Span::styled(format!("  {}", entry.target), dim_style),
        ]));

        match &entry.result {
            ResultValue::Single(val) => {
                lines.push(Line::from(vec![
                    Span::styled("  ", value_style),
                    Span::styled(entry.oid.clone(), oid_style),
                    Span::styled(" = ", dim_style),
                    Span::styled(val.clone(), value_style),
                ]));
            }
            ResultValue::Multiple(pairs) => {
                for (oid, val) in pairs {
                    lines.push(Line::from(vec![
                        Span::styled("  ", value_style),
                        Span::styled(oid.clone(), oid_style),
                        Span::styled(" = ", dim_style),
                        Span::styled(val.clone(), value_style),
                    ]));
                }
            }
            ResultValue::Error(err) => {
                lines.push(Line::from(vec![
                    Span::styled("  ", error_style),
                    Span::styled(entry.oid.clone(), oid_style),
                    Span::styled(" -> ", dim_style),
                    Span::styled(err.clone(), error_style),
                ]));
            }
        }
    }

    lines
}

fn format_timestamp(ts: SystemTime) -> String {
    match ts.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => {
            let secs = dur.as_secs();
            let hours = (secs / 3600) % 24;
            let minutes = (secs / 60) % 60;
            let seconds = secs % 60;
            format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
        }
        Err(_) => "??:??:??".to_string(),
    }
}

fn draw_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    // Show loading indicator if an operation is in-flight
    if let Some(ref op) = app.inflight_op {
        let loading = Line::from(Span::styled(
            format!(" [{} in progress...] ", op),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
        frame.render_widget(Paragraph::new(loading), area);
        return;
    }

    let hints = match app.focused {
        FocusedPanel::Tree => {
            "[Tab] Switch  [j/k] Navigate  [Enter] Expand  [h/l] Collapse/Expand  [gg/G] Top/Bottom  [Space] GET  [n] GETNEXT  [w] WALK  [q] Quit"
        }
        FocusedPanel::Detail => "[Tab] Switch  [j/k] Scroll  [q] Quit",
        FocusedPanel::Results => "[Tab] Switch  [j/k] Scroll  [G] Latest  [q] Quit",
    };

    let status = Line::from(Span::styled(hints, Style::default().fg(Color::DarkGray)));
    frame.render_widget(Paragraph::new(status), area);
}
