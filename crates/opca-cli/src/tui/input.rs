use tui_textarea::TextArea;

pub struct InputArea {
    pub textarea: TextArea<'static>,
}

impl InputArea {
    #[must_use]
    pub fn new() -> Self {
        let mut ta = TextArea::default();
        ta.set_placeholder_text("Type a message... (Enter=send, Shift+Enter=newline)");
        Self { textarea: ta }
    }

    pub fn input(&self) -> String {
        self.textarea.lines().join("\n")
    }

    pub fn clear(&mut self) {
        self.textarea = TextArea::default();
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.textarea
            .lines()
            .iter()
            .all(std::string::String::is_empty)
    }

    #[must_use]
    pub fn cursor(&self) -> usize {
        let (row, col) = self.textarea.cursor();
        let lines = self.textarea.lines();
        let mut pos = col;
        for (i, line) in lines.iter().enumerate() {
            if i < row {
                pos += line.chars().count() + 1;
            }
        }
        pos
    }

    #[must_use]
    pub fn cursor_row_col(&self) -> (usize, usize) {
        let (row, col) = self.textarea.cursor();
        (row, col)
    }

    #[must_use]
    pub fn current_line_before_cursor(&self) -> String {
        let (row, col) = self.textarea.cursor();
        let lines = self.textarea.lines();
        if let Some(line) = lines.get(row) {
            line.chars().take(col).collect()
        } else {
            String::new()
        }
    }
}

impl Default for InputArea {
    fn default() -> Self {
        Self::new()
    }
}
