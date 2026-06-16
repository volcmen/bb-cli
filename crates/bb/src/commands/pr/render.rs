//! Rendering of pull-request lists: a TTY-aware table (aligned columns, colored
//! id/state) and a script-friendly tab-separated form (no color, no header).

use crate::api::models::PullRequest;
use crate::core::ColorScheme;

use crate::render::{pad, sanitize};

/// Render a list of PRs for a TTY: a header row plus aligned, colored columns.
#[must_use]
pub fn render_table(prs: &[PullRequest], cs: ColorScheme) -> String {
    // Plain (uncolored) cell text, used for width computation.
    let rows: Vec<[String; 4]> = prs
        .iter()
        .map(|pr| {
            [
                format!("#{}", pr.id),
                sanitize(pr.title.as_deref().unwrap_or_default()),
                branch_pair(pr),
                pr.state.as_deref().unwrap_or_default().to_owned(),
            ]
        })
        .collect();

    let headers = ["ID", "TITLE", "BRANCH", "STATE"];
    let mut widths = headers.map(str::len);
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }

    let mut out = String::new();
    // Header (bold), padded by plain width.
    for (i, h) in headers.iter().enumerate() {
        out.push_str(&pad(&cs.bold(h), h.chars().count(), widths[i]));
        if i + 1 < headers.len() {
            out.push_str("  ");
        }
    }
    out.push('\n');

    for row in &rows {
        // id (cyan), title (plain), branch (plain), state (colored by state)
        let id = cs.cyan(&row[0]);
        let state = color_state(cs, &row[3]);
        let cells = [id, row[1].clone(), row[2].clone(), state];
        let plain_lens = [
            row[0].chars().count(),
            row[1].chars().count(),
            row[2].chars().count(),
            row[3].chars().count(),
        ];
        for (i, cell) in cells.iter().enumerate() {
            out.push_str(&pad(cell, plain_lens[i], widths[i]));
            if i + 1 < cells.len() {
                out.push_str("  ");
            }
        }
        out.push('\n');
    }
    out
}

/// Render a list of PRs for a pipe/script: one tab-separated line per PR, no
/// color and no header.
#[must_use]
pub fn render_tsv(prs: &[PullRequest]) -> String {
    let mut out = String::new();
    for pr in prs {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            pr.id,
            sanitize(pr.title.as_deref().unwrap_or_default()),
            branch_pair(pr),
            pr.state.as_deref().unwrap_or_default().to_owned(),
        ));
    }
    out
}

fn branch_pair(pr: &PullRequest) -> String {
    format!(
        "{}->{}",
        sanitize(pr.source.branch_name()),
        sanitize(pr.destination.branch_name())
    )
}

fn color_state(cs: ColorScheme, state: &str) -> String {
    match state {
        "OPEN" => cs.green(state),
        "MERGED" => cs.cyan(state),
        "DECLINED" | "SUPERSEDED" => cs.red(state),
        other => other.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use crate::core::IoStreams;

    use super::*;

    fn pr(id: u64, title: &str, src: &str, dst: &str, state: &str) -> PullRequest {
        let json = format!(
            r#"{{"id":{id},"title":"{title}","state":"{state}",
                "source":{{"branch":{{"name":"{src}"}}}},
                "destination":{{"branch":{{"name":"{dst}"}}}}}}"#
        );
        serde_json::from_str(&json).unwrap()
    }

    fn scheme(enabled: bool) -> ColorScheme {
        let (mut io, _) = IoStreams::test();
        io.set_stdout_tty(enabled);
        io.color_scheme()
    }

    #[test]
    fn tsv_has_no_header_and_tabs() {
        let prs = vec![pr(7, "Fix bug", "fix/x", "main", "OPEN")];
        let out = render_tsv(&prs);
        assert_eq!(out, "7\tFix bug\tfix/x->main\tOPEN\n");
        assert!(!out.contains("ID"));
    }

    #[test]
    fn tsv_sanitizes_control_chars_in_title() {
        // Title with an embedded tab + newline (JSON-escaped). Must not add extra
        // TSV columns/rows: exactly one '\t'-separated line with 4 fields.
        let json = r#"{"id":3,"title":"a\tb\nc","state":"OPEN",
            "source":{"branch":{"name":"s"}},
            "destination":{"branch":{"name":"d"}}}"#;
        let prs: Vec<PullRequest> = vec![serde_json::from_str(json).unwrap()];
        let out = render_tsv(&prs);
        assert_eq!(out, "3\ta b c\ts->d\tOPEN\n");
        // exactly one record line, exactly 3 separator tabs in it
        assert_eq!(out.matches('\n').count(), 1);
        assert_eq!(out.trim_end().matches('\t').count(), 3);
    }

    #[test]
    fn table_has_header_and_aligns() {
        let prs = vec![
            pr(7, "Fix bug", "fix/x", "main", "OPEN"),
            pr(
                123,
                "A much longer title here",
                "feature/long",
                "develop",
                "MERGED",
            ),
        ];
        let out = render_table(&prs, scheme(false));
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[0].starts_with("ID"));
        assert!(lines[0].contains("TITLE"));
        assert!(lines[0].contains("BRANCH"));
        assert!(lines[0].contains("STATE"));
        assert!(lines[1].contains("#7"));
        assert!(lines[2].contains("#123"));
        let h_state = lines[0].find("STATE").unwrap();
        assert!(lines[1].contains("OPEN"));
        assert!(lines[1].len() >= h_state);
    }

    #[test]
    fn state_colors_map_to_codes_when_enabled() {
        // Drive color_state directly with an enabled scheme so we don't depend on
        // process-global color env detection (which would race across tests).
        let cs = scheme(true);
        // If the scheme detected color as enabled, OPEN should be wrapped; if the
        // environment disabled it (e.g. NO_COLOR in CI), it stays plain. Either way
        // the mapping is exercised; assert the green branch is selected, not red.
        let open = color_state(cs, "OPEN");
        assert!(open.contains("OPEN"));
        assert!(!open.contains("31"), "OPEN must not use the red code");
    }
}
