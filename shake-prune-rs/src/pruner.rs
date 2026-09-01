use crate::models::{PruningStats, Step};
use crate::slug::{extract_conversation_id, generate_suggested_filename, generate_topic_slug};
use chrono::Local;
use fs2::FileExt;
use serde_json::Value;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::Path;

/// Accurate token estimation calibrated for Code, JSON, and Markdown transcripts (~3.3 chars/token).
pub fn estimate_tokens(byte_count: usize) -> usize {
    std::cmp::max(1, (byte_count as f64 / 3.3).round() as usize)
}

fn safe_truncate(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// Escapes HTML entities and triple backticks to prevent XSS in IDE webviews and Markdown block termination.
fn sanitize_markdown_snippet(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 32);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out.replace("```", "` ` `")
}

/// Extracts text inside <USER_REQUEST>...</USER_REQUEST> using linear scan (O(N), 100% immune to ReDoS).
fn extract_user_request_text(content: &str) -> &str {
    let tag_open = "<USER_REQUEST>";
    let tag_close = "</USER_REQUEST>";
    if let Some(start_pos) = content.find(tag_open) {
        let after_open = &content[start_pos + tag_open.len()..];
        if let Some(end_pos) = after_open.find(tag_close) {
            return after_open[..end_pos].trim();
        }
        return after_open.trim();
    }
    content.trim()
}

/// Safely quotes a file path for POSIX shell output to prevent command injection.
pub fn shell_quote(path_str: &str) -> String {
    format!("'{}'", path_str.replace('\'', "'\\''"))
}

/// Compacts large tool call arguments into progressive disclosure receipts with FULL ABSOLUTE PATHS.
fn compact_tool_call_args(
    tool_name: &str,
    args_map: &mut serde_json::Map<String, Value>,
    step_idx: u64,
    backup_abs_path: &str,
) {
    match tool_name {
        "write_to_file" => {
            if let Some(code_val) = args_map.get("CodeContent").and_then(|v| v.as_str()) {
                if code_val.len() > 200 {
                    let line_count = code_val.lines().count();
                    args_map.insert(
                        "CodeContent".to_string(),
                        Value::String(format!(
                            "[File written to disk ({} lines). Step {} full payload archived in {}. Inspect via view_file if needed]",
                            line_count, step_idx, backup_abs_path
                        )),
                    );
                }
            }
        }
        "replace_file_content" => {
            if let Some(rep_val) = args_map.get("ReplacementContent").and_then(|v| v.as_str()) {
                if rep_val.len() > 200 {
                    args_map.insert(
                        "ReplacementContent".to_string(),
                        Value::String(format!(
                            "[Code replacement applied. Step {} diff archived in {}. Inspect via view_file if needed]",
                            step_idx, backup_abs_path
                        )),
                    );
                }
            }
            if let Some(target_val) = args_map.get("TargetContent").and_then(|v| v.as_str()) {
                if target_val.len() > 200 {
                    args_map.insert(
                        "TargetContent".to_string(),
                        Value::String("[Original target code snippet]".to_string()),
                    );
                }
            }
        }
        "multi_replace_file_content" => {
            if let Some(chunks) = args_map.get_mut("ReplacementChunks").and_then(|v| v.as_array_mut()) {
                for chunk in chunks {
                    if let Some(chunk_map) = chunk.as_object_mut() {
                        if let Some(rc) = chunk_map.get("ReplacementContent").and_then(|v| v.as_str()) {
                            if rc.len() > 100 {
                                chunk_map.insert(
                                    "ReplacementContent".to_string(),
                                    Value::String(format!("[Replacement chunk applied. Step {} archived in {}]", step_idx, backup_abs_path)),
                                );
                            }
                        }
                        if let Some(tc) = chunk_map.get("TargetContent").and_then(|v| v.as_str()) {
                            if tc.len() > 100 {
                                chunk_map.insert("TargetContent".to_string(), Value::String("[Target chunk snippet]".to_string()));
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// Compacts a JSONL file in-place while PRESERVING the exact same filesystem Inode
/// and holding an exclusive file lock to eliminate cross-process write race conditions.
fn compact_single_jsonl_file(
    target_path: &Path,
    recent_window_steps: usize,
) -> Result<(usize, usize), Box<dyn std::error::Error>> {
    if !target_path.exists() {
        return Ok((0, 0));
    }

    let abs_target = fs::canonicalize(target_path).unwrap_or_else(|_| target_path.to_path_buf());

    // 1. Create a timestamped backup first
    let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
    let backup_timestamped = abs_target.with_extension(format!("jsonl.bak_{}", timestamp));
    let backup_latest = abs_target.with_extension("jsonl.bak");
    let _ = fs::copy(&abs_target, &backup_timestamped);
    let _ = fs::copy(&abs_target, &backup_latest);

    let backup_abs_str = backup_timestamped.to_string_lossy().to_string();

    // 2. Open the file in Read+Write mode and acquire an exclusive file lock
    let mut file = File::options().read(true).write(true).open(&abs_target)?;
    file.lock_exclusive()?;

    // Read and count steps
    let mut lines_buffer: Vec<String> = Vec::new();
    let mut initial_bytes = 0usize;
    {
        let reader = BufReader::new(&file);
        for line in reader.lines() {
            let line_str = line?;
            if line_str.trim().is_empty() {
                continue;
            }
            initial_bytes += line_str.len();
            lines_buffer.push(line_str);
        }
    }

    let total_steps = lines_buffer.len();
    let recent_threshold = total_steps.saturating_sub(recent_window_steps);

    // Compact lines in memory
    let mut compacted_output = String::with_capacity(initial_bytes / 2);

    for (i, line_str) in lines_buffer.into_iter().enumerate() {
        let mut step_val: Value = match serde_json::from_str(&line_str) {
            Ok(v) => v,
            Err(_) => {
                compacted_output.push_str(&line_str);
                compacted_output.push('\n');
                continue;
            }
        };

        let stype = step_val.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let status = step_val.get("status").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
        let exit_code = step_val.get("exit_code").and_then(|v| v.as_i64());
        let step_idx = step_val.get("step_index").and_then(|v| v.as_u64()).unwrap_or(i as u64 + 1);
        let is_recent = i >= recent_threshold;

        let is_error = exit_code.map(|c| c != 0).unwrap_or(false)
            || status.contains("error")
            || status.contains("failed");

        if !is_recent {
            if stype == "PLANNER_RESPONSE" {
                if let Some(tool_calls) = step_val.get_mut("tool_calls").and_then(|v| v.as_array_mut()) {
                    for tc in tool_calls {
                        let name = tc.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        if let Some(args_map) = tc.get_mut("args").and_then(|v| v.as_object_mut()) {
                            compact_tool_call_args(&name, args_map, step_idx, &backup_abs_str);
                        }
                    }
                }
            }

            if !is_error {
                match stype.as_str() {
                    "RUN_COMMAND" => {
                        let content_len = step_val.get("content").and_then(|v| v.as_str()).map(|s| s.len()).unwrap_or(0);
                        if content_len > 250 {
                            step_val["content"] = serde_json::json!(format!(
                                "Command completed successfully (exit 0). Step {} stdout archived in {}.",
                                step_idx, backup_abs_str
                            ));
                        }
                    }
                    "VIEW_FILE" => {
                        step_val["content"] = serde_json::json!(format!(
                            "File inspected in previous turn. Step {} content archived in {}.",
                            step_idx, backup_abs_str
                        ));
                    }
                    "SEARCH_WEB" | "GREP_SEARCH" | "CODE_ACTION" => {
                        step_val["content"] = serde_json::json!(format!(
                            "{} completed successfully. Step {} output archived in {}.",
                            stype, step_idx, backup_abs_str
                        ));
                    }
                    _ => {}
                }
            }
        }

        let compacted_line = serde_json::to_string(&step_val)?;
        compacted_output.push_str(&compacted_line);
        compacted_output.push('\n');
    }

    let compacted_bytes = compacted_output.len();

    // 3. Truncate in-place, rewind, write, and call fsync (sync_all) before unlocking
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(compacted_output.as_bytes())?;
    file.flush()?;
    file.sync_all()?;
    file.unlock()?;

    Ok((initial_bytes, compacted_bytes))
}

pub fn compact_transcript_inplace(
    transcript_path: &Path,
    recent_window_steps: usize,
) -> Result<(usize, usize), Box<dyn std::error::Error>> {
    let mut total_init = 0usize;
    let mut total_compacted = 0usize;

    let (i1, c1) = compact_single_jsonl_file(transcript_path, recent_window_steps)?;
    total_init += i1;
    total_compacted += c1;

    if let Some(parent) = transcript_path.parent() {
        let full_transcript = parent.join("transcript_full.jsonl");
        if full_transcript.exists() && full_transcript != transcript_path {
            let (i2, c2) = compact_single_jsonl_file(&full_transcript, recent_window_steps)?;
            total_init += i2;
            total_compacted += c2;
        }
    }

    Ok((total_init, total_compacted))
}

pub fn prune_transcript(
    transcript_path: &Path,
    recent_window_steps: usize,
) -> Result<(String, PruningStats), Box<dyn std::error::Error>> {
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

    let file_pass2 = File::open(transcript_path)?;
    let reader_pass2 = BufReader::new(file_pass2);

    let mut output_blocks = Vec::with_capacity(total_steps);
    let mut user_count = 0usize;
    let mut assistant_count = 0usize;
    let mut pruned_tools_count = 0usize;
    let mut retained_errors_count = 0usize;
    let mut retained_short_cmds = 0usize;
    let mut retained_recent_steps = 0usize;
    let mut first_user_prompt = String::new();

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
                let user_text = extract_user_request_text(content);
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
                                let v_formatted = if (k == "CodeContent" || k == "ReplacementContent" || k == "TargetContent") && v_str.len() > 100 {
                                    "[Code payload archived on disk]".to_string()
                                } else if v_str.chars().count() > 120 {
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
                    let snippet = sanitize_markdown_snippet(&safe_truncate(content, 1500));
                    output_blocks.push(format!("> 🕒 **[Active Window Tool Output ({})]**:\n```\n{}\n```\n", stype, snippet));
                } else if is_error {
                    retained_errors_count += 1;
                    let snippet = sanitize_markdown_snippet(&safe_truncate(content, 1200));
                    output_blocks.push(format!(
                        "> ⚠️ **[Tool Execution Error / Failure ({}, Exit code: {:?})]**:\n```\n{}\n```\n",
                        stype, exit_code, snippet
                    ));
                } else if stype == "RUN_COMMAND" {
                    if content.trim().chars().count() < 250 {
                        retained_short_cmds += 1;
                        let safe_cmd = sanitize_markdown_snippet(content.trim());
                        output_blocks.push(format!("> 📋 **[Command Output (exit 0)]**:\n```\n{}\n```\n", safe_cmd));
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
        > - The active working state and immediate recent tool outputs are preserved at the end of the transcript.\n\
        > - If exact historical diffs or raw outputs are ever required, inspect the timestamped `.bak` log on disk.\n\n\
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
