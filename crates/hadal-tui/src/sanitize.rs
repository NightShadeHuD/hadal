//! Sanitize externally-sourced strings by replacing C0/C1 control characters.
//!
//! Paths, OS error messages, config details, and import filenames are rendered
//! in single-line console cells. ESC/CSI and the other C0/C1 controls could
//! move the cursor, clear the screen, or otherwise corrupt output, so every
//! character in `U+0000..=U+001F` and `U+0080..=U+009F` is replaced with
//! `U+FFFD` (the Unicode replacement character).

/// Replace C0 and C1 control characters with `U+FFFD`.
pub fn sanitize(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            let code = c as u32;
            if (0x00..=0x1F).contains(&code) || (0x80..=0x9F).contains(&code) {
                '\u{FFFD}'
            } else {
                c
            }
        })
        .collect()
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_replaced() {
        let input = "\x1b[31mred\x1b[0m";
        let result = sanitize(input);
        // ESC bytes are replaced; the now-inert bracket text may remain but can
        // no longer drive the terminal.
        assert!(!result.contains('\x1b'));
        assert_eq!(result.matches('\u{FFFD}').count(), 2);
        assert!(result.contains("red"));
    }

    #[test]
    fn carriage_return_replaced() {
        let input = "hello\rworld";
        let result = sanitize(input);
        assert!(!result.contains('\r'));
        assert_eq!(result.matches('\u{FFFD}').count(), 1);
        assert!(result.contains("hello"));
        assert!(result.contains("world"));
    }

    #[test]
    fn null_replaced() {
        let input = "hello\0world";
        let result = sanitize(input);
        assert!(!result.contains('\0'));
        assert_eq!(result.matches('\u{FFFD}').count(), 1);
    }

    #[test]
    fn tab_replaced() {
        let input = "hello\tworld";
        let result = sanitize(input);
        assert!(!result.contains('\t'));
        assert_eq!(result.matches('\u{FFFD}').count(), 1);
    }

    #[test]
    fn newline_replaced() {
        let input = "line1\nline2";
        let result = sanitize(input);
        assert!(!result.contains('\n'));
        assert_eq!(result.matches('\u{FFFD}').count(), 1);
    }

    #[test]
    fn c1_replaced() {
        let input = "\u{80}\u{9F}test";
        let result = sanitize(input);
        assert!(!result.contains('\u{80}'));
        assert!(!result.contains('\u{9F}'));
        assert_eq!(result.matches('\u{FFFD}').count(), 2);
        assert!(result.contains("test"));
    }

    #[test]
    fn clean_string_unchanged() {
        let input = "Hello, World! 123";
        assert_eq!(sanitize(input), input);
    }

    #[test]
    fn path_with_control_chars() {
        let input = "/data/\x1b[2msave.json";
        let result = sanitize(input);
        assert!(!result.contains('\x1b'));
        assert!(result.contains("/data/"));
        assert!(result.contains("save.json"));
    }
}
