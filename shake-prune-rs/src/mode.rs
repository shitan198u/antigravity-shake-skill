use crate::analysis::TranscriptAnalysis;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CompactionMode {
    #[default]
    Auto,
    Standard,
    Deep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResolvedMode {
    Standard,
    Deep,
}

impl std::fmt::Display for CompactionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompactionMode::Auto => write!(f, "auto"),
            CompactionMode::Standard => write!(f, "standard"),
            CompactionMode::Deep => write!(f, "deep"),
        }
    }
}

impl std::fmt::Display for ResolvedMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolvedMode::Standard => write!(f, "standard"),
            ResolvedMode::Deep => write!(f, "deep"),
        }
    }
}

impl std::str::FromStr for CompactionMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().trim() {
            "auto" => Ok(CompactionMode::Auto),
            "standard" => Ok(CompactionMode::Standard),
            "deep" => Ok(CompactionMode::Deep),
            other => Err(format!(
                "Invalid compaction mode '{}'. Valid modes: auto, standard, deep",
                other
            )),
        }
    }
}

/// Resolve requested mode against transcript analysis metrics and configured threshold.
pub fn resolve_mode(
    requested: CompactionMode,
    analysis: &TranscriptAnalysis,
    deep_after_user_turns: usize,
) -> ResolvedMode {
    match requested {
        CompactionMode::Standard => ResolvedMode::Standard,
        CompactionMode::Deep => ResolvedMode::Deep,
        CompactionMode::Auto => {
            if analysis.total_user_turns > deep_after_user_turns {
                ResolvedMode::Deep
            } else {
                ResolvedMode::Standard
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn mock_analysis(user_turns: usize) -> TranscriptAnalysis {
        TranscriptAnalysis {
            transcript_path: PathBuf::from("transcript.jsonl"),
            logs_dir: PathBuf::from("."),
            artifact_dir: PathBuf::from("."),
            bytes: 1000,
            estimated_tokens: 300,
            total_user_turns: user_turns,
            total_assistant_turns: user_turns,
            total_tool_steps: 10,
            unpruned_tool_count: 5,
            failed_tool_count: 0,
            max_step_index: 20,
            last_user_request: Some("fix issue".to_string()),
            recent_failed_tools: vec![],
            recent_files: vec![],
        }
    }

    #[test]
    fn test_auto_mode_resolves_standard_under_threshold() {
        let a = mock_analysis(15);
        assert_eq!(
            resolve_mode(CompactionMode::Auto, &a, 30),
            ResolvedMode::Standard
        );
    }

    #[test]
    fn test_auto_mode_resolves_deep_over_threshold() {
        let a = mock_analysis(35);
        assert_eq!(
            resolve_mode(CompactionMode::Auto, &a, 30),
            ResolvedMode::Deep
        );
    }

    #[test]
    fn test_explicit_modes_override_threshold() {
        let a_small = mock_analysis(5);
        assert_eq!(
            resolve_mode(CompactionMode::Deep, &a_small, 30),
            ResolvedMode::Deep
        );

        let a_large = mock_analysis(50);
        assert_eq!(
            resolve_mode(CompactionMode::Standard, &a_large, 30),
            ResolvedMode::Standard
        );
    }
}
