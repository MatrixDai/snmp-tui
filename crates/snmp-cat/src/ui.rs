use std::time::SystemTime;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::{App, ConnectionState, FocusedPanel, PanelSearch, ResultValue};
use crate::modal::Modal;

/// Render the entire application UI.
pub fn draw(frame: &mut Frame, app: &mut App) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title bar
            Constraint::Min(0),    // main area
            Constraint::Length(2), // status bar (2 lines)
        ])
        .split(frame.area());

    draw_title_bar(frame, outer[0], app);
    draw_main_area(frame, outer[1], app);
    draw_status_bar(frame, outer[2], app);

    // Render modal overlay if active
    if app.modal.is_some() {
        draw_modal(frame, app);
    }

    // Render help overlay if active
    if app.show_help {
        draw_help_overlay(frame);
    }

    // Clear expired status messages (after 2 seconds)
    if let Some((_, instant)) = &app.status_message
        && instant.elapsed() > std::time::Duration::from_secs(2)
    {
        app.status_message = None;
    }
}

fn draw_title_bar(frame: &mut Frame, area: Rect, app: &App) {
    let conn_span = match &app.connection {
        ConnectionState::Disconnected => {
            Span::styled("[No device]", Style::default().fg(Color::Gray))
        }
        ConnectionState::Connecting => {
            Span::styled("[Connecting...]", Style::default().fg(Color::Yellow))
        }
        ConnectionState::Validating { .. } => {
            Span::styled("[Validating...]", Style::default().fg(Color::Yellow))
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
            Constraint::Percentage(30), // detail
            Constraint::Percentage(70), // results
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

fn panel_title_style(focused: FocusedPanel, panel: FocusedPanel) -> Style {
    if focused == panel {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    }
}

fn draw_tree_panel(frame: &mut Frame, area: Rect, app: &mut App) {
    let style = panel_border_style(app.focused, FocusedPanel::Tree);
    let title_style = panel_title_style(app.focused, FocusedPanel::Tree);
    let block = Block::default()
        .title(" MIB Tree ")
        .title_style(title_style)
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
            "No MIBs loaded.",
            Style::default().fg(Color::Yellow),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Add MIB files via --mib-dir or --mib-file,",
            Style::default().fg(Color::Gray),
        )));
        lines.push(Line::from(Span::styled(
            "or configure mib_dirs in config.toml.",
            Style::default().fg(Color::Gray),
        )));
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_detail_panel(frame: &mut Frame, area: Rect, app: &mut App) {
    let style = panel_border_style(app.focused, FocusedPanel::Detail);
    let title_style = panel_title_style(app.focused, FocusedPanel::Detail);
    let block = Block::default()
        .title(" Object Detail ")
        .title_style(title_style)
        .borders(Borders::ALL)
        .border_style(style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let search_input = app.detail_state.search.active;
    let search_highlighting = search_input || app.detail_state.search.confirmed;
    let (content_area, search_area) = if search_input {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(inner);
        (chunks[0], Some(chunks[1]))
    } else {
        (inner, None)
    };

    let lines = build_detail_lines(app);
    let total_lines = lines.len();
    let viewport_height = content_area.height as usize;

    // Update search matches
    if search_highlighting {
        app.detail_state.search.update_matches(&lines);
    }

    // Auto-scroll to current match
    if search_input && let Some(match_line) = app.detail_state.search.current_line() {
        let half = viewport_height / 2;
        app.detail_state.scroll_offset = match_line.saturating_sub(half);
    }

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
    let query = if search_highlighting && !app.detail_state.search.query.is_empty() {
        Some(app.detail_state.search.query.clone())
    } else {
        None
    };
    let visible_lines: Vec<Line> = lines
        .into_iter()
        .skip(scroll)
        .take(viewport_height)
        .map(|line| {
            if let Some(ref q) = query {
                highlight_line(line, q)
            } else {
                line
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(visible_lines), content_area);

    if let Some(sa) = search_area {
        draw_search_bar(frame, sa, &app.detail_state.search);
    }
}

fn build_detail_lines(app: &App) -> Vec<Line<'static>> {
    let node_idx = match app.tree_state.selected_node() {
        Some(idx) => idx,
        None => {
            return vec![Line::from(Span::styled(
                "Select a node in the MIB tree",
                Style::default().fg(Color::Gray),
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
    let dim_style = Style::default().fg(Color::Gray);

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
    let title_style = panel_title_style(app.focused, FocusedPanel::Results);
    let block = Block::default()
        .title(" Query Results ")
        .title_style(title_style)
        .borders(Borders::ALL)
        .border_style(style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let search_input = app.results_state.search.active;
    let search_highlighting = search_input || app.results_state.search.confirmed;
    let (content_area, search_area) = if search_input {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(inner);
        (chunks[0], Some(chunks[1]))
    } else {
        (inner, None)
    };

    let viewport_height = content_area.height as usize;
    app.results_state.viewport_height = viewport_height;

    if app.results_state.entries.is_empty() {
        let msg = if matches!(app.connection, ConnectionState::Connected { .. }) {
            "Select an OID and press [Space] to GET, [n] GETNEXT, or [w] WALK."
        } else {
            "Press [o] to connect to an SNMP device."
        };
        let placeholder = Line::from(Span::styled(msg, Style::default().fg(Color::Gray)));
        frame.render_widget(Paragraph::new(vec![placeholder]), content_area);
        if let Some(sa) = search_area {
            draw_search_bar(frame, sa, &app.results_state.search);
        }
        return;
    }

    let lines = build_results_lines(app);
    let total_lines = lines.len();
    app.results_state.total_lines = total_lines;

    // Update search matches
    if search_highlighting {
        app.results_state.search.update_matches(&lines);
    }

    // Auto-scroll to current match when searching
    if search_input && let Some(match_line) = app.results_state.search.current_line() {
        let half = viewport_height / 2;
        app.results_state.scroll_offset = match_line.saturating_sub(half);
        app.results_state.auto_scroll = false;
    } else if app.results_state.auto_scroll && total_lines > viewport_height {
        // Auto-scroll to bottom on new entries
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
    let query = if search_highlighting && !app.results_state.search.query.is_empty() {
        Some(app.results_state.search.query.clone())
    } else {
        None
    };
    let visible_lines: Vec<Line> = lines
        .into_iter()
        .skip(scroll)
        .take(viewport_height)
        .map(|line| {
            if let Some(ref q) = query {
                highlight_line(line, q)
            } else {
                line
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(visible_lines), content_area);

    if let Some(sa) = search_area {
        draw_search_bar(frame, sa, &app.results_state.search);
    }
}

fn build_results_lines(app: &App) -> Vec<Line<'static>> {
    let header_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let oid_style = Style::default().fg(Color::Cyan);
    let value_style = Style::default().fg(Color::White);
    let error_style = Style::default().fg(Color::Red);
    let dim_style = Style::default().fg(Color::Gray);

    let mut lines = Vec::new();

    for (i, entry) in app.results_state.entries.iter().enumerate() {
        if i > 0 {
            lines.push(Line::from(Span::styled("─".repeat(40), dim_style)));
        }

        let time_str = format_timestamp(entry.timestamp);
        // Header: [time] OP numeric_oid
        if entry.oid.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(format!("[{}] ", time_str), dim_style),
                Span::styled(format!("{}", entry.operation), header_style),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(format!("[{}] ", time_str), dim_style),
                Span::styled(format!("{} ", entry.operation), header_style),
                Span::styled(entry.oid.clone(), oid_style),
            ]));
        }

        match &entry.result {
            ResultValue::Single(val) => {
                if entry.object_name.is_empty() {
                    // No resolved name (e.g. CONNECT/DISCONNECT) — just show the message
                    lines.push(Line::from(vec![
                        Span::styled("  ", value_style),
                        Span::styled(val.clone(), value_style),
                    ]));
                } else {
                    // Value line: name.instance = Type: value
                    lines.push(Line::from(vec![
                        Span::styled("  ", value_style),
                        Span::styled(entry.object_name.clone(), oid_style),
                        Span::styled(" = ", dim_style),
                        Span::styled(val.clone(), value_style),
                    ]));
                }
            }
            ResultValue::Multiple(pairs) => {
                for (name, val) in pairs {
                    lines.push(Line::from(vec![
                        Span::styled("  ", value_style),
                        Span::styled(name.clone(), oid_style),
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
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(area);

    // Line 1: global keys or modal context
    let line1 = status_line1(app);
    frame.render_widget(Paragraph::new(line1), rows[0]);

    // Line 2: panel/modal hints or transient message
    let line2 = status_line2(app);
    frame.render_widget(Paragraph::new(line2), rows[1]);
}

fn status_line1(app: &App) -> Line<'static> {
    if let Some(ref modal) = app.modal {
        let label = match modal {
            Modal::ConnectionManager(mgr) => {
                if mgr.edit_view.is_some() {
                    "Connection Manager > Edit"
                } else {
                    "Connection Manager"
                }
            }
            Modal::Set(_) => "SET Value",
            Modal::Search(_) => "Search MIB Objects",
            Modal::MibInfo(_) => "MIB Modules",
        };
        Line::from(Span::styled(
            format!(" {}", label),
            Style::default().fg(Color::Cyan),
        ))
    } else {
        Line::from(Span::styled(
            " [Tab] Switch  [c] Connect  [m] MIBs  [Ctrl+K] Clear  [/] Search  [?] Help  [q] Quit",
            Style::default().fg(Color::DarkGray),
        ))
    }
}

fn status_line2(app: &App) -> Line<'static> {
    // Transient status message takes priority
    if let Some((ref msg, _)) = app.status_message {
        return Line::from(Span::styled(
            format!(" {} ", msg),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // In-flight operation indicator
    if let Some(ref op) = app.inflight_op {
        return Line::from(Span::styled(
            format!(" [{} in progress...] ", op),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // Modal-specific hints
    if let Some(ref modal) = app.modal {
        let hints = match modal {
            Modal::ConnectionManager(mgr) => {
                if mgr.edit_view.is_some() {
                    " [Tab] Next field  [Up/Down] Navigate/Cycle  [Enter] Save  [Esc] Back"
                } else {
                    " [j/k] Navigate  [Enter] Connect  [n] New  [e] Edit  [d] Delete  [Esc] Close"
                }
            }
            Modal::Set(_) => " [Tab] Next field  [Enter] Send SET  [Esc] Cancel",
            Modal::Search(_) => " [Enter] Navigate to  [Up/Down] Select  [Esc] Cancel",
            Modal::MibInfo(_) => " [j/k] Navigate  [/] Search  [Esc] Close",
        };
        return Line::from(Span::styled(hints, Style::default().fg(Color::Gray)));
    }

    // Inline search hints
    let (search_input, search_confirmed) = match app.focused {
        FocusedPanel::Detail => (
            app.detail_state.search.active,
            app.detail_state.search.confirmed,
        ),
        FocusedPanel::Results => (
            app.results_state.search.active,
            app.results_state.search.confirmed,
        ),
        _ => (false, false),
    };

    if search_input {
        return Line::from(Span::styled(
            " Search: Type to search  [Enter] Confirm  [Esc] Cancel",
            Style::default().fg(Color::Gray),
        ));
    }
    if search_confirmed {
        return Line::from(Span::styled(
            " Search: [n/N] Next/Prev match  [/] New search  [Esc] Clear",
            Style::default().fg(Color::Gray),
        ));
    }

    // Panel-specific hints
    let is_connected = matches!(app.connection, ConnectionState::Connected { .. });
    let panel_prefix_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let hint_style = Style::default().fg(Color::Gray);

    match app.focused {
        FocusedPanel::Tree => {
            let hints = if is_connected {
                "[j/k] Navigate  [Enter] Expand  [r] Reset  [Space] GET  [n] GETNEXT  [w] WALK  [s] SET  [y] Copy"
            } else {
                "[j/k] Navigate  [Enter] Expand  [r] Reset  [y] Copy"
            };
            Line::from(vec![
                Span::styled(" MIB Tree: ", panel_prefix_style),
                Span::styled(hints, hint_style),
            ])
        }
        FocusedPanel::Detail => Line::from(vec![
            Span::styled(" Detail: ", panel_prefix_style),
            Span::styled(
                "[j/k] Scroll  [gg] Top  [G] Bottom  [/] Search  [y] Copy",
                hint_style,
            ),
        ]),
        FocusedPanel::Results => Line::from(vec![
            Span::styled(" Results: ", panel_prefix_style),
            Span::styled(
                "[j/k] Scroll  [gg] Top  [G] Latest  [/] Search  [y] Copy",
                hint_style,
            ),
        ]),
    }
}

// ============================================================
// Inline search helpers
// ============================================================

fn draw_search_bar(frame: &mut Frame, area: Rect, search: &PanelSearch) {
    let info = if search.matches.is_empty() && !search.query.is_empty() {
        " [No matches]".to_string()
    } else if !search.matches.is_empty() {
        format!(" [{}/{}]", search.current_match + 1, search.matches.len())
    } else {
        String::new()
    };
    let line = Line::from(vec![
        Span::styled("/", Style::default().fg(Color::Yellow)),
        Span::styled(search.query.clone(), Style::default().fg(Color::White)),
        Span::styled(info, Style::default().fg(Color::Gray)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

/// Highlight occurrences of `query` (case-insensitive) within a Line by splitting spans.
fn highlight_line<'a>(line: Line<'a>, query: &str) -> Line<'a> {
    let highlight_style = Style::default().fg(Color::Black).bg(Color::Yellow);
    let query_lower = query.to_lowercase();
    let mut new_spans: Vec<Span<'a>> = Vec::new();

    for span in line.spans {
        let text = span.content.as_ref();
        let text_lower = text.to_lowercase();
        let mut start = 0;
        let mut found = false;

        while let Some(pos) = text_lower[start..].find(&query_lower) {
            found = true;
            let abs_pos = start + pos;
            // Text before match
            if abs_pos > start {
                new_spans.push(Span::styled(text[start..abs_pos].to_string(), span.style));
            }
            // Matched text
            new_spans.push(Span::styled(
                text[abs_pos..abs_pos + query.len()].to_string(),
                highlight_style,
            ));
            start = abs_pos + query.len();
        }

        if found {
            // Remainder after last match
            if start < text.len() {
                new_spans.push(Span::styled(text[start..].to_string(), span.style));
            }
        } else {
            new_spans.push(span);
        }
    }

    Line::from(new_spans)
}

// ============================================================
// Modal rendering
// ============================================================

/// Compute a centered rectangle with given width/height percentages.
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn draw_modal(frame: &mut Frame, app: &mut App) {
    match &mut app.modal {
        Some(Modal::ConnectionManager(m)) => draw_connection_manager_modal(frame, m),
        Some(Modal::Set(m)) => draw_set_modal(frame, m),
        Some(Modal::Search(m)) => draw_search_modal(frame, m),
        Some(Modal::MibInfo(m)) => draw_mib_info_modal(frame, m),
        None => {}
    }
}

fn draw_connection_manager_modal(
    frame: &mut Frame,
    modal: &mut crate::modal::ConnectionManagerModal,
) {
    // If edit view is active, render the connect form instead
    if let Some(ref edit) = modal.edit_view {
        let area = centered_rect(50, 60, frame.area());
        frame.render_widget(Clear, area);

        let title = if modal.editing_index.is_some() {
            " Edit Connection "
        } else {
            " New Connection "
        };
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let label_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let value_style = Style::default().fg(Color::White);
        let focused_style = Style::default().fg(Color::Yellow).bg(Color::DarkGray);
        let dim_style = Style::default().fg(Color::Gray);

        let visible = edit.visible_fields();
        let mut lines: Vec<Line> = Vec::new();

        for &field_idx in &visible {
            let field = &edit.fields[field_idx];
            let is_focused = field_idx == edit.focused_field;

            let value_display = if is_focused {
                format!("{}_", field.value)
            } else {
                field.value.clone()
            };

            let cycle_hint = if matches!(field.kind, crate::modal::FieldKind::Cycle(_)) {
                " [Up/Down to cycle]"
            } else {
                ""
            };

            lines.push(Line::from(vec![
                Span::styled(format!("  {:15}", field.label), label_style),
                Span::styled(
                    value_display,
                    if is_focused {
                        focused_style
                    } else {
                        value_style
                    },
                ),
                Span::styled(cycle_hint, dim_style),
            ]));
            lines.push(Line::from(""));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  [Tab] Next field  [Up/Down] Navigate/Cycle  [Enter] Save  [Esc] Back",
            dim_style,
        )));

        frame.render_widget(Paragraph::new(lines), inner);
        return;
    }

    // List view
    let area = centered_rect(55, 60, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Connection Manager ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let heading_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let name_style = Style::default().fg(Color::White);
    let host_style = Style::default().fg(Color::Gray);
    let version_style = Style::default().fg(Color::Yellow);
    let dim_style = Style::default().fg(Color::Gray);
    let selected_style = Style::default().fg(Color::Black).bg(Color::Cyan);

    let content_height = inner.height as usize;
    let footer_lines = 2;
    let header_lines = 2;
    let list_height = content_height.saturating_sub(header_lines + footer_lines);
    modal.viewport_height = list_height;

    let mut lines: Vec<Line> = Vec::new();

    if modal.connections.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No saved connections",
            heading_style,
        )));
        lines.push(Line::from(""));
    } else {
        lines.push(Line::from(Span::styled(
            format!("  {} saved connections", modal.connections.len()),
            heading_style,
        )));
        lines.push(Line::from(""));

        let col_width = (inner.width as usize).saturating_sub(4);

        for (i, entry) in modal
            .connections
            .iter()
            .enumerate()
            .skip(modal.scroll_offset)
            .take(list_height)
        {
            let is_selected = i == modal.selected;
            let prefix = if is_selected { "▸ " } else { "  " };
            let host_port = format!("{}:{}", entry.host, entry.port);
            let used =
                prefix.len() + entry.alias.len() + 2 + host_port.len() + 2 + entry.version.len();
            let padding = col_width.saturating_sub(used);

            if is_selected {
                let text = format!(
                    "{}{}{}{}  {}",
                    prefix,
                    entry.alias,
                    " ".repeat(padding + 2),
                    host_port,
                    entry.version,
                );
                lines.push(Line::from(Span::styled(text, selected_style)));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(prefix.to_string(), name_style),
                    Span::styled(entry.alias.clone(), name_style),
                    Span::styled(" ".repeat(padding + 2), name_style),
                    Span::styled(host_port, host_style),
                    Span::styled(format!("  {}", entry.version), version_style),
                ]));
            }
        }
    }

    // Fill remaining
    let used = lines.len();
    let target = content_height.saturating_sub(footer_lines);
    for _ in used..target {
        lines.push(Line::from(""));
    }

    // Footer hints
    let esc_hint = if modal.is_startup { "Quit" } else { "Close" };
    lines.push(Line::from(Span::styled(
        format!(
            "  [j/k] Navigate  [Enter] Connect  [n] New  [e] Edit  [d] Delete  [Esc] {}",
            esc_hint
        ),
        dim_style,
    )));
    lines.push(Line::from(""));

    lines.truncate(content_height);
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_set_modal(frame: &mut Frame, modal: &crate::modal::SetModal) {
    let area = centered_rect(60, 50, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" SNMP SET ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let label_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let value_style = Style::default().fg(Color::White);
    let focused_style = Style::default().fg(Color::Yellow).bg(Color::DarkGray);
    let dim_style = Style::default().fg(Color::Gray);

    let oid_display = if modal.is_scalar {
        format!("{} (will send as {}.0)", modal.oid, modal.oid)
    } else {
        modal.oid.clone()
    };

    let lines = vec![
        Line::from(vec![
            Span::styled("  Name:    ", label_style),
            Span::styled(modal.name.clone(), value_style),
        ]),
        Line::from(vec![
            Span::styled("  OID:     ", label_style),
            Span::styled(oid_display, value_style),
        ]),
        Line::from(vec![
            Span::styled("  Type:    ", label_style),
            Span::styled(modal.syntax_label.clone(), value_style),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Value:   ", label_style),
            Span::styled(format!("{}_", modal.value_input), focused_style),
        ]),
        Line::from(vec![
            Span::styled("           ", label_style),
            Span::styled(modal.value_hint.clone(), dim_style),
        ]),
        Line::from(""),
        Line::from(Span::styled("  [Enter] Send SET  [Esc] Cancel", dim_style)),
    ];

    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_search_modal(frame: &mut Frame, modal: &crate::modal::SearchModal) {
    let area = centered_rect(60, 70, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Search MIB Objects ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let label_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let value_style = Style::default().fg(Color::White);
    let focused_style = Style::default().fg(Color::Yellow).bg(Color::DarkGray);
    let selected_style = Style::default().fg(Color::Black).bg(Color::Cyan);
    let dim_style = Style::default().fg(Color::Gray);
    let oid_style = Style::default().fg(Color::Gray);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("  Search:  ", label_style),
            Span::styled(format!("{}_", modal.query), focused_style),
        ]),
        Line::from(""),
    ];

    if modal.results.is_empty() {
        if modal.query.is_empty() {
            lines.push(Line::from(Span::styled(
                "  Type to search MIB object names",
                dim_style,
            )));
        } else {
            lines.push(Line::from(Span::styled("  No matches found", dim_style)));
        }
    } else {
        lines.push(Line::from(Span::styled(
            format!("  {} match(es):", modal.results.len()),
            dim_style,
        )));
        lines.push(Line::from(""));

        for (i, result) in modal.results.iter().enumerate() {
            let is_selected = i == modal.selected;
            let style = if is_selected {
                selected_style
            } else {
                value_style
            };
            let prefix = if is_selected { "> " } else { "  " };

            lines.push(Line::from(vec![
                Span::styled(format!("{}{}", prefix, result.name), style),
                Span::styled(format!("  ({})", result.oid), oid_style),
            ]));
        }
    }

    // Hint at bottom
    let viewport_height = inner.height as usize;
    while lines.len() < viewport_height.saturating_sub(1) {
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        "  [Enter] Navigate to  [Up/Down] Select  [Esc] Cancel",
        dim_style,
    )));

    // Truncate to viewport
    lines.truncate(viewport_height);

    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_mib_info_modal(frame: &mut Frame, modal: &mut crate::modal::MibInfoModal) {
    if let Some(ref mut ov) = modal.object_view {
        draw_object_list_view(frame, ov);
        return;
    }

    let area = centered_rect(60, 70, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Loaded MIB Modules ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let heading_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let name_style = Style::default().fg(Color::White);
    let count_style = Style::default().fg(Color::Yellow);
    let file_style = Style::default().fg(Color::Gray);
    let dim_style = Style::default().fg(Color::Gray);
    let selected_style = Style::default().fg(Color::Black).bg(Color::Cyan);

    let content_height = inner.height as usize;
    let header_lines = 2;
    let search_lines = if modal.search_active { 1 } else { 0 };
    let footer_lines = 1 + search_lines;
    let list_height = content_height.saturating_sub(header_lines + footer_lines);
    modal.viewport_height = list_height;

    let mut lines = vec![
        Line::from(Span::styled(
            format!(
                "  {} modules, {} objects total",
                modal.filtered.len(),
                modal.total_objects
            ),
            heading_style,
        )),
        Line::from(""),
    ];

    let col_width = (inner.width as usize).saturating_sub(4);

    let visible: Vec<(usize, usize)> = modal
        .filtered
        .iter()
        .enumerate()
        .skip(modal.scroll_offset)
        .take(list_height)
        .map(|(vis_idx, &mod_idx)| (vis_idx, mod_idx))
        .collect();

    for (vis_idx, mod_idx) in &visible {
        let (ref name, count, ref file) = modal.modules[*mod_idx];
        let is_selected = *vis_idx == modal.selected;

        // Extract just the filename
        let filename = std::path::Path::new(file)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("");

        let count_str = format!("{}", count);
        let prefix = if is_selected { "▸ " } else { "  " };
        let used = prefix.len() + name.len() + 2 + filename.len() + 2 + count_str.len();
        let padding = col_width.saturating_sub(used);

        if is_selected {
            let text = format!(
                "{}{}{}{}  {}",
                prefix,
                name,
                " ".repeat(padding + 2),
                filename,
                count_str,
            );
            lines.push(Line::from(Span::styled(text, selected_style)));
        } else {
            lines.push(Line::from(vec![
                Span::styled(prefix, name_style),
                Span::styled(name.clone(), name_style),
                Span::styled(" ".repeat(padding + 2), name_style),
                Span::styled(filename.to_string(), file_style),
                Span::styled(format!("  {}", count_str), count_style),
            ]));
        }
    }

    // Fill remaining space
    let used = lines.len();
    let target = content_height.saturating_sub(footer_lines);
    for _ in used..target {
        lines.push(Line::from(""));
    }

    // Footer hint
    let hint = "  [j/k] Navigate  [Enter] View  [/] Search  [Esc] Close";
    lines.push(Line::from(Span::styled(hint, dim_style)));

    // Search bar
    if modal.search_active {
        let search_line = Line::from(vec![
            Span::styled("  /", Style::default().fg(Color::Yellow)),
            Span::styled(
                modal.search_query.clone(),
                Style::default().fg(Color::White),
            ),
            Span::styled("_", Style::default().fg(Color::Yellow)),
        ]);
        lines.push(search_line);
    }

    lines.truncate(content_height);
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_object_list_view(frame: &mut Frame, ov: &mut crate::modal::ObjectListView) {
    let area = centered_rect(60, 70, frame.area());
    frame.render_widget(Clear, area);

    let title = format!(" {} ({} objects) ", ov.module_name, ov.filtered.len());
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let name_style = Style::default().fg(Color::White);
    let oid_style = Style::default().fg(Color::Gray);
    let dim_style = Style::default().fg(Color::Gray);
    let selected_style = Style::default().fg(Color::Black).bg(Color::Cyan);

    let content_height = inner.height as usize;
    let search_lines = if ov.search_active { 1 } else { 0 };
    let footer_lines = 1 + search_lines;
    let list_height = content_height.saturating_sub(footer_lines);
    ov.viewport_height = list_height;

    let col_width = (inner.width as usize).saturating_sub(4);
    let mut lines: Vec<Line> = Vec::new();

    let visible: Vec<(usize, usize)> = ov
        .filtered
        .iter()
        .enumerate()
        .skip(ov.scroll_offset)
        .take(list_height)
        .map(|(vis_idx, &obj_idx)| (vis_idx, obj_idx))
        .collect();

    for (vis_idx, obj_idx) in &visible {
        let (ref name, ref oid) = ov.objects[*obj_idx];
        let is_selected = *vis_idx == ov.selected;

        let used = 2 + name.len() + 2 + oid.len();
        let padding = col_width.saturating_sub(used);

        if is_selected {
            let text = format!("  {}{}  {}", name, " ".repeat(padding), oid);
            lines.push(Line::from(Span::styled(text, selected_style)));
        } else {
            lines.push(Line::from(vec![
                Span::styled("  ", name_style),
                Span::styled(name.clone(), name_style),
                Span::styled(" ".repeat(padding + 2), name_style),
                Span::styled(oid.clone(), oid_style),
            ]));
        }
    }

    // Fill remaining
    let used = lines.len();
    let target = content_height.saturating_sub(footer_lines);
    for _ in used..target {
        lines.push(Line::from(""));
    }

    let hint = "  [j/k] Navigate  [/] Search  [Esc] Back";
    lines.push(Line::from(Span::styled(hint, dim_style)));

    if ov.search_active {
        let search_line = Line::from(vec![
            Span::styled("  /", Style::default().fg(Color::Yellow)),
            Span::styled(ov.search_query.clone(), Style::default().fg(Color::White)),
            Span::styled("_", Style::default().fg(Color::Yellow)),
        ]);
        lines.push(search_line);
    }

    lines.truncate(content_height);
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_help_overlay(frame: &mut Frame) {
    let area = centered_rect(60, 80, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Key Bindings ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let heading_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let key_style = Style::default().fg(Color::Yellow);
    let desc_style = Style::default().fg(Color::White);
    let dim_style = Style::default().fg(Color::Gray);

    let lines = vec![
        Line::from(Span::styled("  Global", heading_style)),
        help_line(
            "    Tab / Shift+Tab",
            "Switch panel focus",
            key_style,
            desc_style,
        ),
        help_line("    c", "Connection manager", key_style, desc_style),
        help_line("    m", "Loaded MIB modules", key_style, desc_style),
        help_line("    Ctrl+K", "Clear results", key_style, desc_style),
        help_line("    /", "Search MIB objects", key_style, desc_style),
        help_line("    ?", "Toggle this help", key_style, desc_style),
        help_line("    q", "Quit", key_style, desc_style),
        Line::from(""),
        Line::from(Span::styled("  MIB Tree Panel", heading_style)),
        help_line("    j/k or Up/Down", "Navigate tree", key_style, desc_style),
        help_line(
            "    Enter / l / Right",
            "Expand node",
            key_style,
            desc_style,
        ),
        help_line(
            "    h / Left",
            "Collapse / go to parent",
            key_style,
            desc_style,
        ),
        help_line("    gg", "Jump to top", key_style, desc_style),
        help_line("    G", "Jump to bottom", key_style, desc_style),
        help_line(
            "    r",
            "Reset tree to initial state",
            key_style,
            desc_style,
        ),
        help_line("    Space", "GET selected OID", key_style, desc_style),
        help_line("    n", "GETNEXT (advancing)", key_style, desc_style),
        help_line("    w", "WALK subtree", key_style, desc_style),
        help_line("    s", "SET value dialog", key_style, desc_style),
        help_line("    y", "Copy node name + OID", key_style, desc_style),
        Line::from(""),
        Line::from(Span::styled("  Detail Panel", heading_style)),
        help_line(
            "    j/k or Up/Down",
            "Scroll description",
            key_style,
            desc_style,
        ),
        help_line("    gg", "Jump to top", key_style, desc_style),
        help_line("    G", "Jump to bottom", key_style, desc_style),
        help_line("    /", "Search in detail", key_style, desc_style),
        help_line("    n / N", "Next / prev match", key_style, desc_style),
        help_line("    y", "Copy detail to clipboard", key_style, desc_style),
        Line::from(""),
        Line::from(Span::styled("  Results Panel", heading_style)),
        help_line(
            "    j/k or Up/Down",
            "Scroll results",
            key_style,
            desc_style,
        ),
        help_line("    gg", "Jump to top", key_style, desc_style),
        help_line("    G", "Jump to latest", key_style, desc_style),
        help_line("    /", "Search in results", key_style, desc_style),
        help_line("    n / N", "Next / prev match", key_style, desc_style),
        help_line(
            "    y",
            "Copy last result to clipboard",
            key_style,
            desc_style,
        ),
        Line::from(""),
        Line::from(Span::styled("  Press any key to close", dim_style)),
    ];

    let viewport_height = inner.height as usize;
    let visible: Vec<Line> = lines.into_iter().take(viewport_height).collect();
    frame.render_widget(Paragraph::new(visible), inner);
}

fn help_line<'a>(key: &'a str, desc: &'a str, key_style: Style, desc_style: Style) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("{:24}", key), key_style),
        Span::styled(desc, desc_style),
    ])
}
