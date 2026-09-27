/// Strips ANSI SGR sequences (`ESC[` + digits/`;` + `m`) from `input`.
pub fn strip_sgr(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(input.len());
    let mut kept = 0;
    let mut index = 0;
    while index < bytes.len() {
        if let Some(end) = sgr_end(bytes, index) {
            output.push_str(&input[kept..index]);
            kept = end;
            index = end;
        } else {
            index += char_width(bytes[index]);
        }
    }
    output.push_str(&input[kept..]);
    output
}

fn sgr_end(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start) != Some(&0x1b) || bytes.get(start + 1) != Some(&b'[') {
        return None;
    }
    let mut index = start + 2;
    while matches!(bytes.get(index), Some(byte) if byte.is_ascii_digit() || *byte == b';') {
        index += 1;
    }
    (bytes.get(index) == Some(&b'm')).then_some(index + 1)
}

fn char_width(leading_byte: u8) -> usize {
    match leading_byte {
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => 1,
    }
}

/// Renders entries as at most 50 directory-then-file lines plus a `… +K more` line.
pub fn render(entries: Vec<(String, bool)>) -> Vec<String> {
    let mut entries = entries;
    entries.sort_by(|(left_name, left_is_dir), (right_name, right_is_dir)| {
        right_is_dir
            .cmp(left_is_dir)
            .then_with(|| left_name.to_lowercase().cmp(&right_name.to_lowercase()))
            .then_with(|| left_name.cmp(right_name))
    });
    const LIMIT: usize = 50;
    let shown = &entries[..entries.len().min(LIMIT)];
    let mut lines: Vec<String> = shown
        .iter()
        .map(|(name, is_dir)| {
            if *is_dir {
                format!("{name}\\")
            } else {
                name.clone()
            }
        })
        .collect();
    let remaining = entries.len() - shown.len();
    if remaining > 0 {
        lines.push(format!("… +{remaining} more"));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::{render, strip_sgr};

    #[test]
    fn strip_sgr_removes_color_codes_and_keeps_the_rest() {
        assert_eq!(strip_sgr("\x1b[01;34mC:\\a\x1b[0m"), "C:\\a");
        assert_eq!(strip_sgr("C:\\plain\\path"), "C:\\plain\\path");
        assert_eq!(strip_sgr("C:\\a\x1b"), "C:\\a\x1b");
    }

    #[test]
    fn render_lists_directories_first_then_files_ignoring_case() {
        let lines = render(vec![
            ("Zulu.txt".to_owned(), false),
            ("beta".to_owned(), true),
            ("Alpha".to_owned(), true),
            ("a.txt".to_owned(), false),
        ]);
        assert_eq!(lines, vec!["Alpha\\", "beta\\", "a.txt", "Zulu.txt"]);
    }

    #[test]
    fn render_truncates_at_fifty_lines_with_a_more_line() {
        let entries: Vec<(String, bool)> = (0..51).map(|i| (format!("f{i:02}"), false)).collect();
        let lines = render(entries);
        assert_eq!(lines.len(), 51);
        assert_eq!(lines[50], "… +1 more");
        let fifty: Vec<(String, bool)> = (0..50).map(|i| (format!("f{i:02}"), false)).collect();
        assert_eq!(render(fifty).len(), 50);
    }
}
