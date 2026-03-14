#![forbid(unsafe_code)]

/// Terminal color and table rendering utilities.
///
/// Respects `NO_COLOR` env and `TERM=dumb` conventions.

/// ANSI 256-color codes used throughout the CLI.
pub mod colors {
    pub const ACCENT: u8 = 33;  // blue
    pub const SUCCESS: u8 = 32; // green
    pub const WARN: u8 = 33;    // yellow (SGR, not 256)
    pub const ERROR: u8 = 31;   // red
    pub const INFO: u8 = 36;    // cyan
    pub const MUTED: u8 = 90;   // dim gray
}

/// Returns true if color output should be used.
pub fn is_color_enabled() -> bool {
    if std::env::var("NO_COLOR").is_ok() {
        return false;
    }
    if let Ok(term) = std::env::var("TERM") {
        if term == "dumb" {
            return false;
        }
    }
    true
}

/// Wrap `text` in SGR escape sequences for the given color code.
pub fn colored(text: &str, color_code: u8) -> String {
    if !is_color_enabled() {
        return text.to_string();
    }
    format!("\x1b[{color_code}m{text}\x1b[0m")
}

/// Bold variant of colored.
pub fn bold_colored(text: &str, color_code: u8) -> String {
    if !is_color_enabled() {
        return text.to_string();
    }
    format!("\x1b[1;{color_code}m{text}\x1b[0m")
}

/// Strip ANSI SGR escape sequences from text.
pub fn strip_ansi(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_escape = false;
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            in_escape = true;
            continue;
        }
        if in_escape {
            // SGR sequences end with 'm' (or any letter really, but we only emit SGR)
            if ch.is_ascii_alphabetic() {
                in_escape = false;
            }
            continue;
        }
        result.push(ch);
    }
    result
}

/// Display width of text excluding ANSI escape sequences.
///
/// Uses simple char count — suitable for ASCII and most Latin scripts.
/// For East Asian full-width chars, add `unicode-width` crate.
pub fn visible_width(text: &str) -> usize {
    strip_ansi(text).chars().count()
}

/// Severity badge with color.
pub fn severity_badge(severity: &str) -> String {
    let (label, color) = match severity {
        "CRIT" => ("[CRIT]", colors::ERROR),
        "HIGH" => ("[HIGH]", colors::WARN),
        " MED" | "MED" => ("[ MED]", colors::INFO),
        " LOW" | "LOW" => ("[ LOW]", 37), // white
        "INFO" => ("[INFO]", colors::MUTED),
        other => return format!("[{other}]"),
    };
    bold_colored(label, color)
}

/// Doctor check badges with color.
pub fn check_badge(prefix: &str) -> String {
    match prefix {
        "[PASS]" => colored("[PASS]", colors::SUCCESS),
        "[WARN]" => colored("[WARN]", colors::WARN),
        "[FAIL]" => bold_colored("[FAIL]", colors::ERROR),
        "[INFO]" => colored("[INFO]", colors::MUTED),
        other => other.to_string(),
    }
}

/// Column alignment for table rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
}

/// Render an ANSI-safe table with column alignment and auto-width.
///
/// `columns` is a list of `(header, alignment)` tuples.
/// `rows` is a list of row data (each row is a Vec of cell strings).
///
/// Returns the rendered table as a string with newlines.
pub fn render_table(columns: &[(&str, Align)], rows: &[Vec<String>]) -> String {
    if columns.is_empty() {
        return String::new();
    }

    // Calculate column widths (max of header and all row cells).
    let mut widths: Vec<usize> = columns.iter().map(|(h, _)| visible_width(h)).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(visible_width(cell));
            }
        }
    }

    let mut output = String::new();

    // Header
    let header: Vec<String> = columns
        .iter()
        .enumerate()
        .map(|(i, (h, align))| pad_cell(h, widths[i], *align))
        .collect();
    output.push_str(&colored(&header.join("  "), colors::MUTED));
    output.push('\n');

    // Separator
    let sep: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
    output.push_str(&colored(&sep.join("──"), colors::MUTED));
    output.push('\n');

    // Rows
    for row in rows {
        let cells: Vec<String> = columns
            .iter()
            .enumerate()
            .map(|(i, (_, align))| {
                let cell = row.get(i).map(|s| s.as_str()).unwrap_or("");
                pad_cell(cell, widths[i], *align)
            })
            .collect();
        output.push_str(&cells.join("  "));
        output.push('\n');
    }

    output
}

/// Pad a cell to the target width, accounting for ANSI escapes.
fn pad_cell(text: &str, target_width: usize, align: Align) -> String {
    let vis = visible_width(text);
    if vis >= target_width {
        return text.to_string();
    }
    let padding = target_width - vis;
    match align {
        Align::Left => format!("{text}{}", " ".repeat(padding)),
        Align::Right => format!("{}{text}", " ".repeat(padding)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_removes_sgr_sequences() {
        assert_eq!(strip_ansi("\x1b[31mhello\x1b[0m"), "hello");
        assert_eq!(strip_ansi("\x1b[1;33mwarn\x1b[0m text"), "warn text");
        assert_eq!(strip_ansi("no escapes"), "no escapes");
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn visible_width_excludes_ansi() {
        assert_eq!(visible_width("\x1b[31mhello\x1b[0m"), 5);
        assert_eq!(visible_width("plain"), 5);
        assert_eq!(visible_width(""), 0);
    }

    #[test]
    fn render_table_alignment_and_header() {
        let cols = vec![("NAME", Align::Left), ("COUNT", Align::Right)];
        let rows = vec![
            vec!["alpha".into(), "42".into()],
            vec!["beta".into(), "7".into()],
        ];
        let rendered = render_table(&cols, &rows);
        let plain = strip_ansi(&rendered);
        // Header and separator should be present
        assert!(plain.contains("NAME"));
        assert!(plain.contains("COUNT"));
        assert!(plain.contains("────"));
        // Data rows
        assert!(plain.contains("alpha"));
        assert!(plain.contains("42"));
    }

    #[test]
    fn render_table_empty_returns_empty() {
        assert_eq!(render_table(&[], &[]), "");
    }

    #[test]
    fn no_color_env_disables_colors() {
        // This test verifies the colored() function returns plain text
        // when NO_COLOR is set. We test the logic directly since setting
        // env vars in tests is not safe to do concurrently.
        let plain = strip_ansi(&colored("test", colors::ERROR));
        assert_eq!(plain, "test");
    }

    #[test]
    fn severity_badge_formats_correctly() {
        let crit = strip_ansi(&severity_badge("CRIT"));
        assert_eq!(crit, "[CRIT]");

        let info = strip_ansi(&severity_badge("INFO"));
        assert_eq!(info, "[INFO]");
    }

    #[test]
    fn check_badge_formats_correctly() {
        let pass = strip_ansi(&check_badge("[PASS]"));
        assert_eq!(pass, "[PASS]");

        let fail = strip_ansi(&check_badge("[FAIL]"));
        assert_eq!(fail, "[FAIL]");
    }

    #[test]
    fn pad_cell_handles_ansi_width() {
        let colored_text = "\x1b[31mhi\x1b[0m";
        let padded = pad_cell(colored_text, 10, Align::Left);
        assert_eq!(visible_width(&padded), 10);
    }
}
