//! Pure greedy text wrapping. Widths come from a caller-supplied `measure` callback, so this
//! module never touches fonts. All operations are on `char`s, never bytes (non-ASCII safe).

/// ASCII ellipsis appended when a line is truncated.
const ELLIPSIS: &str = "...";

/// Greedy word wrap of `text` to `max_width`, at most `max_lines` lines.
///
/// Wraps on spaces; an explicit `'\n'` in the text forces a line break; a word wider than
/// `max_width` is hard-broken by characters. When the text needs more than `max_lines` lines,
/// the last returned line is truncated and ends with `"..."` so that it still fits `max_width`
/// (characters are dropped until it fits). Empty text returns an empty vec.
pub fn wrap_text(
    text: &str,
    max_width: f32,
    max_lines: usize,
    measure: &dyn Fn(&str) -> f32,
) -> Vec<String> {
    if text.is_empty() || max_lines == 0 {
        return Vec::new();
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut first_para = true;
    for para in text.split('\n') {
        if !first_para {
            lines.push(std::mem::take(&mut current));
        }
        first_para = false;
        for word in para.split(' ').filter(|w| !w.is_empty()) {
            if measure(word) > max_width {
                // A word wider than the line is hard-broken by characters.
                if !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                }
                let chars: Vec<char> = word.chars().collect();
                let mut start = 0;
                while start < chars.len() {
                    let mut end = chars.len();
                    while end > start
                        && measure(&chars[start..end].iter().collect::<String>()) > max_width
                    {
                        end -= 1;
                    }
                    if end == start {
                        end = start + 1; // a single char wider than max_width is emitted anyway
                    }
                    let chunk: String = chars[start..end].iter().collect();
                    if end < chars.len() {
                        lines.push(chunk);
                    } else {
                        current = chunk;
                    }
                    start = end;
                }
            } else if current.is_empty() {
                current = word.to_owned();
            } else {
                let candidate = format!("{current} {word}");
                if measure(&candidate) <= max_width {
                    current = candidate;
                } else {
                    lines.push(std::mem::take(&mut current));
                    current = word.to_owned();
                }
            }
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            *last = truncate_with_ellipsis(last, max_width, measure);
        }
    }
    lines
}

/// A single line truncated with `"..."` so that it fits `max_width`. Text that already fits
/// is returned unchanged.
pub fn fit_line(text: &str, max_width: f32, measure: &dyn Fn(&str) -> f32) -> String {
    if measure(text) <= max_width {
        return text.to_owned();
    }
    truncate_with_ellipsis(text, max_width, measure)
}

/// Drops trailing `char`s from `text` until `text + "..."` fits `max_width`, then appends
/// `"..."`. If even the bare ellipsis does not fit, it is returned alone.
fn truncate_with_ellipsis(text: &str, max_width: f32, measure: &dyn Fn(&str) -> f32) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut keep = chars.len();
    while keep > 0 {
        let mut candidate: String = chars[..keep].iter().collect();
        candidate.push_str(ELLIPSIS);
        if measure(&candidate) <= max_width {
            return candidate;
        }
        keep -= 1;
    }
    ELLIPSIS.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic measure: 10 px per char (char-based, so non-ASCII counts as one char).
    fn measure(s: &str) -> f32 {
        s.chars().count() as f32 * 10.0
    }

    #[test]
    fn wraps_greedily_on_width() {
        // "aa bb" is exactly 50 wide and fits; adding " cc" would be 80.
        assert_eq!(
            wrap_text("aa bb cc", 50.0, 4, &measure),
            vec!["aa bb".to_owned(), "cc".to_owned()]
        );
        assert_eq!(
            wrap_text("aaaa bbbb cc", 50.0, 4, &measure),
            vec!["aaaa".to_owned(), "bbbb".to_owned(), "cc".to_owned()]
        );
    }

    #[test]
    fn explicit_newline_forces_a_break() {
        assert_eq!(
            wrap_text("aa\nbb", 100.0, 4, &measure),
            vec!["aa".to_owned(), "bb".to_owned()]
        );
        // An empty paragraph between newlines yields an empty line.
        assert_eq!(
            wrap_text("aa\n\nbb", 100.0, 4, &measure),
            vec!["aa".to_owned(), String::new(), "bb".to_owned()]
        );
    }

    #[test]
    fn long_word_is_hard_broken_by_characters() {
        // 6 chars = 60 wide > 50: chunks of 5 then 1.
        assert_eq!(
            wrap_text("aaaaaa", 50.0, 4, &measure),
            vec!["aaaaa".to_owned(), "a".to_owned()]
        );
        // A long word flushes the current line first.
        assert_eq!(
            wrap_text("bb aaaaaa", 50.0, 4, &measure),
            vec!["bb".to_owned(), "aaaaa".to_owned(), "a".to_owned()]
        );
    }

    #[test]
    fn overflow_truncates_last_line_with_ellipsis_that_fits() {
        // Three lines needed, max_lines = 2: last line "cc dd" (50 wide) + "..." must shrink
        // to fit 50 -> "cc..." (5 chars = 50).
        assert_eq!(
            wrap_text("aa bb cc dd ee", 50.0, 2, &measure),
            vec!["aa bb".to_owned(), "cc...".to_owned()]
        );
        // The ellipsis line still fits max_width.
        let lines = wrap_text("aa bb cc dd ee ff gg", 50.0, 2, &measure);
        assert_eq!(lines.len(), 2);
        assert!(lines[1].ends_with("..."));
        assert!(measure(&lines[1]) <= 50.0);
    }

    #[test]
    fn non_ascii_is_char_safe() {
        // No panic on multi-byte chars; each char counts once.
        assert_eq!(
            wrap_text("héllö wörld", 60.0, 4, &measure),
            vec!["héllö".to_owned(), "wörld".to_owned()]
        );
        assert_eq!(
            wrap_text("ääääää", 50.0, 4, &measure),
            vec!["äääää".to_owned(), "ä".to_owned()]
        );
        let lines = wrap_text("héllo wörld täil", 50.0, 2, &measure);
        assert!(lines[1].ends_with("..."));
        assert!(measure(&lines[1]) <= 50.0);
    }

    #[test]
    fn empty_text_is_empty_vec() {
        assert!(wrap_text("", 100.0, 4, &measure).is_empty());
        assert!(wrap_text("anything", 100.0, 0, &measure).is_empty());
    }

    #[test]
    fn fit_line_truncates_with_ellipsis_only_when_needed() {
        assert_eq!(fit_line("hello", 100.0, &measure), "hello");
        // "hello world" = 110 > 50; keep dropping until "..." fits: "he..." = 50.
        assert_eq!(fit_line("hello world", 50.0, &measure), "he...");
        assert!(measure(&fit_line("hello world", 50.0, &measure)) <= 50.0);
    }
}
