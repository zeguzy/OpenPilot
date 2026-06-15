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
    let wrap_width = area.width.saturating_sub(GUTTER.len() as u16).max(10) as usize;
    let all_items: Vec<ListItem> = app
        .chat_items
        .iter()
        .flat_map(|item| render_chat_item(item, wrap_width))
        .collect();

    let total = all_items.len();
    let visible = area.height as usize;
    let max_offset = total.saturating_sub(visible);
    let offset = app.scroll_offset.min(max_offset);
    let start = max_offset.saturating_sub(offset);

    let items: Vec<ListItem> = if total <= visible {
        all_items
    } else {
        let end = (start + visible).min(total);
        all_items[start..end].to_vec()
    };

    let list = List::new(items);
    frame.render_widget(list, area);
}

fn render_chat_item(item: &ChatItem, wrap_width: usize) -> Vec<ListItem<'static>> {
    match item {
        ChatItem::UserMessage(msg) => {
            let lines: Vec<ListItem> = wrap_text(msg, wrap_width)
                .into_iter()
                .map(|line| {
                    ListItem::new(Line::from(vec![
                        Span::raw(GUTTER),
                        Span::styled(line, user_style()),
                    ]))
                })
                .collect();
            if lines.is_empty() {
                vec![ListItem::new(Line::from(GUTTER))]
            } else {
                lines
            }
        }
        ChatItem::AssistantText(msg) | ChatItem::StreamingAssistant(msg) => {
            let lines: Vec<ListItem> = wrap_text(msg, wrap_width)
                .into_iter()
                .map(|line| ListItem::new(Line::from(format!("{GUTTER}{line}"))))
                .collect();
            if lines.is_empty() {
                vec![ListItem::new(Line::from(GUTTER))]
            } else {
                lines
            }
        }
        ChatItem::TaskPanel {
            task_id,
            events,
            collapsed,
            ..
        } => render_task_panel(task_id, events, *collapsed, wrap_width),
        ChatItem::SystemMessage(msg) => wrap_text(msg, wrap_width)
            .into_iter()
            .map(|line| {
                ListItem::new(Line::from(vec![
                    Span::raw(GUTTER),
                    Span::styled(line, Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect(),
        ChatItem::Error(msg) => wrap_text(&format!("error: {msg}"), wrap_width)
            .into_iter()
            .map(|line| {
                ListItem::new(Line::from(vec![
                    Span::raw(GUTTER),
                    Span::styled(line, Style::default().fg(Color::Red)),
                ]))
            })
            .collect(),
    }
}

fn render_task_panel(
    task_id: &str,
    events: &[String],
    collapsed: bool,
    wrap_width: usize,
) -> Vec<ListItem<'static>> {
    if collapsed {
        let last = events
            .last()
            .map_or_else(|| "starting".to_string(), std::clone::Clone::clone);
        vec![ListItem::new(Line::from(vec![
            Span::raw(GUTTER),
            Span::styled(
                "+ ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("task-{task_id} "),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(last),
            Span::styled(
                format!("  [/expand {task_id}]"),
                Style::default().fg(Color::DarkGray),
            ),
        ]))]
    } else {
        let mut lines = vec![ListItem::new(Line::from(vec![
            Span::raw(GUTTER),
            Span::styled(
                "v ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("task-{task_id}"),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  [/collapse {task_id}]"),
                Style::default().fg(Color::DarkGray),
            ),
        ]))];
        for ev in events {
            for wrapped in wrap_text(ev, wrap_width.saturating_sub(4)) {
                lines.push(ListItem::new(Line::from(format!("{GUTTER}  | {wrapped}"))));
            }
        }
        lines
    }
}

/// Display width: 1 for ASCII, 2 for CJK/fullwidth (approximate, not unicode-width).
const fn char_display_width(c: char) -> usize {
    if c.is_ascii() { 1 } else { 2 }
}

fn text_display_width(s: &str) -> usize {
    s.chars().map(char_display_width).sum()
}

/// Word-wrap to `max_width` display columns. Long words are broken char-by-char.
fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return text.lines().map(String::from).collect();
    }

    let mut result = Vec::new();
    for line in text.lines() {
        if text_display_width(line) <= max_width {
            result.push(line.to_string());
        } else {
            wrap_single_line(line, max_width, &mut result);
        }
    }

    if result.is_empty() {
        vec![String::new()]
    } else {
        result
    }
}

fn wrap_single_line(line: &str, max_width: usize, out: &mut Vec<String>) {
    let mut current = String::new();
    let mut current_width = 0usize;

    for word in line.split(' ') {
        let word_width = text_display_width(word);
        if word_width > max_width {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            out.extend(break_long_word(word, max_width, &mut current));
            current_width = text_display_width(&current);
        } else if current.is_empty() {
            current = word.to_string();
            current_width = word_width;
        } else if current_width + 1 + word_width <= max_width {
            current.push(' ');
            current.push_str(word);
            current_width += 1 + word_width;
        } else {
            out.push(std::mem::take(&mut current));
            current = word.to_string();
            current_width = word_width;
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
}

/// Break a word wider than `max_width`. Full chunks returned; last partial in `tail`.
fn break_long_word(word: &str, max_width: usize, tail: &mut String) -> Vec<String> {
    let mut full = Vec::new();
    let mut chunk = String::new();
    let mut chunk_width = 0usize;
    for c in word.chars() {
        let cw = char_display_width(c);
        if chunk_width + cw > max_width {
            full.push(std::mem::take(&mut chunk));
            chunk_width = 0;
        }
        chunk.push(c);
        chunk_width += cw;
    }
    *tail = chunk;
    full
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
    let prefix_len = match &app.mode {
        AppMode::Orchestrator => GUTTER.len() + 2,
        AppMode::Task { task_id } => GUTTER.len() + task_id.len() + 7,
    };

    let input_text = input.input();
    let input_lines: Vec<&str> = input_text.split('\n').collect();
    let prompt_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    let mut lines = Vec::with_capacity(input_lines.len());
    for (i, text) in input_lines.iter().enumerate() {
        if i == 0 {
            lines.push(Line::from(vec![
                Span::styled(prompt.clone(), prompt_style),
                Span::raw((*text).to_string()),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::raw(" ".repeat(prefix_len)),
                Span::raw((*text).to_string()),
            ]));
        }
    }
    frame.render_widget(Paragraph::new(ratatui::text::Text::from(lines)), area);

    let (row, _col) = input.cursor_row_col();
    let text_before = input.current_line_before_cursor();
    let text_width = text_display_width(&text_before);
    let cursor_y = area.y + row as u16;
    let cursor_x = area.x + prefix_len as u16 + text_width as u16;
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
