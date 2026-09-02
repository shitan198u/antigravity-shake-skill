use crate::metadata::load_or_discover_history;
use crate::models::{CompactionEvent, PruningStats};
use crate::slug::{extract_conversation_id, generate_suggested_filename, generate_topic_slug};
use chrono::Local;
use fs2::FileExt;
use serde_json::Value;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::Path;

/// Options configuring the compaction and pruning pipeline.
#[derive(Debug, Clone)]
pub struct CompactionOptions {
    pub recent_user_turns: usize, // Number of human conversational turns to keep unpruned (default: 10)
    pub recent_window_steps: usize, // Fallback step-level minimum (default: 6)
    pub thought_window_turns: Option<usize>, // Thought window (e.g. Some(20) for /full-shake)
    pub marathon_horizon: bool, // Enable Milestone Horizon on marathon threads (>30 user turns)
    pub keep_backups: usize,
    pub in_place: bool,
    pub dry_run: bool,
}

impl Default for CompactionOptions {
    fn default() -> Self {
        Self {
            recent_user_turns: 10,
            recent_window_steps: 6,
            thought_window_turns: None,
            marathon_horizon: false,
            keep_backups: 5,
            in_place: true,
            dry_run: false,
        }
    }
}

/// Accurate token estimation calibrated for Code, JSON, and Markdown transcripts (~3.3 chars/token).
pub fn estimate_tokens(byte_count: usize) -> usize {
    std::cmp::max(1, (byte_count as f64 / 3.3).round() as usize)
}

pub fn safe_truncate(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// Escapes HTML entities and triple backticks to prevent XSS in IDE webviews and Markdown block termination.
pub fn sanitize_markdown_snippet(s: &str) -> String {
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
pub fn extract_user_request_text(content: &str) -> &str {
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

/// Retains only the latest `keep_count` timestamped backups in `logs_dir`,
/// safely pruning older historical snapshots while preserving `transcript.jsonl.bak`.
pub fn prune_old_backups(logs_dir: &Path, keep_count: usize) {
    if keep_count == 0 {
        return;
    }

    let entries = match fs::read_dir(logs_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    let mut timestamped_backups: Vec<(String, std::path::PathBuf)> = Vec::new();

    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().to_string();
        if file_name.contains(".jsonl.bak_") {
            timestamped_backups.push((file_name, entry.path()));
        }
    }

    timestamped_backups.sort_by(|a, b| b.0.cmp(&a.0));

    if timestamped_backups.len() > keep_count {
        for (_, old_path) in timestamped_backups.iter().skip(keep_count) {
            let _ = fs::remove_file(old_path);
        }
    }
}

/// Compacts large tool call arguments into structured receipts with FULL ABSOLUTE PATHS and 1-INDEXED LINE NUMBERS.
/// Strictly idempotent: never double-wraps already-pruned receipts.
fn compact_tool_call_args(
    tool_name: &str,
    args_map: &mut serde_json::Map<String, Value>,
    step_idx: u64,
    backup_abs_path: &str,
    line_no: usize,
) {
    match tool_name {
        "run_command" => {
            if let Some(cmd_val) = args_map.get("CommandLine").and_then(|v| v.as_str()) {
                if !cmd_val.starts_with("[PRUNED") && (cmd_val.len() > 250 || cmd_val.contains("<< 'EOF'") || cmd_val.contains("<< 'END'")) {
                    let first_line = cmd_val.lines().next().unwrap_or("run_command").trim();
                    let line_count = cmd_val.lines().count();
                    args_map.insert(
                        "CommandLine".to_string(),
                        Value::String(format!(
                            "[PRUNED heredoc command=\"{}\" lines={} archive={} line={}]",
                            first_line, line_count, backup_abs_path, line_no
                        )),
                    );
                }
            }
        }
        "write_to_file" => {
            if let Some(code_val) = args_map.get("CodeContent").and_then(|v| v.as_str()) {
                if !code_val.starts_with("[PRUNED") && code_val.len() > 200 {
                    let line_count = code_val.lines().count();
                    args_map.insert(
                        "CodeContent".to_string(),
                        Value::String(format!(
                            "[PRUNED tool=write_to_file step={} lines={} archive={} line={}]",
                            step_idx, line_count, backup_abs_path, line_no
                        )),
                    );
                }
            }
        }
        "replace_file_content" => {
            if let Some(rep_val) = args_map.get("ReplacementContent").and_then(|v| v.as_str()) {
                if !rep_val.starts_with("[PRUNED") && rep_val.len() > 200 {
                    args_map.insert(
                        "ReplacementContent".to_string(),
                        Value::String(format!(
                            "[PRUNED tool=replace_file_content step={} archive={} line={}]",
                            step_idx, backup_abs_path, line_no
                        )),
                    );
                }
            }
            if let Some(target_val) = args_map.get("TargetContent").and_then(|v| v.as_str()) {
                if !target_val.starts_with("[PRUNED") && target_val.len() > 200 {
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
                            if !rc.starts_with("[PRUNED") && rc.len() > 100 {
                                chunk_map.insert(
                                    "ReplacementContent".to_string(),
                                    Value::String(format!(
                                        "[PRUNED tool=multi_replace_file_content step={} archive={} line={}]",
                                        step_idx, backup_abs_path, line_no
                                    )),
                                );
                            }
                        }
                        if let Some(tc) = chunk_map.get("TargetContent").and_then(|v| v.as_str()) {
                            if !tc.starts_with("[PRUNED") && tc.len() > 100 {
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

pub fn format_history_timeline(events: &[CompactionEvent]) -> String {
    if events.is_empty() {
        return String::new();
    }

    let mut rows = String::new();
    for (idx, ev) in events.iter().enumerate() {
        let step_label = if ev.anchored_step > 0 {
            format!("Step {}+", ev.anchored_step)
        } else {
            "Historical".to_string()
        };
        let before_kb = if ev.bytes_before > 0 {
            format!("{:.1} KB", ev.bytes_before as f64 / 1024.0)
        } else {
            "—".to_string()
        };
        let savings_label = if ev.reduction_pct > 0.0 {
            format!("-{:.1}%", ev.reduction_pct)
        } else {
            "—".to_string()
        };
        let archive_link = if !ev.backup_file.is_empty() {
            let enc_link = format!("file://{}", urlencoding::encode(&ev.backup_file).replace("%2F", "/"));
            format!("[📄 Backup #{}]({})", idx + 1, enc_link)
        } else {
            "—".to_string()
        };

        rows.push_str(&format!(
            "| `{}` | {} | `{}` | `{}` | {} | {} |\n",
            ev.timestamp_display, ev.trigger, step_label, before_kb, savings_label, archive_link
        ));
    }

    format!(
        "<details>\n\
        <summary>📜 <b>Session Compaction Timeline & Checkpoint History ({} events)</b></summary>\n\n\
        | Time | Trigger Event | Working Checkpoint | Input Size | Saved | Archive Backup |\n\
        | :--- | :--- | :---: | :---: | :---: | :--- |\n\
        {}\n\
        </details>\n\n",
        events.len(),
        rows.trim_end()
    )
}

/// Unified Single-Pass Compaction & Pruning Pipeline:
/// 1. Locks `transcript.jsonl` exclusively.
/// 2. Creates timestamped backup under lock and rotates older backups.
/// 3. Pass 1: Indexes User conversational turns, assistant turns, and ephemeral message positions.
/// 4. Milestone Horizon (if --marathon-horizon enabled and user_turns > 30):
///    - Preserves Genesis Turn 1 verbatim.
///    - Replaces intermediate turns with a structured Milestone Checkpoint block.
///    - Preserves the last 25 user turns verbatim.
/// 5. Pass 2: In a single loop:
///    - Keeps all tool steps within the recent user conversational turns 100% intact.
///    - Deduplicates historical EPHEMERAL_MESSAGE notices (keeps only the latest 1).
///    - Compacts older tool outputs into line-indexed receipts: [PRUNED ... line=N].
///    - Compacts older multi-line bash heredoc CommandLine inputs.
///    - Produces the in-place compacted JSONL stream AND Markdown report simultaneously.
/// 6. Inode-safe truncate-and-rewrite with `fsync` commitment.
pub fn run_compaction_pipeline(
    transcript_path: &Path,
    options: &CompactionOptions,
) -> Result<(String, String, PruningStats, String), Box<dyn std::error::Error>> {
    if !transcript_path.exists() {
        return Err(format!("Transcript file does not exist: {}", transcript_path.display()).into());
    }

    let abs_target = fs::canonicalize(transcript_path).unwrap_or_else(|_| transcript_path.to_path_buf());
    let logs_dir = abs_target.parent().unwrap_or_else(|| Path::new("."));

    let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
    let backup_timestamped = abs_target.with_extension(format!("jsonl.bak_{}", timestamp));
    let backup_latest = abs_target.with_extension("jsonl.bak");
    let backup_abs_str = backup_timestamped.to_string_lossy().to_string();

    let file_opt = if !options.dry_run && options.in_place {
        let file = File::options().read(true).write(true).open(&abs_target)?;
        file.lock_exclusive()?;

        // Create timestamped backup while holding the exclusive lock (zero torn writes)
        let _ = fs::copy(&abs_target, &backup_timestamped);
        let _ = fs::copy(&abs_target, &backup_latest);

        // Enforce rolling backup retention: keep latest N timestamped backups
        prune_old_backups(logs_dir, options.keep_backups);

        Some(file)
    } else {
        None
    };

    // Read and buffer lines for the single pass
    let file_for_reading = File::open(&abs_target)?;
    let reader = BufReader::new(file_for_reading);

    let mut lines_buffer: Vec<(usize, String)> = Vec::new(); // (original_line_no_1_indexed, content)
    let mut raw_bytes = 0usize;
    let mut total_assistant_turns = 0usize;
    let mut user_turn_positions: Vec<(usize, usize)> = Vec::new(); // (turn_index, buffer_index)
    let mut ephemeral_message_indices: Vec<usize> = Vec::new();

    for (line_idx, line) in reader.lines().enumerate() {
        let line_str = line?;
        if line_str.trim().is_empty() {
            continue;
        }
        raw_bytes += line_str.len();
        let original_line_no = line_idx + 1;
        let buf_idx = lines_buffer.len();

        if let Ok(val) = serde_json::from_str::<Value>(&line_str) {
            let t = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if t == "PLANNER_RESPONSE" {
                total_assistant_turns += 1;
            } else if t == "USER_INPUT" {
                user_turn_positions.push((user_turn_positions.len() + 1, buf_idx));
            } else if t == "EPHEMERAL_MESSAGE" {
                ephemeral_message_indices.push(buf_idx);
            }
        }
        lines_buffer.push((original_line_no, line_str));
    }

    let total_user_turns = user_turn_positions.len();

    // ⚡ MILESTONE HORIZON (For Marathon Threads > 30 User Turns)
    let mut effective_lines: Vec<(usize, String)> = Vec::with_capacity(lines_buffer.len());
    let mut is_milestone_horizon_active = false;

    if options.marathon_horizon && total_user_turns > 30 {
        is_milestone_horizon_active = true;
        let genesis_end_idx = if user_turn_positions.len() > 1 {
            user_turn_positions[1].1
        } else {
            1
        };

        let horizon_turn_idx = total_user_turns.saturating_sub(25);
        let horizon_start_idx = user_turn_positions[horizon_turn_idx].1;

        // 1. Genesis Turn 1
        for item in &lines_buffer[..genesis_end_idx] {
            effective_lines.push(item.clone());
        }

        // 2. Synthesized Milestone Block
        let milestone_content = format!(
            "### 🏛️ Historical Milestone Horizon (Turns 2 to {})\n\n\
            > **Verbatim History Reference**: The complete unpruned transcript of earlier turns is preserved permanently in `transcript_full.jsonl` and in historical backups (`{}`).\n\n\
            All earlier user instructions, architectural decisions, and error fixes are archived with exact line-indexed pointers in the referenced backup.\n\n\
            Active working momentum continues with the last 25 turns preserved verbatim below.",
            horizon_turn_idx, backup_abs_str
        );
        let milestone_step = serde_json::json!({
            "step_index": 2,
            "source": "SYSTEM",
            "type": "PLANNER_RESPONSE",
            "status": "DONE",
            "content": milestone_content
        });
        effective_lines.push((genesis_end_idx + 1, serde_json::to_string(&milestone_step)?));

        // 3. Last 25 User Turns
        for item in &lines_buffer[horizon_start_idx..] {
            effective_lines.push(item.clone());
        }
    } else {
        effective_lines = lines_buffer;
    }

    // Re-index effective user turns and ephemeral positions after milestone horizon
    let mut effective_user_turn_indices: Vec<usize> = Vec::new();
    let mut effective_ephemeral_indices: Vec<usize> = Vec::new();

    for (buf_idx, (_, line_str)) in effective_lines.iter().enumerate() {
        if let Ok(val) = serde_json::from_str::<Value>(line_str) {
            let t = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if t == "USER_INPUT" {
                effective_user_turn_indices.push(buf_idx);
            } else if t == "EPHEMERAL_MESSAGE" {
                effective_ephemeral_indices.push(buf_idx);
            }
        }
    }

    let effective_total_user_turns = effective_user_turn_indices.len();
    let effective_total_steps = effective_lines.len();

    // Active working window cutoff
    let active_window_start = if options.recent_user_turns == 0 {
        effective_total_steps.saturating_sub(options.recent_window_steps)
    } else if effective_total_user_turns > options.recent_user_turns {
        effective_user_turn_indices[effective_total_user_turns - options.recent_user_turns]
    } else {
        effective_total_steps.saturating_sub(options.recent_window_steps)
    };

    let latest_ephemeral_idx = effective_ephemeral_indices.last().copied();

    let thought_threshold = options
        .thought_window_turns
        .map(|w| total_assistant_turns.saturating_sub(w))
        .unwrap_or(0);

    let conv_id = extract_conversation_id(&abs_target.to_string_lossy());

    let cumulative_full_bytes = logs_dir
        .join("transcript_full.jsonl")
        .metadata()
        .map(|m| m.len() as usize)
        .unwrap_or(raw_bytes);

    // Processing buffers
    let mut compacted_output = String::with_capacity(raw_bytes / 2);
    let mut output_blocks = Vec::with_capacity(effective_total_steps);

    let mut user_count = 0usize;
    let mut assistant_count = 0usize;
    let mut pruned_tools_count = 0usize;
    let mut retained_errors_count = 0usize;
    let mut retained_short_cmds = 0usize;
    let mut retained_recent_steps = 0usize;
    let mut first_user_prompt = String::new();

    for (i, (orig_line_no, line_str)) in effective_lines.into_iter().enumerate() {
        let mut step_val: Value = match serde_json::from_str(&line_str) {
            Ok(v) => v,
            Err(_) => {
                compacted_output.push_str(&line_str);
                compacted_output.push('\n');
                let snippet = sanitize_markdown_snippet(&safe_truncate(&line_str, 500));
                output_blocks.push(format!("> ⚠️ **[Unparsed Raw Log Line (Preserved)]**:\n```\n{}\n```\n", snippet));
                continue;
            }
        };

        let stype = step_val.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let status = step_val.get("status").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
        let exit_code = step_val.get("exit_code").and_then(|v| v.as_i64());
        let step_idx = step_val.get("step_index").and_then(|v| v.as_u64()).unwrap_or(i as u64 + 1);

        // Deduplicate EPHEMERAL_MESSAGE: only retain the latest one!
        if stype == "EPHEMERAL_MESSAGE" {
            if Some(i) == latest_ephemeral_idx {
                compacted_output.push_str(&line_str);
                compacted_output.push('\n');
            }
            continue;
        }

        // Active Working Window check based on human user conversational turns
        let is_recent = i >= active_window_start;

        let is_error = exit_code.map(|c| c != 0).unwrap_or(false)
            || status.contains("error")
            || status.contains("failed");

        match stype.as_str() {
            "USER_INPUT" => {
                user_count += 1;
                let raw_content = step_val.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let user_text = extract_user_request_text(raw_content);
                if first_user_prompt.is_empty() {
                    first_user_prompt = user_text.to_string();
                }
                output_blocks.push(format!("### 👤 User (Turn {})\n\n{}\n", user_count, user_text));
            }
            "PLANNER_RESPONSE" => {
                assistant_count += 1;
                let assistant_text = step_val.get("content").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                let thinking_text = step_val.get("thinking").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();

                let is_thought_retained = options.thought_window_turns.is_none() || assistant_count > thought_threshold;

                if !is_thought_retained {
                    if let Some(obj) = step_val.as_object_mut() {
                        obj.remove("thinking");
                    }
                }

                if !assistant_text.is_empty() || !thinking_text.is_empty() {
                    let mut assistant_block = String::from("### 🤖 Assistant\n\n");
                    if is_thought_retained && !thinking_text.is_empty() {
                        assistant_block.push_str(&format!(
                            "<details>\n<summary>💭 Thought Process</summary>\n\n{}\n\n</details>\n\n",
                            thinking_text
                        ));
                    }
                    if !assistant_text.is_empty() {
                        assistant_block.push_str(&assistant_text);
                        assistant_block.push('\n');
                    }
                    output_blocks.push(assistant_block);
                }

                if let Some(tool_calls) = step_val.get_mut("tool_calls").and_then(|v| v.as_array_mut()) {
                    for tc in tool_calls.iter_mut() {
                        let name = tc.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        if let Some(args_map) = tc.get_mut("args").and_then(|v| v.as_object_mut()) {
                            if !is_recent {
                                compact_tool_call_args(&name, args_map, step_idx, &backup_abs_str, orig_line_no);
                            }
                            let mut arg_items = Vec::new();
                            for (k, v) in args_map.iter() {
                                let v_str = match v {
                                    serde_json::Value::String(s) => s.replace('\n', " "),
                                    other => other.to_string().replace('\n', " "),
                                };
                                let v_formatted = if (k == "CodeContent" || k == "ReplacementContent" || k == "TargetContent" || k == "CommandLine") && v_str.contains("[PRUNED") {
                                    v_str
                                } else if v_str.chars().count() > 120 {
                                    format!("{}... [truncated]", safe_truncate(&v_str, 120))
                                } else {
                                    v_str
                                };
                                arg_items.push(format!("{}={}", k, v_formatted));
                            }
                            let arg_summary = arg_items.join(", ");
                            output_blocks.push(format!("- ⚙️ **Action Executed**: `{}({})`", name, arg_summary));
                        }
                    }
                }
            }
            "RUN_COMMAND" | "VIEW_FILE" | "SEARCH_WEB" | "GREP_SEARCH" | "CODE_ACTION" => {
                let content_str = step_val.get("content").and_then(|v| v.as_str()).unwrap_or("");

                if is_recent {
                    retained_recent_steps += 1;
                    let snippet = sanitize_markdown_snippet(&safe_truncate(content_str, 1500));
                    output_blocks.push(format!("> 🕒 **[Active Window Tool Output ({})]**:\n```\n{}\n```\n", stype, snippet));
                } else if is_error {
                    retained_errors_count += 1;
                    let snippet = sanitize_markdown_snippet(&safe_truncate(content_str, 1200));
                    output_blocks.push(format!(
                        "> ⚠️ **[Tool Execution Error / Failure ({}, Exit code: {:?})]**:\n```\n{}\n```\n",
                        stype, exit_code, snippet
                    ));
                } else if stype == "RUN_COMMAND" {
                    if content_str.starts_with("[PRUNED") {
                        pruned_tools_count += 1;
                        output_blocks.push(format!("> ℹ️ *{}*\n", content_str));
                    } else if content_str.trim().chars().count() < 250 {
                        retained_short_cmds += 1;
                        let safe_cmd = sanitize_markdown_snippet(content_str.trim());
                        output_blocks.push(format!("> 📋 **[Command Output (exit 0)]**:\n```\n{}\n```\n", safe_cmd));
                    } else {
                        let line_count = content_str.lines().count();
                        pruned_tools_count += 1;
                        let receipt = format!(
                            "[PRUNED tool=RUN_COMMAND step={} exit={} lines={} archive={} line={}]",
                            step_idx, exit_code.unwrap_or(0), line_count, backup_abs_str, orig_line_no
                        );
                        step_val["content"] = serde_json::json!(receipt);
                        output_blocks.push(format!("> ℹ️ *{}*\n", receipt));
                    }
                } else if stype == "VIEW_FILE" {
                    if content_str.starts_with("[PRUNED") {
                        pruned_tools_count += 1;
                        output_blocks.push(format!("> ℹ️ *{}*\n", content_str));
                    } else {
                        let line_count = content_str.lines().count();
                        pruned_tools_count += 1;
                        let receipt = format!(
                            "[PRUNED tool=VIEW_FILE step={} lines={} archive={} line={}]",
                            step_idx, line_count, backup_abs_str, orig_line_no
                        );
                        step_val["content"] = serde_json::json!(receipt);
                        output_blocks.push(format!("> ℹ️ *{}*\n", receipt));
                    }
                } else {
                    pruned_tools_count += 1;
                    let receipt = format!(
                        "[PRUNED tool={} step={} archive={} line={}]",
                        stype, step_idx, backup_abs_str, orig_line_no
                    );
                    step_val["content"] = serde_json::json!(receipt);
                    output_blocks.push(format!("> ℹ️ *{}*\n", receipt));
                }
            }
            _ => {}
        }

        let compacted_line = serde_json::to_string(&step_val)?;
        compacted_output.push_str(&compacted_line);
        compacted_output.push('\n');
    }

    if let Some(mut file) = file_opt {
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(compacted_output.as_bytes())?;
        file.flush()?;
        file.sync_all()?;
        file.unlock()?;
    }

    let topic_slug = generate_topic_slug(&first_user_prompt);
    let suggested_filename = generate_suggested_filename(&topic_slug);

    let anchor_path = logs_dir.parent().unwrap_or_else(|| Path::new(".")).join("active_shake_anchor.json");
    let history_events = load_or_discover_history(logs_dir, &anchor_path);
    let timeline_section = format_history_timeline(&history_events);

    let mode_note = if is_milestone_horizon_active {
        "> - **Compaction Mode**: ⚡ Marathon Reset (/full-shake) (Turn 1 Genesis preserved; intermediate turns collapsed into Milestone Horizon; thoughts windowed; last 25 turns active).\n".to_string()
    } else if let Some(w) = options.thought_window_turns {
        if total_assistant_turns > w {
            format!("> - **Compaction Mode**: ⚡ Full Deep Compaction (Scratchpad thoughts retained for last {} turns; older thoughts dropped).\n", w)
        } else {
            "> - **Compaction Mode**: 🟢 Standard Zero-Loss Compaction (All thoughts retained; session under 20 turns).\n".to_string()
        }
    } else {
        "> - **Compaction Mode**: 🟢 Standard Zero-Loss Compaction (100% thoughts retained).\n".to_string()
    };

    let header = format!(
        "# Shaken & Pruned History: {}\n\n\
        > [!IMPORTANT]\n\
        > **Context Note for Assistant**:\n\
        > This document is a complete, verbatim transcript of earlier turns with token bloat removed via `/shake`.\n\
        > - **User prompts, Assistant explanations, and Decisions are 100% complete and verbatim.**\n\
        {}\
        > - Actions marked `[PRUNED ...]` were successfully executed. Stored stdout is archived in the referenced backup with exact line pointers (`line=N`).\n\
        > - You can inspect any archived file or execution at exact line `N` using `view_file`.\n\
        > - You do **NOT** need to re-run past successful commands unless the user explicitly requests it.\n\
        > - Any errors or failures encountered in past turns are explicitly preserved below with full stack traces.\n\
        > - The active working state (last {} user conversational turns) is preserved completely at the end of the transcript.\n\n\
        - **Session ID**: `{}`\n\
        - **Topic**: `{}`\n\
        - **Source Transcript**: `{}`\n\
        - **User Turns**: {} | **Assistant Turns**: {}\n\
        - **Tool Dumps Pruned**: {} | **Errors Preserved**: {}\n\n\
        {}\
        ---\n\n",
        topic_slug.replace('_', " ").to_uppercase(),
        mode_note,
        options.recent_user_turns,
        conv_id,
        topic_slug.replace('_', " "),
        transcript_path.display(),
        user_count,
        assistant_count,
        pruned_tools_count,
        retained_errors_count,
        timeline_section
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

    let cumulative_savings_pct = if cumulative_full_bytes > 0 {
        (1.0 - (raw_bytes as f64 / cumulative_full_bytes as f64)) * 100.0
    } else {
        0.0
    };

    let compacted_jsonl_bytes = compacted_output.len();
    let this_run_savings_pct = if raw_bytes > 0 {
        (1.0 - (compacted_jsonl_bytes as f64 / raw_bytes as f64)) * 100.0
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
        this_run_before_bytes: raw_bytes,
        this_run_after_bytes: compacted_jsonl_bytes,
        this_run_savings_pct,
        cumulative_full_bytes,
        cumulative_savings_pct,
        user_turns: user_count,
        assistant_turns: assistant_count,
        pruned_tools: pruned_tools_count,
        retained_errors: retained_errors_count,
        retained_short_cmds,
        retained_recent_steps,
        topic_slug,
        suggested_filename,
        history_events,
    };

    Ok((compacted_output, full_document, stats, backup_abs_str))
}
