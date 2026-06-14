use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, Paragraph};

use super::app::{App, AppMode, ChatItem};
use super::input::InputArea;

const GUTTER: &str = "  ";

pub fn render(frame: &mut Frame, app: &App, input: &InputArea) {
    let area = frame.area();

    let has_queue = !app.pending_messages.is_empty();
    let bottom_height = match (app.is_working, has_queue) {
        (true, true) => 5,
        (true, false) => 4,
        (false, true) => 3,
        (false, false) => 2,
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(bottom_height)])
        .split(area);

    render_chat(frame, app, chunks[0]);
    render_bottom(frame, app, input, chunks[1]);
}

fn render_chat(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .chat_items
        .iter()
        .flat_map(|item| render_chat_item(item))
        .collect();

    let list = List::new(items);
    frame.render_widget(list, area);
}

fn render_chat_item(item: &ChatItem) -> Vec<ListItem<'static>> {
    match item {
        ChatItem::UserMessage(msg) => {
            let lines: Vec<ListItem> = msg
                .lines()
                .map(|line| {
                    ListItem::new(Line::from(vec![
                        Span::raw(GUTTER),
                        Span::styled(line.to_string(), user_style()),
                    ]))
                })
                .collect();
            if lines.is_empty() {
                vec![ListItem::new(Line::from(format!("{GUTTER}{msg}")))]
            } else {
                lines
            }
        }
        ChatItem::AssistantText(msg) | ChatItem::StreamingAssistant(msg) => {
            let lines: Vec<ListItem> = msg
                .lines()
                .map(|line| ListItem::new(Line::from(format!("{GUTTER}{line}"))))
                .collect();
            if lines.is_empty() {
                vec![ListItem::new(Line::from(format!("{GUTTER}{msg}")))]
            } else {
                lines
            }
        }
        ChatItem::TaskPanel {
            task_id,
            events,
            collapsed,
            ..
        } => {
            if *collapsed {
                let last = events
                    .last()
                    .map_or_else(|| "starting".to_string(), std::clone::Clone::clone);
                let tid = task_id.clone();
                vec![ListItem::new(Line::from(vec![
                    Span::raw(GUTTER),
                    Span::styled(
                        "+ ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("task-{tid} "),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(last),
                    Span::styled(
                        format!("  [/expand {tid}]"),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))]
            } else {
                let tid = task_id.clone();
                let mut lines = vec![ListItem::new(Line::from(vec![
                    Span::raw(GUTTER),
                    Span::styled(
                        "v ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("task-{tid}"),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("  [/collapse {tid}]"),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))];
                for ev in events {
                    lines.push(ListItem::new(Line::from(format!("{GUTTER}  | {ev}"))));
                }
                lines
            }
        }
        ChatItem::SystemMessage(msg) => msg
            .lines()
            .map(|line| {
                ListItem::new(Line::from(vec![
                    Span::raw(GUTTER),
                    Span::styled(line.to_string(), Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect(),
        ChatItem::Error(msg) => vec![ListItem::new(Line::from(vec![
            Span::raw(GUTTER),
            Span::styled(format!("error: {msg}"), Style::default().fg(Color::Red)),
        ]))],
    }
}

fn render_bottom(frame: &mut Frame, app: &App, input: &InputArea, area: Rect) {
    let has_queue = !app.pending_messages.is_empty();
    let sections: Vec<Constraint> = {
        let mut v = Vec::new();
        if has_queue {
            v.push(Constraint::Length(1));
        }
        if app.is_working {
            v.push(Constraint::Length(2));
        }
        v.push(Constraint::Length(2));
        v
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(sections)
        .split(area);

    let mut idx = 0;
    if has_queue {
        render_queue(frame, app, chunks[idx]);
        idx += 1;
    }
    if app.is_working {
        render_working_status(frame, app, chunks[idx]);
        idx += 1;
    }
    render_input(frame, app, input, chunks[idx]);
}

fn render_queue(frame: &mut Frame, app: &App, area: Rect) {
    let count = app.pending_messages.len();
    let preview = app
        .pending_messages
        .front()
        .map_or(String::new(), |s| s.chars().take(40).collect());
    let line = Line::from(vec![
        Span::raw(GUTTER),
        Span::styled(
            format!("{count} queued",),
            Style::default().fg(Color::Yellow),
        ),
        if preview.is_empty() {
            Span::raw("")
        } else {
            Span::styled(
                format!(" - next: {preview}"),
                Style::default().fg(Color::DarkGray),
            )
        },
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_working_status(frame: &mut Frame, app: &App, area: Rect) {
    let elapsed = app.elapsed_secs();
    let mode_label = match &app.mode {
        AppMode::Orchestrator => "Thinking".to_string(),
        AppMode::Task { task_id } => format!("Task {task_id}"),
    };

    let line = Line::from(vec![
        Span::raw(GUTTER),
        Span::styled(
            format!("{} ", app.spinner_char()),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(mode_label, Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" "),
        Span::styled(
            format!("({elapsed}s - esc to interrupt)"),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_input(frame: &mut Frame, app: &App, input: &InputArea, area: Rect) {
    let prompt = match &app.mode {
        AppMode::Orchestrator => format!("{GUTTER}> "),
        AppMode::Task { task_id } => format!("{GUTTER}task-{task_id}> "),
    };

    let input_text = input.input();
    let line = Line::from(vec![
        Span::styled(
            prompt,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(input_text),
        Span::styled("", Style::default()),
    ]);
    frame.render_widget(Paragraph::new(line), area);

    let prefix_len = match &app.mode {
        AppMode::Orchestrator => GUTTER.len() + 2,
        AppMode::Task { task_id } => GUTTER.len() + task_id.len() + 7,
    };
    let cursor_y = area.y;
    let cursor_x = area.x + prefix_len as u16 + input.cursor() as u16;
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn user_style() -> Style {
    Style::default().fg(Color::Gray)
}

pub fn render_footer_info(app: &App) -> String {
    format!(
        "{} - up:{} down:{} - ${:.4}",
        app.model_name,
        app.prompt_tokens,
        app.completion_tokens,
        app.estimated_cost()
    )
}
