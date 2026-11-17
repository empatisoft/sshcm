//! A one-line text field.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextInput {
    value: String,
    /// Cursor position in characters, `0..=len`.
    cursor: usize,
}

impl TextInput {
    pub fn new(value: &str) -> TextInput {
        TextInput {
            value: value.to_string(),
            cursor: value.chars().count(),
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    /// The value without surrounding whitespace.
    pub fn text(&self) -> &str {
        self.value.trim()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn is_empty(&self) -> bool {
        self.value.trim().is_empty()
    }

    pub fn set(&mut self, value: &str) {
        *self = TextInput::new(value);
    }

    fn byte_index(&self, chars: usize) -> usize {
        self.value
            .char_indices()
            .nth(chars)
            .map_or(self.value.len(), |(i, _)| i)
    }

    /// Applies an editing key. Returns false when the key is not one a text
    /// field handles, so the caller can use it for something else.
    pub fn handle(&mut self, key: KeyEvent) -> bool {
        let len = self.value.chars().count();
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('a') if ctrl => self.cursor = 0,
            KeyCode::Char('e') if ctrl => self.cursor = len,
            KeyCode::Char('u') if ctrl => {
                let end = self.byte_index(self.cursor);
                self.value.replace_range(..end, "");
                self.cursor = 0;
            }
            KeyCode::Char(c) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => {
                let at = self.byte_index(self.cursor);
                self.value.insert(at, c);
                self.cursor += 1;
            }
            KeyCode::Backspace if self.cursor > 0 => {
                let start = self.byte_index(self.cursor - 1);
                let end = self.byte_index(self.cursor);
                self.value.replace_range(start..end, "");
                self.cursor -= 1;
            }
            KeyCode::Backspace => {}
            KeyCode::Delete if self.cursor < len => {
                let start = self.byte_index(self.cursor);
                let end = self.byte_index(self.cursor + 1);
                self.value.replace_range(start..end, "");
            }
            KeyCode::Delete => {}
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(len),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = len,
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn type_text(input: &mut TextInput, text: &str) {
        for c in text.chars() {
            input.handle(key(KeyCode::Char(c)));
        }
    }

    #[test]
    fn typing_and_deleting_work_with_multibyte_text() {
        let mut input = TextInput::default();
        type_text(&mut input, "üretim");
        assert_eq!(input.value(), "üretim");
        input.handle(key(KeyCode::Home));
        input.handle(key(KeyCode::Delete));
        assert_eq!(input.value(), "retim");
        input.handle(key(KeyCode::End));
        input.handle(key(KeyCode::Backspace));
        input.handle(key(KeyCode::Left));
        type_text(&mut input, "ş");
        assert_eq!(input.value(), "retşi");
        assert_eq!(input.cursor(), 4);
    }

    #[test]
    fn edge_keys_do_nothing_at_the_ends() {
        let mut input = TextInput::new("ab");
        input.handle(key(KeyCode::Delete));
        input.handle(key(KeyCode::Right));
        assert_eq!((input.value(), input.cursor()), ("ab", 2));
        input.handle(key(KeyCode::Home));
        input.handle(key(KeyCode::Backspace));
        input.handle(key(KeyCode::Left));
        assert_eq!((input.value(), input.cursor()), ("ab", 0));
    }

    #[test]
    fn control_shortcuts() {
        let mut input = TextInput::new("hello world");
        input.handle(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert_eq!(input.cursor(), 0);
        input.handle(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
        assert_eq!(input.cursor(), 11);
        for _ in 0..5 {
            input.handle(key(KeyCode::Left));
        }
        input.handle(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!((input.value(), input.cursor()), ("world", 0));
        // Unhandled keys are reported back.
        assert!(!input.handle(key(KeyCode::Tab)));
        assert!(!input.handle(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)));
    }
}
