pub mod parser;

pub use parser::parse;

/// A single-line text input with history, used for both `:` and `/` prompts.
#[derive(Debug, Default)]
pub struct LineEditor {
    buffer: String,
    /// Cursor position in chars (not bytes).
    cursor: usize,
    history: Vec<String>,
    /// Position while browsing history; `None` means editing a fresh line.
    history_pos: Option<usize>,
}

impl LineEditor {
    pub fn text(&self) -> &str {
        &self.buffer
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn insert(&mut self, c: char) {
        let at = self.byte_index(self.cursor);
        self.buffer.insert(at, c);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            let at = self.byte_index(self.cursor);
            self.buffer.remove(at);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.len() {
            let at = self.byte_index(self.cursor);
            self.buffer.remove(at);
        }
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.len());
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.len();
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
        self.history_pos = None;
    }

    pub fn set(&mut self, text: String) {
        self.cursor = text.chars().count();
        self.buffer = text;
    }

    /// Takes the current line without recording it (for secrets).
    pub fn take(&mut self) -> String {
        self.cursor = 0;
        self.history_pos = None;
        std::mem::take(&mut self.buffer)
    }

    /// Takes the current line, recording it in history.
    pub fn submit(&mut self) -> String {
        let line = std::mem::take(&mut self.buffer);
        self.cursor = 0;
        self.history_pos = None;
        if !line.trim().is_empty() && self.history.last() != Some(&line) {
            self.history.push(line.clone());
        }
        line
    }

    pub fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let pos = match self.history_pos {
            None => self.history.len() - 1,
            Some(p) => p.saturating_sub(1),
        };
        self.history_pos = Some(pos);
        self.set(self.history[pos].clone());
    }

    pub fn history_next(&mut self) {
        match self.history_pos {
            Some(p) if p + 1 < self.history.len() => {
                self.history_pos = Some(p + 1);
                self.set(self.history[p + 1].clone());
            }
            Some(_) => {
                self.history_pos = None;
                self.set(String::new());
            }
            None => {}
        }
    }

    fn len(&self) -> usize {
        self.buffer.chars().count()
    }

    fn byte_index(&self, char_idx: usize) -> usize {
        self.buffer
            .char_indices()
            .nth(char_idx)
            .map_or(self.buffer.len(), |(i, _)| i)
    }
}

/// Result of pressing Tab in command mode.
#[derive(Debug, PartialEq, Eq)]
pub enum Completion {
    /// Replace the line with this text.
    Replace(String),
    /// Several commands match; show them.
    Candidates(Vec<&'static str>),
    None,
}

/// Completes the command name (first word) of `line`.
pub fn complete(line: &str) -> Completion {
    if line.contains(char::is_whitespace) {
        return Completion::None;
    }
    let matches: Vec<&'static str> = parser::COMMANDS
        .iter()
        .map(|c| c.name)
        .filter(|name| name.starts_with(line))
        .collect();
    match matches.as_slice() {
        [] => Completion::None,
        [only] => Completion::Replace(format!("{only} ")),
        [first, rest @ ..] => {
            let common = rest.iter().fold(first.len(), |n, m| {
                first
                    .bytes()
                    .zip(m.bytes())
                    .take(n)
                    .take_while(|(a, b)| a == b)
                    .count()
            });
            if common > line.len() {
                Completion::Replace(first[..common].to_string())
            } else {
                Completion::Candidates(matches)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_unique_prefix() {
        assert_eq!(complete("shu"), Completion::Replace("shuffle ".into()));
    }

    #[test]
    fn complete_common_prefix_then_candidates() {
        assert_eq!(complete("q"), Completion::Replace("qu".into()));
        assert_eq!(
            complete("se"),
            Completion::Candidates(vec!["search", "seek", "select", "settings"])
        );
    }

    #[test]
    fn complete_ignores_arguments_and_unknowns() {
        assert_eq!(complete("play foo"), Completion::None);
        assert_eq!(complete("xyz"), Completion::None);
    }

    #[test]
    fn editor_handles_multibyte_chars() {
        let mut e = LineEditor::default();
        for c in "héllo".chars() {
            e.insert(c);
        }
        e.left();
        e.left();
        e.left();
        e.backspace();
        assert_eq!(e.text(), "hllo");
        e.insert('ë');
        assert_eq!(e.text(), "hëllo");
    }

    #[test]
    fn editor_history_navigation() {
        let mut e = LineEditor::default();
        e.set("next".into());
        e.submit();
        e.set("vol 50".into());
        e.submit();
        e.history_prev();
        assert_eq!(e.text(), "vol 50");
        e.history_prev();
        assert_eq!(e.text(), "next");
        e.history_next();
        assert_eq!(e.text(), "vol 50");
        e.history_next();
        assert_eq!(e.text(), "");
    }
}
