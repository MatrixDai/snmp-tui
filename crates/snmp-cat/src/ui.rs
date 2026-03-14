use std::time::SystemTime;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::{App, ConnectionState, FocusedPanel, ResultValue};
use crate::modal::Modal;

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
    let block = Block::default()
        .title(" Query Results ")
        .borders(Borders::ALL)
        .border_style(style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let viewport_height = inner.height as usize;
    app.results_state.viewport_height = viewport_height;

    if app.results_state.entries.is_empty() {
        let msg = if matches!(app.connection, ConnectionState::Connected { .. }) {
            "Select an OID and press [Space] to GET, [n] GETNEXT, or [w] WALK."
        } else {
            "Press [o] to connect to an SNMP device."
        };
        let placeholder = Line::from(Span::styled(msg, Style::default().fg(Color::Gray)));
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
    let dim_style = Style::default().fg(Color::Gray);

    let mut lines = Vec::new();

    for (i, entry) in app.results_state.entries.iter().enumerate() {
        if i > 0 {
            lines.push(Line::from(Span::styled("─".repeat(40), dim_style)));
        }

        let time_str = format_timestamp(entry.timestamp);
        if entry.object_name.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(format!("[{}] ", time_str), dim_style),
                Span::styled(format!("{}", entry.operation), header_style),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(format!("[{}] ", time_str), dim_style),
                Span::styled(format!("{} ", entry.operation), header_style),
                Span::styled(entry.object_name.clone(), value_style),
            ]));
        }

        match &entry.result {
            ResultValue::Single(val) => {
                if entry.oid.is_empty() {
                    // No OID (e.g. CONNECT/DISCONNECT) — just show the message
                    lines.push(Line::from(vec![
                        Span::styled("  ", value_style),
                        Span::styled(val.clone(), value_style),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::styled("  ", value_style),
                        Span::styled(entry.oid.clone(), oid_style),
                        Span::styled(" = ", dim_style),
                        Span::styled(val.clone(), value_style),
                    ]));
                }
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
    // Show transient status message (e.g., "Copied to clipboard")
    if let Some((ref msg, _)) = app.status_message {
        let status = Line::from(Span::styled(
            format!(" {} ", msg),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
        frame.render_widget(Paragraph::new(status), area);
        return;
    }

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

    let is_connected = matches!(app.connection, ConnectionState::Connected { .. });

    let hints = if app.modal.is_some() {
        "[Esc] Cancel  [Tab] Next field  [Enter] Confirm/Cycle".to_string()
    } else {
        match app.focused {
            FocusedPanel::Tree => {
                if is_connected {
                    "[Tab] Switch  [j/k] Navigate  [Enter] Expand  [Space] GET  [n] GETNEXT  [w] WALK  [s] SET  [o] Connect  [/] Search  [?] Help  [q] Quit".to_string()
                } else {
                    "[Tab] Switch  [j/k] Navigate  [Enter] Expand  [o] Connect first to query  [/] Search  [?] Help  [q] Quit".to_string()
                }
            }
            FocusedPanel::Detail => {
                "[Tab] Switch  [j/k] Scroll  [o] Connect  [/] Search  [?] Help  [q] Quit".to_string()
            }
            FocusedPanel::Results => {
                "[Tab] Switch  [j/k] Scroll  [G] Latest  [y] Copy  [o] Connect  [/] Search  [?] Help  [q] Quit".to_string()
            }
        }
    };

    let status = Line::from(Span::styled(hints, Style::default().fg(Color::Gray)));
    frame.render_widget(Paragraph::new(status), area);
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

fn draw_modal(frame: &mut Frame, app: &App) {
    match &app.modal {
        Some(Modal::Connect(m)) => draw_connect_modal(frame, m),
        Some(Modal::Set(m)) => draw_set_modal(frame, m),
        Some(Modal::Search(m)) => draw_search_modal(frame, m),
        None => {}
    }
}

fn draw_connect_modal(frame: &mut Frame, modal: &crate::modal::ConnectModal) {
    let area = centered_rect(50, 60, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Connect to Device ")
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

    let visible = modal.visible_fields();
    let mut lines: Vec<Line> = Vec::new();

    for &field_idx in &visible {
        let field = &modal.fields[field_idx];
        let is_focused = field_idx == modal.focused_field;

        let value_display = if is_focused {
            format!("{}_", field.value)
        } else {
            field.value.clone()
        };

        let cycle_hint = if matches!(field.kind, crate::modal::FieldKind::Cycle(_)) {
            " [Enter to cycle]"
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

    // Add hint at bottom
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  [Tab] Next field  [Enter] Cycle/Confirm  [Esc] Cancel",
        dim_style,
    )));

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
        help_line("    o", "Open connect dialog", key_style, desc_style),
        help_line("    c", "Clear results", key_style, desc_style),
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
        help_line("    Space", "GET selected OID", key_style, desc_style),
        help_line("    n", "GETNEXT (advancing)", key_style, desc_style),
        help_line("    w", "WALK subtree", key_style, desc_style),
        help_line("    s", "SET value dialog", key_style, desc_style),
        Line::from(""),
        Line::from(Span::styled("  Detail Panel", heading_style)),
        help_line(
            "    j/k or Up/Down",
            "Scroll description",
            key_style,
            desc_style,
        ),
        Line::from(""),
        Line::from(Span::styled("  Results Panel", heading_style)),
        help_line(
            "    j/k or Up/Down",
            "Scroll results",
            key_style,
            desc_style,
        ),
        help_line("    G", "Jump to latest", key_style, desc_style),
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
