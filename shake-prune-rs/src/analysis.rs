use crate::pruner::{estimate_tokens, extract_user_request_text, safe_truncate};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedToolSummary {
    pub step_index: u64,
    pub tool_type: String,
    pub exit_code: Option<i64>,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptAnalysis {
    pub transcript_path: PathBuf,
    pub logs_dir: PathBuf,
    pub artifact_dir: PathBuf,
    pub bytes: u64,
    pub estimated_tokens: usize,
    pub total_user_turns: usize,
    pub total_assistant_turns: usize,
    pub total_tool_steps: usize,
    pub unpruned_tool_count: usize,
    pub failed_tool_count: usize,
    pub max_step_index: u64,
    pub last_user_request: Option<String>,
    pub recent_failed_tools: Vec<FailedToolSummary>,
    pub recent_files: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct AnalysisOptions {
    /// Stop counting unpruned tools once this threshold is reached.
    pub unpruned_tools_threshold: Option<usize>,
}

/// Analyze a transcript.jsonl file without modifying any disk state.
pub fn analyze_transcript(
    transcript_path: &Path,
    _options: &AnalysisOptions,
) -> Result<TranscriptAnalysis, Box<dyn std::error::Error>> {
    let metadata = std::fs::metadata(transcript_path)?;
    let bytes = metadata.len();
    let estimated_tokens = estimate_tokens(bytes as usize);

    let logs_dir = transcript_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    let artifact_dir = if logs_dir.ends_with(".system_generated/logs") {
        logs_dir
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| logs_dir.clone())
    } else if logs_dir.ends_with("logs") {
        logs_dir
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| logs_dir.clone())
    } else {
        logs_dir.clone()
    };

    let file = File::open(transcript_path)?;
    let reader = BufReader::new(file);

    let mut total_user_turns = 0usize;
    let mut total_assistant_turns = 0usize;
    let mut total_tool_steps = 0usize;
    let mut unpruned_tool_count = 0usize;
    let mut failed_tool_count = 0usize;
    let mut max_step_index = 0u64;
    let mut last_user_request: Option<String> = None;
    let mut recent_failed_tools: Vec<FailedToolSummary> = Vec::new();
    let mut recent_files_set: HashSet<String> = HashSet::new();
    let mut recent_files: Vec<String> = Vec::new();

    for line_res in reader.lines() {
        let line = match line_res {
            Ok(l) => l,
            Err(_) => continue,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let val: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if let Some(idx) = val.get("step_index").and_then(|v| v.as_u64()) {
            if idx > max_step_index {
                max_step_index = idx;
            }
        }

        let stype = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match stype {
            "USER_INPUT" => {
                total_user_turns += 1;
                let raw_content = val.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let text = extract_user_request_text(raw_content);
                if !text.trim().is_empty() {
                    last_user_request = Some(text.trim().to_string());
                }
            }
            "PLANNER_RESPONSE" => {
                total_assistant_turns += 1;
                if let Some(tool_calls) = val.get("tool_calls").and_then(|v| v.as_array()) {
                    for tc in tool_calls {
                        if let Some(args) = tc.get("args").and_then(|v| v.as_object()) {
                            for key in [
                                "AbsolutePath",
                                "TargetFile",
                                "FilePath",
                                "Path",
                                "TargetPath",
                                "File",
                            ] {
                                if let Some(path_str) = args.get(key).and_then(|v| v.as_str()) {
                                    let clean = path_str.trim_matches('"').trim();
                                    if !clean.is_empty()
                                        && !recent_files_set.contains(clean)
                                        && (clean.contains('/') || clean.contains('\\'))
                                    {
                                        recent_files_set.insert(clean.to_string());
                                        recent_files.push(clean.to_string());
                                        if recent_files.len() > 10 {
                                            recent_files.remove(0);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            stype if crate::receipts::is_tool_step_type(stype) => {
                total_tool_steps += 1;
                let content_str = val.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let status = val
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_lowercase();
                let exit_code = val.get("exit_code").and_then(|v| v.as_i64());
                let is_error = exit_code.map(|c| c != 0).unwrap_or(false)
                    || status.contains("error")
                    || status.contains("failed");

                if is_error {
                    failed_tool_count += 1;
                    let snippet = safe_truncate(content_str, 100);
                    let step_index = val.get("step_index").and_then(|v| v.as_u64()).unwrap_or(0);
                    recent_failed_tools.push(FailedToolSummary {
                        step_index,
                        tool_type: stype.to_string(),
                        exit_code,
                        snippet,
                    });
                    if recent_failed_tools.len() > 5 {
                        recent_failed_tools.remove(0);
                    }
                }

                let is_pruned = crate::receipts::is_pruned_receipt(content_str);
                if !is_pruned {
                    if let Some(thresh) = _options.unpruned_tools_threshold {
                        if unpruned_tool_count < thresh {
                            unpruned_tool_count += 1;
                        }
                    } else {
                        unpruned_tool_count += 1;
                    }
                }
            }
            _ => {}
        }
    }

    Ok(TranscriptAnalysis {
        transcript_path: transcript_path.to_path_buf(),
        logs_dir,
        artifact_dir,
        bytes,
        estimated_tokens,
        total_user_turns,
        total_assistant_turns,
        total_tool_steps,
        unpruned_tool_count,
        failed_tool_count,
        max_step_index,
        last_user_request,
        recent_failed_tools,
        recent_files,
    })
}
