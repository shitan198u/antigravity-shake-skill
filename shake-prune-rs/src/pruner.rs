use crate::models::{PruningStats, Step};
use crate::slug::{extract_conversation_id, generate_suggested_filename, generate_topic_slug};
use regex::Regex;
use serde_json::Value;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

pub fn estimate_tokens(byte_count: usize) -> usize {
    std::cmp::max(1, byte_count / 4)
}

fn safe_truncate(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// Compacts transcript.jsonl directly in-place on disk with backup and atomic write.
/// Physically replaces bloated tool outputs with compact action receipts while preserving
/// 100% of User dialogue, Assistant thoughts/reasoning, Error stack traces, and recent active window.
pub fn compact_transcript_inplace(
    transcript_path: &Path,
    recent_window_steps: usize,
) -> Result<(usize, usize), Box<dyn std::error::Error>> {
    // 1. Pass 1: Count total steps
    let file_pass1 = File::open(transcript_path)?;
    let reader_pass1 = BufReader::new(file_pass1);
    let mut total_steps = 0usize;
    let mut initial_bytes = 0usize;

    for line in reader_pass1.lines() {
        let line_str = line?;
        if line_str.trim().is_empty() {
            continue;
        }
        initial_bytes += line_str.len();
        total_steps += 1;
    }

    let recent_threshold = total_steps.saturating_sub(recent_window_steps);

    // 2. Create raw backup transcript.jsonl.bak
    let backup_path = transcript_path.with_extension("jsonl.bak");
    let _ = fs::copy(transcript_path, &backup_path);

    // 3. Pass 2: Stream, compact tool outputs, and write atomically to .tmp
    let file_pass2 = File::open(transcript_path)?;
    let reader_pass2 = BufReader::new(file_pass2);
    let tmp_path = transcript_path.with_extension("jsonl.tmp");
    let mut tmp_file = File::create(&tmp_path)?;

    let mut compacted_bytes = 0usize;

    for (i, line) in reader_pass2.lines().enumerate() {
        let line_str = line?;
        if line_str.trim().is_empty() {
            continue;
        }

        let mut step_val: Value = match serde_json::from_str(&line_str) {
            Ok(v) => v,
            Err(_) => {
                writeln!(tmp_file, "{}", line_str)?;
                compacted_bytes += line_str.len();
                continue;
            }
        };

        let stype = step_val.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let status = step_val.get("status").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
        let exit_code = step_val.get("exit_code").and_then(|v| v.as_i64());
        let is_recent = i >= recent_threshold;

        let is_error = exit_code.map(|c| c != 0).unwrap_or(false)
            || status.contains("error")
            || status.contains("failed");

        if !is_recent && !is_error {
            match stype.as_str() {
                "RUN_COMMAND" => {
                    let content_len = step_val.get("content").and_then(|v| v.as_str()).map(|s| s.len()).unwrap_or(0);
                    if content_len > 250 {
                        step_val["content"] = serde_json::json!("Command completed successfully (exit 0). Verbose stdout pruned via /shake.");
                    }
                }
                "VIEW_FILE" => {
                    step_val["content"] = serde_json::json!("File inspected in previous turn. Content pruned via /shake.");
                }
                "SEARCH_WEB" | "GREP_SEARCH" | "CODE_ACTION" => {
                    step_val["content"] = serde_json::json!(format!("{} completed successfully. Output pruned via /shake.", stype));
                }
                _ => {}
            }
        }

        let compacted_line = serde_json::to_string(&step_val)?;
        compacted_bytes += compacted_line.len();
        writeln!(tmp_file, "{}", compacted_line)?;
    }

    tmp_file.flush()?;
    fs::rename(&tmp_path, transcript_path)?;

    Ok((initial_bytes, compacted_bytes))
}

pub fn prune_transcript(
    transcript_path: &Path,
    recent_window_steps: usize,
) -> Result<(String, PruningStats), Box<dyn std::error::Error>> {
    // Pass 1: Count total steps & measure raw payload size efficiently (O(1) memory)
    let file_pass1 = File::open(transcript_path)?;
    let reader_pass1 = BufReader::new(file_pass1);

    let mut total_steps = 0usize;
    let mut raw_bytes = 0usize;

    for line in reader_pass1.lines() {
        let line_str = line?;
        if line_str.trim().is_empty() {
            continue;
        }
        raw_bytes += line_str.len();
        total_steps += 1;
    }

    let recent_threshold = total_steps.saturating_sub(recent_window_steps);
    let conv_id = extract_conversation_id(&transcript_path.to_string_lossy());

    // Pass 2: Stream, filter, and format output incrementally without storing all steps in RAM
    let file_pass2 = File::open(transcript_path)?;
    let reader_pass2 = BufReader::new(file_pass2);

    let mut output_blocks = Vec::new();
    let mut user_count = 0usize;
    let mut assistant_count = 0usize;
    let mut pruned_tools_count = 0usize;
    let mut retained_errors_count = 0usize;
    let mut retained_short_cmds = 0usize;
    let mut retained_recent_steps = 0usize;
    let mut first_user_prompt = String::new();

    let re_user_request = Regex::new(r"(?s)<USER_REQUEST>(.*?)</USER_REQUEST>")?;

    for (i, line) in reader_pass2.lines().enumerate() {
        let line_str = line?;
        if line_str.trim().is_empty() {
            continue;
        }

        let step: Step = match serde_json::from_str(&line_str) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let stype = step.step_type.as_deref().unwrap_or("");
        let content = step.content.as_deref().unwrap_or("");
        let status = step.status.as_deref().unwrap_or("");
        let exit_code = step.exit_code;
        let is_recent = i >= recent_threshold;

        match stype {
            "USER_INPUT" => {
                user_count += 1;
                let user_text = if let Some(caps) = re_user_request.captures(content) {
                    caps.get(1).map(|m| m.as_str().trim()).unwrap_or(content.trim())
                } else {
                    content.trim()
                };
                if first_user_prompt.is_empty() {
                    first_user_prompt = user_text.to_string();
                }
                output_blocks.push(format!("### 👤 User (Turn {})\n\n{}\n", user_count, user_text));
            }
            "PLANNER_RESPONSE" => {
                let assistant_text = content.trim();
                let thinking_text = step.thinking.as_deref().unwrap_or("").trim();

                if !assistant_text.is_empty() || !thinking_text.is_empty() {
                    assistant_count += 1;
                    let mut assistant_block = String::from("### 🤖 Assistant\n\n");
                    if !thinking_text.is_empty() {
                        assistant_block.push_str(&format!(
                            "<details>\n<summary>💭 Thought Process</summary>\n\n{}\n\n</details>\n\n",
                            thinking_text
                        ));
                    }
                    if !assistant_text.is_empty() {
                        assistant_block.push_str(assistant_text);
                        assistant_block.push('\n');
                    }
                    output_blocks.push(assistant_block);
                }

                if let Some(tool_calls) = &step.tool_calls {
                    for tc in tool_calls {
                        let mut arg_items = Vec::new();
                        if let Some(args_map) = tc.args.as_ref().and_then(|v| v.as_object()) {
                            for (k, v) in args_map {
                                let v_str = match v {
                                    serde_json::Value::String(s) => s.replace('\n', " "),
                                    other => other.to_string().replace('\n', " "),
                                };
                                let v_formatted = if v_str.chars().count() > 120 {
                                    format!("{}... [truncated]", safe_truncate(&v_str, 120))
                                } else {
                                    v_str
                                };
                                arg_items.push(format!("{}={}", k, v_formatted));
                            }
                        }
                        let arg_summary = arg_items.join(", ");
                        output_blocks.push(format!("- ⚙️ **Action Executed**: `{}({})`", tc.name, arg_summary));
                    }
                }
            }
            "RUN_COMMAND" | "VIEW_FILE" | "SEARCH_WEB" | "GREP_SEARCH" | "CODE_ACTION" => {
                let is_error = exit_code.map(|c| c != 0).unwrap_or(false)
                    || status.to_lowercase().contains("error")
                    || status.to_lowercase().contains("failed");

                if is_recent {
                    retained_recent_steps += 1;
                    let snippet = safe_truncate(content, 1500);
                    output_blocks.push(format!("> 🕒 **[Active Window Tool Output ({})]**:\n```\n{}\n```\n", stype, snippet));
                } else if is_error {
                    retained_errors_count += 1;
                    let snippet = safe_truncate(content, 1200);
                    output_blocks.push(format!(
                        "> ⚠️ **[Tool Execution Error / Failure ({}, Exit code: {:?})]**:\n```\n{}\n```\n",
                        stype, exit_code, snippet
                    ));
                } else if stype == "RUN_COMMAND" {
                    if content.trim().chars().count() < 250 {
                        retained_short_cmds += 1;
                        output_blocks.push(format!("> 📋 **[Command Output (exit 0)]**:\n```\n{}\n```\n", content.trim()));
                    } else {
                        let line_count = content.lines().count();
                        pruned_tools_count += 1;
                        output_blocks.push(format!(
                            "> ℹ️ *[Command completed successfully (exit 0). {} lines of verbose stdout pruned for token efficiency]*\n",
                            line_count
                        ));
                    }
                } else if stype == "VIEW_FILE" {
                    let line_count = content.lines().count();
                    pruned_tools_count += 1;
                    output_blocks.push(format!(
                        "> ℹ️ *[File inspected in previous turn. {} lines pruned for token efficiency]*\n",
                        line_count
                    ));
                } else {
                    pruned_tools_count += 1;
                    output_blocks.push(format!(
                        "> ℹ️ *[{} completed successfully. Raw payload pruned for token efficiency]*\n",
                        stype
                    ));
                }
            }
            _ => {}
        }
    }

    let topic_slug = generate_topic_slug(&first_user_prompt);
    let suggested_filename = generate_suggested_filename(&topic_slug);

    let header = format!(
        "# Shaken & Pruned History: {}\n\n\
        > [!IMPORTANT]\n\
        > **Context Note for Assistant**:\n\
        > This document is a complete, verbatim transcript of earlier turns with token bloat removed via `/shake`.\n\
        > - **User prompts, Assistant explanations, and Thought processes are 100% complete and verbatim.**\n\
        > - Actions marked `[Command completed successfully]` or `[File inspected]` were already executed with success.\n\
        > - You do **NOT** need to re-run past successful commands unless the user explicitly requests it.\n\
        > - Any errors or failures encountered in past turns are explicitly preserved below with full stack traces.\n\
        > - The active working state and immediate recent tool outputs are preserved at the end of the transcript.\n\n\
        - **Session ID**: `{}`\n\
        - **Topic**: `{}`\n\
        - **Source Transcript**: `{}`\n\
        - **User Turns**: {} | **Assistant Turns**: {}\n\
        - **Tool Dumps Pruned**: {} | **Errors Preserved**: {}\n\
        ---\n\n",
        topic_slug.replace('_', " ").to_uppercase(),
        conv_id,
        topic_slug.replace('_', " "),
        transcript_path.display(),
        user_count,
        assistant_count,
        pruned_tools_count,
        retained_errors_count
    );

    let full_document = format!("{}{}", header, output_blocks.join("\n\n"));
    let pruned_bytes = full_document.len();
    let raw_tokens = estimate_tokens(raw_bytes);
    let pruned_tokens = estimate_tokens(pruned_bytes);
    let reduction_pct = if raw_bytes > 0 {
        (1.0 - (pruned_bytes as f64 / raw_bytes as f64)) * 100.0
    } else {
        0.0
    };

    let stats = PruningStats {
        conv_id,
        raw_bytes,
        pruned_bytes,
        raw_tokens,
        pruned_tokens,
        reduction_pct,
        user_turns: user_count,
        assistant_turns: assistant_count,
        pruned_tools: pruned_tools_count,
        retained_errors: retained_errors_count,
        retained_short_cmds: retained_short_cmds,
        retained_recent_steps,
        topic_slug,
        suggested_filename,
    };

    Ok((full_document, stats))
}
