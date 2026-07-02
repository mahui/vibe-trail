//! Display-text helpers shared by providers. Generic string shaping only —
//! no provider format knowledge belongs here.

/// Collapse to one line and cap at 80 chars for list display.
pub(crate) fn sanitize_title(text: &str) -> String {
    let collapsed = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let collapsed = collapsed.trim();
    if collapsed.chars().count() > 80 {
        let mut truncated: String = collapsed.chars().take(79).collect();
        truncated.push('…');
        truncated
    } else {
        collapsed.to_string()
    }
}

/// Case-insensitive query match over candidate texts → one-line ±context
/// snippet, or None when the match only lived in structural metadata.
pub(crate) fn make_snippet(texts: &[String], query: &str) -> Option<String> {
    let query_lower = query.to_lowercase();
    for text in texts {
        let lower = text.to_lowercase();
        let Some(byte_start) = lower.find(&query_lower) else {
            continue;
        };
        // Counting chars on `lower` keeps the slice on a valid boundary even
        // when case-folding changed byte lengths.
        let chars: Vec<char> = text.chars().collect();
        let char_start = lower[..byte_start].chars().count().min(chars.len());
        let match_chars = query.chars().count();
        let from = char_start.saturating_sub(60);
        let to = (char_start + match_chars + 100).min(chars.len());
        let mut snippet: String = chars[from..to].iter().collect();
        snippet = snippet.split_whitespace().collect::<Vec<_>>().join(" ");
        let prefix = if from > 0 { "…" } else { "" };
        let suffix = if to < chars.len() { "…" } else { "" };
        return Some(format!("{prefix}{snippet}{suffix}"));
    }
    None
}
