//! Small, shared rendering helpers used across the table/TSV command output.
//!
//! These are pure string utilities that were previously copy-pasted into nearly
//! every command module. Domain-specific coloring (PR vs issue vs pipeline
//! state vocabularies) intentionally stays local to each command, since those
//! mappings genuinely differ.

/// Replace tab/CR/LF with a single space so a cell can't break table layout or
/// smuggle terminal control sequences into the output.
pub(crate) fn sanitize(s: &str) -> String {
    s.replace(['\t', '\r', '\n'], " ")
}

/// Right-pad `s` so its *visible* width (`plain_len`, which ignores ANSI color
/// codes the string may already contain) reaches `target`. Never truncates.
pub(crate) fn pad(s: &str, plain_len: usize, target: usize) -> String {
    let mut out = s.to_owned();
    if plain_len < target {
        out.push_str(&" ".repeat(target - plain_len));
    }
    out
}

/// Percent-encode a value for a URL query (e.g. a Bitbucket `q=` filter): keep
/// RFC 3986 unreserved characters, escape everything else as `%XX`.
pub(crate) fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Escape a value for embedding inside a double-quoted BBQL string literal
/// (Bitbucket's `q=` query language), so a value containing `"` can't break out
/// of the literal and alter the query. Backslash is escaped first so the escape
/// character itself is literal.
pub(crate) fn bbql_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_replaces_control_chars_with_spaces() {
        assert_eq!(sanitize("a\tb\r\nc"), "a b  c");
        assert_eq!(sanitize("plain"), "plain");
    }

    #[test]
    fn pad_extends_to_target_and_never_truncates() {
        assert_eq!(pad("ab", 2, 5), "ab   ");
        assert_eq!(pad("abcdef", 6, 3), "abcdef");
        assert_eq!(pad("x", 1, 1), "x");
    }

    #[test]
    fn percent_encode_keeps_unreserved_and_escapes_rest() {
        assert_eq!(percent_encode("a-z_0.9~"), "a-z_0.9~");
        assert_eq!(percent_encode("a b\"=&"), "a%20b%22%3D%26");
    }
}
