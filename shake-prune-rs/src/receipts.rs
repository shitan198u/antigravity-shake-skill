//! Receipt helpers: warning counting with low false-positive rate.
//!
//! Extracted from `pruner.rs` (P2-7). The old implementation counted every
//! case-sensitive `"WARN"` substring, which false-positived on words like
//! `SWARM`, `WARRANTY`, or `REWARN`. This counts case-insensitive `warning`
//! tokens and `warn:`-style prefixes instead.

/// Count warning signals in tool output.
///
/// Matches (case-insensitive):
/// - `warning` as a substring (covers `warning:`, `warnings`, `WARNING`)
/// - `warn:` / `warn ` as a standalone token prefix (covers `WARN: foo`)
///
/// Does NOT match `swarm`, `warranty`, `rewarn` because those lack a word
/// boundary before `warn` followed by `ing`, `:`, or whitespace/end.
pub fn count_warnings(content: &str) -> usize {
    let lower = content.to_lowercase();
    let bytes = lower.as_bytes();
    let mut count = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        // Match "warning" substring.
        if lower[i..].starts_with("warning") {
            count += 1;
            i += "warning".len();
            continue;
        }
        // Match standalone "warn" followed by ':' / whitespace / end / '(' / '['.
        if lower[i..].starts_with("warn") {
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
