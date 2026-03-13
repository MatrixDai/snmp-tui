use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, FocusedPanel};

/// Render the entire application UI.
pub fn draw(frame: &mut Frame, app: &App) {
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

fn draw_main_area(frame: &mut Frame, area: Rect, app: &App) {
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

fn draw_tree_panel(frame: &mut Frame, area: Rect, app: &App) {
    let style = panel_border_style(app.focused, FocusedPanel::Tree);
    let block = Block::default()
        .title(" MIB Tree ")
        .borders(Borders::ALL)
        .border_style(style);

    let node_count = app.oid_tree.len();
    let content = format!(
        "MIB tree loaded ({} nodes)\n\nUse j/k to navigate\nEnter to expand/collapse",
        node_count
    );
    let paragraph = Paragraph::new(content)
        .block(block)
        .style(Style::default().fg(Color::White));

    frame.render_widget(paragraph, area);
}

fn draw_detail_panel(frame: &mut Frame, area: Rect, app: &App) {
    let style = panel_border_style(app.focused, FocusedPanel::Detail);
    let block = Block::default()
        .title(" Object Detail ")
        .borders(Borders::ALL)
        .border_style(style);

    let content = "Select a node in the MIB tree\nto view its details here.";
    let paragraph = Paragraph::new(content)
        .block(block)
        .style(Style::default().fg(Color::DarkGray));

    frame.render_widget(paragraph, area);
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
        FocusedPanel::Tree => "[Tab] Switch  [j/k] Navigate  [Enter] Expand  [q] Quit",
        FocusedPanel::Detail => "[Tab] Switch  [j/k] Scroll  [q] Quit",
        FocusedPanel::Results => "[Tab] Switch  [j/k] Scroll  [G] Latest  [q] Quit",
    };

    let status = Line::from(Span::styled(hints, Style::default().fg(Color::DarkGray)));
    frame.render_widget(Paragraph::new(status), area);
}
