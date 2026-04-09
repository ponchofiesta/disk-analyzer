use std::path::Path;
use std::time::Duration;

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
    if display.len() <= max_chars {
        return display;
    }

    let keep = max_chars.saturating_sub(3);
    let suffix = &display[display.len().saturating_sub(keep)..];
    format!("...{suffix}")
}
