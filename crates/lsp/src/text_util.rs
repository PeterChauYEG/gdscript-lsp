/// Extract the identifier word that contains the given (line, character) position.
///
/// Returns `None` if the position is out of bounds or not on a word character.
#[must_use]
pub fn word_at(source: &str, line: u32, character: u32) -> Option<&str> {
    let target_line = line as usize;
    let target_col = character as usize;

    let text_line = source.lines().nth(target_line)?;
    let bytes = text_line.as_bytes();

    if target_col > bytes.len() {
        return None;
    }

    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';

    // The cursor may sit just past the last character of the word, so check col-1 too.
    let anchor = if target_col < bytes.len() && is_word(bytes[target_col]) {
        target_col
    } else if target_col > 0 && is_word(bytes[target_col - 1]) {
        target_col - 1
    } else {
        return None;
    };

    let start = (0..=anchor)
        .rev()
        .find(|&i| !is_word(bytes[i]))
        .map_or(0, |i| i + 1);
    let end = (anchor..bytes.len())
        .find(|&i| !is_word(bytes[i]))
        .unwrap_or(bytes.len());

    if start >= end {
        return None;
    }

    Some(&text_line[start..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_word_in_middle() {
        assert_eq!(word_at("extends Node2D", 0, 10), Some("Node2D"));
    }

    #[test]
    fn extracts_word_at_start() {
        assert_eq!(word_at("Node2D", 0, 0), Some("Node2D"));
    }

    #[test]
    fn cursor_just_past_word() {
        // "Node2D " — cursor at index 6 (the space)
        assert_eq!(word_at("Node2D ", 0, 6), Some("Node2D"));
    }

    #[test]
    fn returns_left_word_when_cursor_on_space_after_word() {
        // "a b" — cursor on the space at index 1; picks "a" (word to the left).
        // This is the expected behaviour for completions where the cursor trails the word.
        assert_eq!(word_at("a b", 0, 1), Some("a"));
    }

    #[test]
    fn no_word_on_leading_space() {
        assert_eq!(word_at("  foo", 0, 0), None);
    }
}
