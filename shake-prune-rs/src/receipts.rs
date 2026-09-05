//! Receipt helpers: warning counting with low false-positive rate.
//!
//! Extracted from `pruner.rs` (P2-7). The old implementation counted every
//! case-sensitive `"WARN"` substring, which false-positived on words like
//! `SWARM`, `WARRANTY`, or `REWARN`. This counts case-insensitive `warning`
//! tokens and `warn:`-style prefixes instead without heap allocations (§4.3).

/// Count warning signals in tool output.
///
/// Matches (case-insensitive):
/// - `warning` as a substring (covers `warning:`, `warnings`, `WARNING`)
/// - `warn:` / `warn ` as a standalone token prefix (covers `WARN: foo`)
///
/// Does NOT match `swarm`, `warranty`, `rewarn` because those lack a word
/// boundary before `warn` followed by `ing`, `:`, or whitespace/end.
pub fn count_warnings(content: &str) -> usize {
    let bytes = content.as_bytes();
    let mut count = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        // Zero-alloc case-insensitive substring match (§4.3).
        if i + 7 <= bytes.len() && bytes[i..i + 7].eq_ignore_ascii_case(b"warning") {
            count += 1;
            i += 7;
            continue;
        }
        // Match standalone "warn" followed by ':' / whitespace / end / '(' / '['.
        if i + 4 <= bytes.len() && bytes[i..i + 4].eq_ignore_ascii_case(b"warn") {
            let prev_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
            let after = i + 4;
            let next_ok = after >= bytes.len()
                || matches!(
                    bytes[after],
                    b':' | b' ' | b'\t' | b'\n' | b'\r' | b'(' | b'['
                );
            if prev_ok && next_ok {
                count += 1;
                i += 4;
                continue;
            }
        }
        i += 1;
    }
    count
}

/// Check if a transcript step type is a tool execution (D1).
pub fn is_tool_step_type(stype: &str) -> bool {
    matches!(
        stype,
        "RUN_COMMAND" | "VIEW_FILE" | "SEARCH_WEB" | "GREP_SEARCH" | "CODE_ACTION"
    )
}

/// Check if a string content represents a pruned receipt (B10).
pub fn is_pruned_receipt(content: &str) -> bool {
    content.starts_with("[PRUNED")
}

/// Parse receipt metadata tokens: lines count, master archive line number, and archive path.
pub fn parse_receipt_info(receipt: &str) -> (Option<usize>, Option<usize>, Option<String>) {
    let mut lines_count = None;
    let mut line_no = None;
    let mut archive_path = None;
    for token in receipt
        .trim_matches(|c| c == '[' || c == ']')
        .split_whitespace()
    {
        if let Some(val) = token.strip_prefix("lines=") {
            lines_count = val.parse::<usize>().ok();
        } else if let Some(val) = token.strip_prefix("line=") {
            line_no = val.parse::<usize>().ok();
        } else if let Some(val) = token.strip_prefix("archive=") {
            archive_path = Some(val.to_string());
        }
    }
    (lines_count, line_no, archive_path)
}

/// Build a deterministic pruned receipt string (D6).
///
/// If `archive` is provided as `Some((archive_path, line_no))`, the receipt references
/// the permanent master audit archive. If `None` (e.g. unindexed/synthetic steps),
/// archive pointers are omitted to guarantee Invariant #3 ("every receipt with an archive
/// pointer resolves to an existing line in transcript_full.jsonl") (B4).
pub fn build_receipt(
    tool: &str,
    step: Option<u64>,
    archive: Option<(&str, usize)>,
    lines_count: Option<usize>,
    extra_tags: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    parts.push(format!("tool={}", tool));
    if let Some(s) = step {
        parts.push(format!("step={}", s));
    }
    if let Some(extra) = extra_tags {
        if !extra.is_empty() {
            parts.push(extra.to_string());
        }
    }
    if let Some(lc) = lines_count {
        parts.push(format!("lines={}", lc));
    }
    if let Some((arch, line)) = archive {
        parts.push(format!("archive={}", arch));
        parts.push(format!("line={}", line));
    }
    format!("[PRUNED {}]", parts.join(" "))
}

/// Render a collapsible `<details>` card for a pruned tool receipt in the markdown artifact (D5).
pub fn render_receipt_card(
    tool_name: &str,
    summary_code: &str,
    param_line: &str,
    receipt: &str,
) -> String {
    let (lc, ln, ap) = parse_receipt_info(receipt);
    let lines_label = match lc {
        Some(c) => format!("{} lines archived", c),
        None => "Archived receipt".to_string(),
    };
    let archive_link = match (ap.as_deref(), ln) {
        (Some(arch), Some(l)) => format!(
            "- **Master Archive**: [View line {} in transcript_full.jsonl](file://{}#L{})\n",
            l, arch, l
        ),
        _ => String::new(),
    };
    format!(
        "<details>\n<summary>⚙️ <b>{}</b>{} — <i>{}</i></summary>\n\n{}- **Archive Receipt**: `{}`\n{}\n</details>\n\n",
        tool_name, summary_code, lines_label, param_line, receipt, archive_link
    )
}

#[cfg(test)]
mod tests {
    use super::count_warnings;

    #[test]
    fn detects_warning_variants() {
        assert_eq!(count_warnings("warning: disk low"), 1);
        assert_eq!(count_warnings("Warning: x\nwarnings=2"), 2);
        assert_eq!(count_warnings("WARN: boom"), 1);
        assert_eq!(count_warnings("all good"), 0);
    }

    #[test]
    fn ignores_swarm_and_warranty() {
        assert_eq!(count_warnings("swarm cluster ready"), 0);
        assert_eq!(count_warnings("warranty expired"), 0);
    }
}
