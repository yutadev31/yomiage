// Smaller chunks reduce time-to-first-audio on CPU-only VOICEVOX deployments.
const MIN_CHUNK_CHARS: usize = 10;
const TARGET_CHUNK_CHARS: usize = 25;
const MAX_CHUNK_CHARS: usize = 60;

/// Splits text into natural, bounded pieces while preserving the original characters.
pub fn split_text(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for sentence in split_at_natural_boundaries(text) {
        if char_count(&current) + char_count(&sentence) > MAX_CHUNK_CHARS
            && char_count(&current) >= MIN_CHUNK_CHARS
        {
            chunks.push(std::mem::take(&mut current));
        }

        current.push_str(&sentence);
        while char_count(&current) > TARGET_CHUNK_CHARS {
            let split_at = preferred_split_index(&current);
            chunks.push(take_chars(&mut current, split_at));
        }

        if ends_with_natural_boundary(&sentence) && char_count(&current) >= MIN_CHUNK_CHARS {
            chunks.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

fn split_at_natural_boundaries(text: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;

    for (index, character) in text.char_indices() {
        if matches!(character, '。' | '！' | '？' | '、' | '\n') {
            let end = index + character.len_utf8();
            segments.push(&text[start..end]);
            start = end;
        }
    }

    if start < text.len() {
        segments.push(&text[start..]);
    }
    segments
}

fn ends_with_natural_boundary(text: &str) -> bool {
    matches!(text.chars().last(), Some('。' | '！' | '？' | '、' | '\n'))
}

fn preferred_split_index(text: &str) -> usize {
    let characters: Vec<_> = text.chars().collect();
    let limit = TARGET_CHUNK_CHARS
        .min(MAX_CHUNK_CHARS)
        .min(characters.len());

    for index in (MIN_CHUNK_CHARS..=limit).rev() {
        if characters[index - 1] == '、' {
            return index;
        }
    }
    for index in (MIN_CHUNK_CHARS..=limit).rev() {
        if characters[index - 1].is_whitespace() {
            return index;
        }
    }
    for index in (MIN_CHUNK_CHARS..=limit).rev() {
        if !splits_protected_run(&characters, index) {
            return index;
        }
    }

    // A single URL or ASCII token can be longer than the maximum. Keep it intact
    // when possible, rather than splitting it in the middle.
    for index in limit + 1..characters.len() {
        if !splits_protected_run(&characters, index) {
            return index;
        }
    }
    characters.len()
}

fn splits_protected_run(characters: &[char], index: usize) -> bool {
    index > 0
        && index < characters.len()
        && is_protected_character(characters[index - 1])
        && is_protected_character(characters[index])
}

fn is_protected_character(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(
            character,
            ':' | '/' | '?' | '&' | '=' | '#' | '%' | '.' | '_' | '-' | '~'
        )
}

fn take_chars(text: &mut String, count: usize) -> String {
    let byte_index = text
        .char_indices()
        .nth(count)
        .map_or(text.len(), |(index, _)| index);
    let remaining = text.split_off(byte_index);
    std::mem::replace(text, remaining)
}

fn char_count(text: &str) -> usize {
    text.chars().count()
}

#[cfg(test)]
mod tests {
    use super::split_text;

    #[test]
    fn combines_short_sentences_until_the_minimum_length() {
        let chunks = split_text("短い文です。次も短い文です。最後です。");
        assert_eq!(chunks, vec!["短い文です。次も短い文です。", "最後です。"]);
    }

    #[test]
    fn splits_long_text_at_japanese_commas() {
        let text = format!(
            "{}、{}、{}。",
            "あ".repeat(45),
            "い".repeat(45),
            "う".repeat(45)
        );
        let chunks = split_text(&text);
        assert!(chunks.len() > 2);
        assert!(chunks.iter().any(|chunk| chunk.ends_with('、')));
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn does_not_split_inside_a_url() {
        let url = format!("https://example.com/{}", "a".repeat(110));
        let chunks = split_text(&format!("URLは{url}です。"));
        assert!(chunks.iter().any(|chunk| chunk.contains(&url)));
    }

    #[test]
    fn prioritizes_low_latency_at_commas_and_sentence_boundaries() {
        let chunks = split_text(
            "今日はいい天気ですね、でも明日は雨が降るそうです。傘を持っていったほうがいいかもしれません。",
        );
        assert_eq!(
            chunks,
            vec![
                "今日はいい天気ですね、",
                "でも明日は雨が降るそうです。",
                "傘を持っていったほうがいいかもしれません。",
            ]
        );
    }
}
