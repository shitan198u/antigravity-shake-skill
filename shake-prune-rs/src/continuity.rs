use crate::analysis::TranscriptAnalysis;
use crate::pruner::{redact_secrets, safe_truncate};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FailureSummary {
    pub step: u64,
    pub tool: String,
    pub exit: Option<i64>,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContinuityCard {
    pub task: Option<String>,
    pub recent_files: Vec<String>,
    pub recent_failures: Vec<FailureSummary>,
    pub undo_command: String,
    pub archive_path: String,
}

impl ContinuityCard {
    pub fn build(
        analysis: &TranscriptAnalysis,
        archive_path: &str,
        transcript_path: &Path,
        redact: bool,
    ) -> Self {
        let task = analysis.last_user_request.as_ref().map(|t| {
            let s = safe_truncate(t, 200);
            if redact {
                redact_secrets(&s)
            } else {
                s
            }
        });

        let mut recent_files: Vec<String> = analysis
            .recent_files
            .iter()
            .rev()
            .take(3)
            .map(|f| {
                let s = safe_truncate(f, 120);
                if redact {
                    redact_secrets(&s)
                } else {
                    s
                }
            })
            .collect();
        recent_files.reverse();

        let mut recent_failures: Vec<FailureSummary> = analysis
            .recent_failed_tools
            .iter()
            .rev()
            .take(3)
            .map(|f| FailureSummary {
                step: f.step_index,
                tool: f.tool_type.clone(),
                exit: f.exit_code,
                snippet: if redact {
                    redact_secrets(&f.snippet)
                } else {
                    f.snippet.clone()
                },
            })
            .collect();
        recent_failures.reverse();

        let undo_command = format!("shake-prune undo {}", transcript_path.display());

        Self {
            task,
            recent_files,
            recent_failures,
            undo_command,
            archive_path: archive_path.to_string(),
        }
    }

    pub fn to_ephemeral_notice(
        &self,
        trigger_phrase: &str,
        shaken_file: &str,
        anchored_step: u64,
    ) -> String {
        let mut parts = Vec::new();
        if let Some(t) = &self.task {
            let clean = t.replace(['\n', '\r'], " ").trim().to_string();
            if !clean.is_empty() {
                parts.push(format!("Task: {}", clean));
            }
        }
        if !self.recent_files.is_empty() {
            parts.push(format!("Recent files: {}", self.recent_files.join(", ")));
        }
        if !self.recent_failures.is_empty() {
            let fails: Vec<String> = self
                .recent_failures
                .iter()
                .map(|f| {
                    if let Some(code) = f.exit {
                        format!("{} step={} exit={}", f.tool, f.step, code)
                    } else {
                        format!("{} step={}", f.tool, f.step)
                    }
                })
                .collect();
            parts.push(format!("Recent failures: {}", fails.join("; ")));
        }
        parts.push(format!("Undo: {}", self.undo_command));

        let details = parts.join("; ");
        let notice = format!(
            "[Context auto-compacted via /shake ({}). Active state anchored in @{} (Step {}+). Continue from: {}.]",
            trigger_phrase, shaken_file, anchored_step, details
        );

        if notice.chars().count() > 1000 {
            safe_truncate(&notice, 996) + "...]"
        } else {
            notice
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::FailedToolSummary;
    use std::path::PathBuf;

    #[test]
    fn test_continuity_card_formatting_and_bounds() {
        let analysis = TranscriptAnalysis {
            transcript_path: PathBuf::from("/home/user/transcript.jsonl"),
            logs_dir: PathBuf::from("/home/user"),
            artifact_dir: PathBuf::from("/home/user"),
            bytes: 500000,
            estimated_tokens: 150000,
            total_user_turns: 12,
            total_assistant_turns: 12,
            total_tool_steps: 30,
            unpruned_tool_count: 22,
            failed_tool_count: 1,
            max_step_index: 45,
            last_user_request: Some("Implement new feature".to_string()),
            recent_failed_tools: vec![FailedToolSummary {
                step_index: 40,
                tool_type: "RUN_COMMAND".to_string(),
                exit_code: Some(1),
                snippet: "build failed".to_string(),
            }],
            recent_files: vec!["src/main.rs".to_string(), "src/lib.rs".to_string()],
        };

        let card = ContinuityCard::build(
            &analysis,
            "/home/user/transcript_full.jsonl",
            Path::new("/home/user/transcript.jsonl"),
            false,
        );

        assert_eq!(card.task, Some("Implement new feature".to_string()));
        assert_eq!(card.recent_files, vec!["src/main.rs", "src/lib.rs"]);
        assert_eq!(card.recent_failures.len(), 1);

        let notice = card.to_ephemeral_notice(
            "exceeded 80k token threshold",
            "/home/user/shake_latest.md",
            45,
        );
        assert!(notice.contains("exceeded 80k token threshold"));
        assert!(notice.contains("@/home/user/shake_latest.md"));
        assert!(notice.contains("Task: Implement new feature"));
        assert!(notice.contains("Recent files: src/main.rs, src/lib.rs"));
        assert!(notice.contains("Recent failures: RUN_COMMAND step=40 exit=1"));
        assert!(notice.contains("Undo: shake-prune undo /home/user/transcript.jsonl"));
        assert!(notice.len() <= 1000);
    }
}
