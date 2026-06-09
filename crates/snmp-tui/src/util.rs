//! Small shared helpers used across the TUI crate.

use std::io::Write;
use std::str::FromStr;

/// Append a warning message to the debug log file.
pub fn debug_log_warning(msg: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open("/tmp/snmp-tui-debug.log")
    {
        let _ = writeln!(f, "[WARN] {}", msg);
    }
}

/// Parse a dot-separated string into a vector of values, silently dropping
/// any component that fails to parse (e.g. "1.3.6.1" -> [1, 3, 6, 1]).
pub fn parse_dotted<T: FromStr>(s: &str) -> Vec<T> {
    s.split('.').filter_map(|p| p.parse().ok()).collect()
}
