use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, FocusedPanel};

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

    draw_title_bar(frame, outer[0]);
    draw_main_area(frame, outer[1], app);
    draw_status_bar(frame, outer[2], app);
}

fn draw_title_bar(frame: &mut Frame, area: Rect) {
    let title = Line::from(vec![
        Span::styled(
            "snmp-cat",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled("[No device]", Style::default().fg(Color::DarkGray)),
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

    // Viewport height = inner area height
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

        // Build the line: indent + prefix + name
        let indent = "  ".repeat(depth);
        let prefix = if has_children {
            if is_expanded { "▾ " } else { "▸ " }
        } else {
            "  "
        };

        // Branch nodes: name(subid), leaf nodes: just name
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

fn draw_detail_panel(frame: &mut Frame, area: Rect, app: &App) {
    let style = panel_border_style(app.focused, FocusedPanel::Detail);
    let block = Block::default()
        .title(" Object Detail ")
        .borders(Borders::ALL)
        .border_style(style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = build_detail_lines(app);
    frame.render_widget(Paragraph::new(lines), inner);
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

        if let Some(ref desc) = mib_obj.description {
            lines.push(Line::from(Span::raw("")));
            // Strip quotes from description if present
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

fn draw_results_panel(frame: &mut Frame, area: Rect, app: &App) {
    let style = panel_border_style(app.focused, FocusedPanel::Results);
    let block = Block::default()
        .title(" Query Results ")
        .borders(Borders::ALL)
        .border_style(style);

    let content = "SNMP query results will\nappear here.";
    let paragraph = Paragraph::new(content)
        .block(block)
        .style(Style::default().fg(Color::DarkGray));

    frame.render_widget(paragraph, area);
}

fn draw_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    let hints = match app.focused {
        FocusedPanel::Tree => {
            "[Tab] Switch  [j/k] Navigate  [Enter] Expand  [h/l] Collapse/Expand  [gg/G] Top/Bottom  [q] Quit"
        }
        FocusedPanel::Detail => "[Tab] Switch  [j/k] Scroll  [q] Quit",
        FocusedPanel::Results => "[Tab] Switch  [j/k] Scroll  [G] Latest  [q] Quit",
    };

    let status = Line::from(Span::styled(hints, Style::default().fg(Color::DarkGray)));
    frame.render_widget(Paragraph::new(status), area);
}
