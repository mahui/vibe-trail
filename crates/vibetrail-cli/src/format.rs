use chrono::{DateTime, Utc};
use vibetrail_core::Role;

pub fn relative_time(date: &DateTime<Utc>) -> String {
    let seconds = (Utc::now() - *date).num_seconds();
    match seconds {
        s if s < 60 => "just now".to_string(),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3600),
        s if s < 86_400 * 30 => format!("{}d ago", s / 86_400),
        _ => date.format("%Y-%m-%d").to_string(),
    }
}

pub fn duration(seconds: f64) -> String {
    match seconds {
        s if s < 1.0 => "-".to_string(),
        s if s < 60.0 => format!("{}s", s as u64),
        s if s < 3600.0 => format!("{}m", (s / 60.0) as u64),
        s => format!("{:.1}h", s / 3600.0),
    }
}

pub fn abbreviate_path(path: &str) -> String {
    match dirs::home_dir() {
        Some(home) => {
            let home = home.to_string_lossy();
            match path.strip_prefix(home.as_ref()) {
                Some(rest) => format!("~{rest}"),
                None => path.to_string(),
            }
        }
        None => path.to_string(),
    }
}

pub fn truncate(text: &str, length: usize) -> String {
    if text.chars().count() > length {
        let mut truncated: String = text.chars().take(length - 1).collect();
        truncated.push('…');
        truncated
    } else {
        text.to_string()
    }
}

pub fn role_icon(role: Role) -> &'static str {
    match role {
        Role::User => "❯",
        Role::Assistant => "●",
        Role::System => "◦",
    }
}

/// Fixed-width left-aligned column padding (rough; CJK width ignored).
pub fn pad(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count >= width {
        text.to_string()
    } else {
        format!("{text}{}", " ".repeat(width - count))
    }
}
