use std::path::Path;
use std::time::{Duration, SystemTime};

use time::{format_description, OffsetDateTime, UtcOffset};

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];

    if bytes < 1024 {
        return format!("{bytes} B");
    }

    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    format!("{value:.1} {}", UNITS[unit])
}

pub fn format_duration(duration: Duration) -> String {
    if duration.as_secs() >= 60 {
        let minutes = duration.as_secs() / 60;
        let seconds = duration.as_secs() % 60;
        return format!("{minutes}m {seconds}s");
    }

    format!("{:.1}s", duration.as_secs_f32())
}

pub fn shorten_path(path: &Path, max_chars: usize) -> String {
    let display = path.display().to_string();
    shorten_text(&display, max_chars)
}

pub fn shorten_text(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }

    let keep = max_chars.saturating_sub(3);
    let suffix = &text[text.len().saturating_sub(keep)..];
    format!("...{suffix}")
}

pub fn format_modified_time(modified_at: Option<SystemTime>) -> String {
    let Some(modified_at) = modified_at else {
        return String::from("Unknown");
    };

    let Ok(format) = format_description::parse("[year]-[month]-[day] [hour]:[minute]") else {
        return String::from("Unknown");
    };

    let datetime = OffsetDateTime::from(modified_at);
    let localized = UtcOffset::current_local_offset()
        .map(|offset| datetime.to_offset(offset))
        .unwrap_or(datetime);

    localized
        .format(&format)
        .unwrap_or_else(|_| String::from("Unknown"))
}
