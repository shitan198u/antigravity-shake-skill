use crate::atomic::{
    commit_staged_in_place_with_snapshot, recover_if_interrupted, set_user_only_permissions,
    stage_compacted_output, SnapshotFingerprint,
};
use crate::metadata::load_or_discover_history;
use crate::models::{CompactionEvent, PruningStats};
use crate::receipts::count_warnings;
use crate::slug::{extract_conversation_id, generate_suggested_filename, generate_topic_slug};
use fs2::FileExt;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::Instant;

// Explicit pruning, truncation, and retention thresholds
pub const SHORT_CMD_RETENTION_CHARS: usize = 250;
pub const TOOL_ARG_CODE_PRUNE_CHARS: usize = 200;
pub const TOOL_ARG_REPLACE_PRUNE_CHARS: usize = 200;
pub const TOOL_ARG_CHUNK_PRUNE_CHARS: usize = 100;
pub const TOOL_ARG_HEREDOC_PRUNE_CHARS: usize = 250;
pub const DISPLAY_ARG_TRUNCATE_CHARS: usize = 120;
pub const ACTIVE_TOOL_SNIPPET_CHARS: usize = 1500;
pub const ERROR_TOOL_SNIPPET_CHARS: usize = 1200;
pub const UNPARSED_LINE_SNIPPET_CHARS: usize = 500;

/// Options configuring the compaction and pruning pipeline.
#[derive(Debug, Clone)]
pub struct CompactionOptions {
    pub recent_user_turns: usize, // Number of human conversational turns to keep unpruned (default: 10)
    pub recent_tools_cap: usize,  // Maximum recent tool outputs to keep unpruned (default: 20)
    pub recent_errors_cap: usize, // Maximum recent tool calls to preserve raw errors (default: 30)
    pub recent_window_steps: usize, // Fallback step-level minimum (default: 6)
    pub thought_window_turns: Option<usize>, // Thought window (e.g. Some(20) for /full-shake)
    pub marathon_horizon: bool,   // Enable Milestone Horizon on marathon threads (>30 user turns)
    pub in_place: bool,
    pub dry_run: bool,
    pub redact_secrets: bool, // Redact API keys, tokens, and Authorization headers (P1-3)
    pub non_blocking_lock: bool, // Fail open immediately on lock contention (P1-2)
}

impl Default for CompactionOptions {
    fn default() -> Self {
        Self {
            recent_user_turns: 10,
            recent_tools_cap: 20,
            recent_errors_cap: 30,
            recent_window_steps: 6,
            thought_window_turns: None,
            marathon_horizon: false,
            in_place: true,
            dry_run: false,
            redact_secrets: false,
            non_blocking_lock: false,
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

/// Purges all redundant historical timestamped `.bak_*` files in `logs_dir`,
/// reclaiming disk space while maintaining the single atomic `transcript.jsonl.bak`.
pub fn purge_legacy_timestamped_backups(logs_dir: &Path) {
    if let Ok(entries) = fs::read_dir(logs_dir) {
        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.starts_with("transcript.jsonl.bak_") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

/// Redacts sensitive credentials (GitHub tokens, AWS keys, Bearer tokens, private keys) from text (P1-3).
pub fn redact_secrets(text: &str) -> String {
    let pat_gh = regex::Regex::new(r"(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9_]{36}").unwrap();
    let pat_gh_pat = regex::Regex::new(r"github_pat_[A-Za-z0-9_]{82}").unwrap();
    let pat_aws = regex::Regex::new(r"\b(AKIA|ABIA|ACCA|ASIA)[0-9A-Z]{16}\b").unwrap();
    let pat_bearer = regex::Regex::new(r"(?i)\bBearer\s+[A-Za-z0-9_\-\.]{20,}").unwrap();
    let pat_auth = regex::Regex::new(r"(?i)Authorization:\s*[^\r\n]+").unwrap();
    let pat_key = regex::Regex::new(
        r"-----BEGIN (?:[A-Z ]+) PRIVATE KEY-----[\s\S]*?-----END (?:[A-Z ]+) PRIVATE KEY-----",
    )
    .unwrap();

    let s1 = pat_gh.replace_all(text, "[REDACTED_GH_TOKEN]");
    let s2 = pat_gh_pat.replace_all(&s1, "[REDACTED_GH_PAT]");
    let s3 = pat_aws.replace_all(&s2, "[REDACTED_AWS_KEY]");
    let s4 = pat_bearer.replace_all(&s3, "Bearer [REDACTED]");
    let s5 = pat_auth.replace_all(&s4, "Authorization: [REDACTED]");
    let s6 = pat_key.replace_all(&s5, "[REDACTED_PRIVATE_KEY]");
    s6.to_string()
}

/// Builds an O(1) step_index -> 1-indexed line number lookup map for `transcript_full.jsonl`.
///
/// Uses a sidecar index cache (`transcript_full.index.json`) keyed by file
/// size + mtime so marathon archives are not fully re-parsed on every
/// compaction (P1-4). Falls back to a full scan when the cache is missing,
/// stale, or corrupt.
pub fn index_master_full_transcript(full_transcript_path: &Path) -> HashMap<u64, usize> {
    if let Some(cached) = load_cached_master_index(full_transcript_path) {
        return cached;
    }
    let map = scan_master_full_transcript(full_transcript_path);
    store_cached_master_index(full_transcript_path, &map);
    map
}

fn master_index_cache_path(full_transcript_path: &Path) -> std::path::PathBuf {
    let fname = full_transcript_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    full_transcript_path.with_file_name(format!("{}.index.json", fname))
}

fn load_cached_master_index(full_transcript_path: &Path) -> Option<HashMap<u64, usize>> {
    let meta = fs::metadata(full_transcript_path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as u64;
    let cache_path = master_index_cache_path(full_transcript_path);
    let raw = fs::read_to_string(&cache_path).ok()?;
    let val: Value = serde_json::from_str(&raw).ok()?;
    if val.get("size").and_then(|v| v.as_u64()) != Some(meta.len()) {
        return None;
    }
    if val.get("mtime").and_then(|v| v.as_u64()) != Some(mtime) {
        return None;
    }
    let entries = val.get("index")?.as_object()?;
    let mut map = HashMap::with_capacity(entries.len());
    for (k, v) in entries {
        if let (Ok(step), Some(line)) = (k.parse::<u64>(), v.as_u64()) {
            map.insert(step, line as usize);
        }
    }
    Some(map)
}

fn store_cached_master_index(full_transcript_path: &Path, map: &HashMap<u64, usize>) {
    let meta = match fs::metadata(full_transcript_path) {
        Ok(m) => m,
        Err(_) => return,
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    // Bound cache size: marathon archives with >200k steps skip caching to
    // avoid a multi-MB JSON sidecar on every run.
    if map.len() > 200_000 {
        return;
    }
    let index_obj: serde_json::Map<String, Value> = map
        .iter()
        .map(|(k, v)| (k.to_string(), Value::from(*v as u64)))
        .collect();
    let payload = serde_json::json!({
        "size": meta.len(),
        "mtime": mtime,
        "index": index_obj,
    });
    let cache_path = master_index_cache_path(full_transcript_path);
    if let Ok(tmp) = tempfile::Builder::new()
        .prefix(".shake_index_")
        .tempfile_in(
            full_transcript_path
                .parent()
                .unwrap_or_else(|| Path::new(".")),
        )
    {
        if tmp
            .as_file()
            .write_all(
                serde_json::to_string(&payload)
                    .unwrap_or_default()
                    .as_bytes(),
            )
            .is_ok()
        {
            let _ = tmp.persist(&cache_path);
        }
    }
}

fn scan_master_full_transcript(full_transcript_path: &Path) -> HashMap<u64, usize> {
    let mut map = HashMap::new();
    if let Ok(file) = File::open(full_transcript_path) {
        let reader = BufReader::new(file);
        for (line_idx, line) in reader.lines().enumerate() {
            if let Ok(line_str) = line {
                if let Ok(val) = serde_json::from_str::<Value>(&line_str) {
                    // Skip synthetic milestone records to prevent collision
                    if val
                        .get("synthetic")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                        || val
                            .get("is_milestone")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false)
                    {
                        continue;
                    }
                    if let Some(step_idx) = val.get("step_index").and_then(|v| v.as_u64()) {
                        // If duplicate step_index occurs, retain first occurrence to preserve historical provenance
                        map.entry(step_idx).or_insert(line_idx + 1);
                    }
                }
            }
        }
    }
    map
}

/// Compacts large tool call arguments into structured receipts with FULL ABSOLUTE PATHS and 1-INDEXED LINE NUMBERS.
/// Strictly idempotent: never double-wraps already-pruned receipts.
pub fn compact_tool_call_args(
    tool_name: &str,
    args_map: &mut serde_json::Map<String, Value>,
    step_idx: u64,
    backup_abs_path: &str,
    line_no: usize,
) {
    match tool_name {
        "run_command" => {
            if let Some(cmd_val) = args_map.get("CommandLine").and_then(|v| v.as_str()) {
                if !cmd_val.starts_with("[PRUNED")
                    && (cmd_val.len() > TOOL_ARG_HEREDOC_PRUNE_CHARS
                        || cmd_val.contains("<< 'EOF'")
                        || cmd_val.contains("<< 'END'"))
                {
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
                if !code_val.starts_with("[PRUNED") && code_val.len() > TOOL_ARG_CODE_PRUNE_CHARS {
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
                if !rep_val.starts_with("[PRUNED") && rep_val.len() > TOOL_ARG_REPLACE_PRUNE_CHARS {
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
                if !target_val.starts_with("[PRUNED")
                    && target_val.len() > TOOL_ARG_REPLACE_PRUNE_CHARS
                {
                    args_map.insert(
                        "TargetContent".to_string(),
                        Value::String("[Original target code snippet]".to_string()),
                    );
                }
            }
        }
        "multi_replace_file_content" => {
            if let Some(chunks) = args_map
                .get_mut("ReplacementChunks")
                .and_then(|v| v.as_array_mut())
            {
                for chunk in chunks {
                    if let Some(chunk_map) = chunk.as_object_mut() {
                        if let Some(rc) =
                            chunk_map.get("ReplacementContent").and_then(|v| v.as_str())
                        {
                            if !rc.starts_with("[PRUNED") && rc.len() > TOOL_ARG_CHUNK_PRUNE_CHARS {
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
                            if !tc.starts_with("[PRUNED") && tc.len() > TOOL_ARG_CHUNK_PRUNE_CHARS {
                                chunk_map.insert(
                                    "TargetContent".to_string(),
                                    Value::String("[Target chunk snippet]".to_string()),
                                );
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
            let enc_link = format!(
                "file://{}",
                urlencoding::encode(&ev.backup_file).replace("%2F", "/")
            );
            format!("[📄 Archive #{}]({})", idx + 1, enc_link)
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
        | Time | Trigger Event | Working Checkpoint | Input Size | Saved | Archive Link |\n\
        | :--- | :--- | :---: | :---: | :---: | :--- |\n\
        {}\n\
        </details>\n\n",
        events.len(),
        rows.trim_end()
    )
}

/// Unified Single Master Archive Compaction Pipeline:
/// 1. Locks `transcript.jsonl` exclusively.
/// 2. Creates single atomic crash fallback: `transcript.jsonl.bak`.
/// 3. Purges all legacy redundant `.bak_*` files to eliminate disk bloat.
/// 4. Maps receipts directly to `transcript_full.jsonl` (permanent zero dangling links).
/// 5. Inode-safe truncate-and-rewrite with `fsync` commitment.
// Copies bytes from an already open and locked file without opening secondary
// file handles to the same path (avoiding Windows ERROR_LOCK_VIOLATION / os error 33).
fn copy_from_locked_file(file: &mut File, dest_path: &Path) -> std::io::Result<u64> {
    file.seek(SeekFrom::Start(0))?;
    let mut dest = File::create(dest_path)?;
    let bytes = std::io::copy(file, &mut dest)?;
    dest.flush()?;
    dest.sync_all()?;
    file.seek(SeekFrom::Start(0))?;
    Ok(bytes)
}

/// Synchronizes the permanent master archive (`transcript_full.jsonl`) under exclusive lock
/// before in-place pruning. Any steps present in the live transcript that are not yet recorded
/// in the master archive are appended atomically, guaranteed with 0600 permissions and synced (P0-1).
fn sync_master_full_transcript(
    file: &mut File,
    full_transcript_path: &Path,
) -> Result<HashMap<u64, usize>, Box<dyn std::error::Error>> {
    if !full_transcript_path.exists() {
        copy_from_locked_file(file, full_transcript_path).map_err(|e| {
            format!(
                "Critical: Failed to initialize permanent master archive at {}: {}. Compaction aborted to protect data integrity.",
                full_transcript_path.display(), e
            )
        })?;
        set_user_only_permissions(full_transcript_path);
        let step_map = scan_master_full_transcript(full_transcript_path);
        store_cached_master_index(full_transcript_path, &step_map);
        return Ok(step_map);
    }

    let mut step_map = scan_master_full_transcript(full_transcript_path);
    let mut current_line_count = 0;
    if let Ok(f) = File::open(full_transcript_path) {
        let reader = BufReader::new(f);
        current_line_count = reader.lines().count();
    }

    file.seek(SeekFrom::Start(0))?;
    let reader = BufReader::new(&mut *file);
    let mut missing_lines: Vec<(u64, String)> = Vec::new();

    for line_res in reader.lines() {
        let line_str = line_res?;
        if line_str.trim().is_empty() {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<Value>(&line_str) {
            if val
                .get("synthetic")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
                || val
                    .get("is_milestone")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            {
                continue;
            }
            if let Some(step_idx) = val.get("step_index").and_then(|v| v.as_u64()) {
                if !step_map.contains_key(&step_idx) {
                    missing_lines.push((step_idx, line_str));
                }
            }
        }
    }

    file.seek(SeekFrom::Start(0))?;

    if !missing_lines.is_empty() {
        let mut full_file = File::options()
            .append(true)
            .open(full_transcript_path)
            .map_err(|e| {
                format!(
                    "Critical: Failed to open permanent master archive at {} for append: {}. Compaction aborted.",
                    full_transcript_path.display(), e
                )
            })?;

        for (step_idx, line_str) in missing_lines {
            current_line_count += 1;
            writeln!(full_file, "{}", line_str)?;
            step_map.insert(step_idx, current_line_count);
        }
        full_file.flush()?;
        full_file.sync_all()?;
        set_user_only_permissions(full_transcript_path);
        store_cached_master_index(full_transcript_path, &step_map);
    }

    Ok(step_map)
}

pub fn run_compaction_pipeline(
    transcript_path: &Path,
    options: &CompactionOptions,
) -> Result<(String, String, PruningStats, String), Box<dyn std::error::Error>> {
    let pipeline_start = Instant::now();

    // P0-2: Auto-recover from any previous interrupted compaction before checking existence
    if let Ok(Some(recovery_msg)) = recover_if_interrupted(transcript_path) {
        eprintln!("{}", recovery_msg);
    }

    if !transcript_path.exists() {
        return Err(format!(
            "Transcript file does not exist: {}",
            transcript_path.display()
        )
        .into());
    }

    let abs_target =
        fs::canonicalize(transcript_path).unwrap_or_else(|_| transcript_path.to_path_buf());
    let logs_dir = abs_target.parent().unwrap_or_else(|| Path::new("."));

    let full_transcript_path = logs_dir.join("transcript_full.jsonl");
    let backup_latest = abs_target.with_extension("jsonl.bak");

    let (mut file_opt, snapshot_fingerprint, master_archive_abs_str, master_step_to_line) =
        if !options.dry_run && options.in_place {
            let mut file = File::options().read(true).write(true).open(&abs_target)?;
            if options.non_blocking_lock {
                file.try_lock_exclusive().map_err(|e| {
                    format!(
                        "Lock contention: could not acquire exclusive lock immediately on '{}': {}",
                        abs_target.display(),
                        e
                    )
                })?;
            } else {
                file.lock_exclusive()?;
            }

            // P0-3: Record snapshot fingerprint while holding exclusive lock before reading
            let snap = SnapshotFingerprint::from_file(&file)?;

            // P0-1: Guarantee permanent master archive synchronization under exclusive lock
            let step_map = sync_master_full_transcript(&mut file, &full_transcript_path)?;

            // Mandatory fail-closed crash fallback while holding the exclusive lock
            copy_from_locked_file(&mut file, &backup_latest).map_err(|e| {
                format!(
                    "Critical: Failed to create atomic backup at {}: {}. Compaction aborted to prevent data loss.",
                    backup_latest.display(), e
                )
            })?;
            set_user_only_permissions(&backup_latest);

            // Verify backup byte size exactly matches before allowing any truncation
            let orig_len = file.metadata()?.len();
            let backup_len = fs::metadata(&backup_latest)?.len();
            if orig_len != backup_len {
                return Err(format!(
                    "Critical: Backup size mismatch (original {} bytes, backup {} bytes). Compaction aborted.",
                    orig_len, backup_len
                ).into());
            }

            // Purge legacy timestamped backups to reclaim disk space
            purge_legacy_timestamped_backups(logs_dir);

            let abs_str = fs::canonicalize(&full_transcript_path)
                .unwrap_or_else(|_| full_transcript_path.clone())
                .to_string_lossy()
                .to_string();

            (Some(file), Some(snap), abs_str, Some(step_map))
        } else {
            let (abs_str, step_map) = if full_transcript_path.exists() {
                let sm = index_master_full_transcript(&full_transcript_path);
                let s = fs::canonicalize(&full_transcript_path)
                    .unwrap_or_else(|_| full_transcript_path.clone())
                    .to_string_lossy()
                    .to_string();
                (s, Some(sm))
            } else {
                (backup_latest.to_string_lossy().to_string(), None)
            };
            (None, None, abs_str, step_map)
        };

    // Read and buffer lines for the single pass. Clone existing locked handle
    // to avoid secondary handle open on Windows while locked.
    let file_for_reading = match &mut file_opt {
        Some(f) => {
            f.seek(SeekFrom::Start(0))?;
            f.try_clone()?
        }
        None => File::open(&abs_target)?,
    };
    let reader = BufReader::new(file_for_reading);

    let mut lines_buffer: Vec<(usize, String)> = Vec::new();
    let mut raw_bytes = 0usize;
    let mut user_turn_positions: Vec<(usize, usize)> = Vec::new();

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
            if t == "USER_INPUT" {
                user_turn_positions.push((user_turn_positions.len() + 1, buf_idx));
            }
        }
        lines_buffer.push((original_line_no, line_str));
    }

    let total_user_turns = user_turn_positions.len();

    // ⚡ MILESTONE HORIZON (For Marathon Threads > 30 User Turns)
    let mut effective_lines: Vec<(usize, String)> = Vec::with_capacity(lines_buffer.len());
    let mut is_milestone_horizon_active = false;
    let mut genesis_end_idx = 0usize;

    if options.marathon_horizon && total_user_turns > 30 {
        is_milestone_horizon_active = true;
        genesis_end_idx = if user_turn_positions.len() > 1 {
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
            > **Verbatim History Reference**: The complete unpruned transcript of earlier turns is preserved permanently in `transcript_full.jsonl`.\n\n\
            All earlier user instructions, architectural decisions, and error fixes are archived with exact line-indexed pointers in the permanent master archive (`{}`).\n\n\
            Active working momentum continues with the last 25 turns preserved verbatim below.",
            horizon_turn_idx, master_archive_abs_str
        );
        let milestone_step = serde_json::json!({
            "source": "SYSTEM",
            "type": "PLANNER_RESPONSE",
            "status": "DONE",
            "content": milestone_content,
            "is_milestone": true,
            "synthetic": true
        });
        effective_lines.push((genesis_end_idx + 1, serde_json::to_string(&milestone_step)?));

        // 3. Last 25 User Turns
        for item in &lines_buffer[horizon_start_idx..] {
            effective_lines.push(item.clone());
        }
    } else {
        effective_lines = lines_buffer;
    }

    // Re-index effective user turns, assistant turns, ephemeral positions, and tool step positions after milestone horizon
    let mut effective_user_turn_indices: Vec<usize> = Vec::new();
    let mut effective_shake_ephemeral_indices: Vec<usize> = Vec::new();
    let mut effective_tool_indices: Vec<usize> = Vec::new();
    let mut effective_assistant_turns = 0usize;

    let is_shake_ephemeral = |val: &Value| -> bool {
        if let Some(content) = val.get("content").and_then(|v| v.as_str()) {
            content.contains("Context compacted via /shake")
                || content.contains("Context auto-compacted via /shake")
                || content.contains("active_shake_anchor.json")
                || content.contains("HOOK_NOTICE")
                || content.contains("ANCHOR_NOTICE")
        } else {
            false
        }
    };

    for (buf_idx, (_, line_str)) in effective_lines.iter().enumerate() {
        if let Ok(val) = serde_json::from_str::<Value>(line_str) {
            let t = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if t == "USER_INPUT" {
                effective_user_turn_indices.push(buf_idx);
            } else if t == "PLANNER_RESPONSE" {
                effective_assistant_turns += 1;
            } else if t == "EPHEMERAL_MESSAGE" {
                if is_shake_ephemeral(&val) {
                    effective_shake_ephemeral_indices.push(buf_idx);
                }
            } else if matches!(
                t,
                "RUN_COMMAND" | "VIEW_FILE" | "SEARCH_WEB" | "GREP_SEARCH" | "CODE_ACTION"
            ) {
                effective_tool_indices.push(buf_idx);
            }
        }
    }

    let latest_shake_ephemeral_idx = effective_shake_ephemeral_indices.last().copied();

    let effective_total_user_turns = effective_user_turn_indices.len();
    let effective_total_steps = effective_lines.len();

    // Active working window cutoff (Human Conversational Horizon)
    let active_window_start = if options.recent_user_turns > 0 {
        if effective_total_user_turns > options.recent_user_turns {
            effective_user_turn_indices[effective_total_user_turns - options.recent_user_turns]
        } else {
            0 // When total user turns <= 10, all user turns are recent!
        }
    } else {
        // User explicitly set --recent-user-turns 0: use step-based fallback window
        effective_total_steps.saturating_sub(options.recent_window_steps)
    };

    // Tool execution cap cutoff (Maximum recent tool outputs to keep unpruned)
    let tool_cutoff_idx = if options.recent_tools_cap > 0
        && effective_tool_indices.len() > options.recent_tools_cap
    {
        effective_tool_indices[effective_tool_indices.len() - options.recent_tools_cap]
    } else {
        0
    };

    // Error retention cap cutoff (Maximum recent tool calls to preserve raw errors: default 30)
    let error_cutoff_idx = if options.recent_errors_cap > 0
        && effective_tool_indices.len() > options.recent_errors_cap
    {
        effective_tool_indices[effective_tool_indices.len() - options.recent_errors_cap]
    } else {
        0
    };

    let thought_threshold = options
        .thought_window_turns
        .map(|w| effective_assistant_turns.saturating_sub(w))
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
    let mut generated_json_lines: Vec<String> = Vec::with_capacity(effective_total_steps);

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
                let snippet = sanitize_markdown_snippet(&safe_truncate(
                    &line_str,
                    UNPARSED_LINE_SNIPPET_CHARS,
                ));
                output_blocks.push(format!(
                    "> ⚠️ **[Unparsed Raw Log Line (Preserved)]**:\n```\n{}\n```\n",
                    snippet
                ));
                continue;
            }
        };

        let stype = step_val
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let status = step_val
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        let exit_code = step_val.get("exit_code").and_then(|v| v.as_i64());
        let step_idx = step_val
            .get("step_index")
            .and_then(|v| v.as_u64())
            .unwrap_or(i as u64 + 1);

        // Deduplicate EPHEMERAL_MESSAGE: only deduplicate shake-related notices (P2-5)
        if stype == "EPHEMERAL_MESSAGE" {
            if is_shake_ephemeral(&step_val) {
                if Some(i) == latest_shake_ephemeral_idx {
                    compacted_output.push_str(&line_str);
                    compacted_output.push('\n');
                }
            } else {
                // Non-shake ephemeral messages are preserved verbatim
                compacted_output.push_str(&line_str);
                compacted_output.push('\n');
            }
            continue;
        }

        // Active Working Window check based on human user conversational turns
        let is_recent = i >= active_window_start;
        let is_recent_tool = is_recent && (i >= tool_cutoff_idx);

        let is_error = exit_code.map(|c| c != 0).unwrap_or(false)
            || status.contains("error")
            || status.contains("failed");
        let is_recent_error = is_error && options.recent_errors_cap > 0 && (i >= error_cutoff_idx);

        let has_explicit_step_idx = step_val
            .get("step_index")
            .and_then(|v| v.as_u64())
            .is_some();
        let is_synthetic = step_val
            .get("is_synthetic")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
            || step_val
                .get("is_milestone")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

        // Exact line number in the master archive (transcript_full.jsonl)
        let resolved_line_no = if let Some(m) = &master_step_to_line {
            match m.get(&step_idx).copied() {
                Some(l) => l,
                None if options.dry_run || !has_explicit_step_idx || is_synthetic => orig_line_no,
                None => {
                    return Err(format!(
                        "Critical integrity failure: step {} cannot be resolved in master archive '{}'. Refusing to emit unresolvable receipt.",
                        step_idx,
                        master_archive_abs_str
                    ).into());
                }
            }
        } else {
            orig_line_no
        };

        match stype.as_str() {
            "USER_INPUT" => {
                user_count += 1;
                let raw_content = step_val
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let user_text = extract_user_request_text(raw_content);
                if first_user_prompt.is_empty() {
                    first_user_prompt = if options.redact_secrets {
                        redact_secrets(user_text)
                    } else {
                        user_text.to_string()
                    };
                }
                let user_display = if options.redact_secrets {
                    redact_secrets(user_text)
                } else {
                    user_text.to_string()
                };
                output_blocks.push(format!(
                    "### 👤 User (Turn {})\n\n{}\n",
                    user_count, user_display
                ));
            }
            "PLANNER_RESPONSE" => {
                assistant_count += 1;
                let assistant_text = step_val
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let thinking_text = step_val
                    .get("thinking")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();

                let is_genesis = is_milestone_horizon_active && i < genesis_end_idx;
                let is_thought_retained = is_genesis
                    || options.thought_window_turns.is_none()
                    || assistant_count > thought_threshold;

                if !is_thought_retained {
                    if let Some(obj) = step_val.as_object_mut() {
                        obj.remove("thinking");
                    }
                }

                if !assistant_text.is_empty() || !thinking_text.is_empty() {
                    let mut assistant_block = String::from("### 🤖 Assistant\n\n");
                    let clean_thinking = if options.redact_secrets {
                        redact_secrets(&thinking_text)
                    } else {
                        thinking_text
                    };
                    let clean_assistant = if options.redact_secrets {
                        redact_secrets(&assistant_text)
                    } else {
                        assistant_text
                    };
                    if is_thought_retained && !clean_thinking.is_empty() {
                        assistant_block.push_str(&format!(
                            "<details>\n<summary>💭 Thought Process</summary>\n\n{}\n\n</details>\n\n",
                            clean_thinking
                        ));
                    }
                    if !clean_assistant.is_empty() {
                        assistant_block.push_str(&clean_assistant);
                        assistant_block.push('\n');
                    }
                    output_blocks.push(assistant_block);
                }

                if let Some(tool_calls) = step_val
                    .get_mut("tool_calls")
                    .and_then(|v| v.as_array_mut())
                {
                    for tc in tool_calls.iter_mut() {
                        let name = tc
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        if let Some(args_map) = tc.get_mut("args").and_then(|v| v.as_object_mut()) {
                            if !is_recent_tool {
                                compact_tool_call_args(
                                    &name,
                                    args_map,
                                    step_idx,
                                    &master_archive_abs_str,
                                    resolved_line_no,
                                );
                            }
                            let mut arg_items = Vec::new();
                            for (k, v) in args_map.iter() {
                                let v_str = match v {
                                    serde_json::Value::String(s) => s.replace('\n', " "),
                                    other => other.to_string().replace('\n', " "),
                                };
                                let v_formatted = if (k == "CodeContent"
                                    || k == "ReplacementContent"
                                    || k == "TargetContent"
                                    || k == "CommandLine")
                                    && v_str.contains("[PRUNED")
                                {
                                    v_str
                                } else if v_str.chars().count() > DISPLAY_ARG_TRUNCATE_CHARS {
                                    format!(
                                        "{}... [truncated]",
                                        safe_truncate(&v_str, DISPLAY_ARG_TRUNCATE_CHARS)
                                    )
                                } else {
                                    v_str
                                };
                                arg_items.push(format!("{}={}", k, v_formatted));
                            }
                            let arg_summary = arg_items.join(", ");
                            output_blocks.push(format!(
                                "- ⚙️ **Action Executed**: `{}({})`",
                                name, arg_summary
                            ));
                        }
                    }
                }
            }
            "RUN_COMMAND" | "VIEW_FILE" | "SEARCH_WEB" | "GREP_SEARCH" | "CODE_ACTION" => {
                let content_str = step_val
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if is_recent_tool {
                    retained_recent_steps += 1;
                    let snippet = sanitize_markdown_snippet(&safe_truncate(
                        content_str,
                        ACTIVE_TOOL_SNIPPET_CHARS,
                    ));
                    output_blocks.push(format!(
                        "> 🕒 **[Active Window Tool Output ({})]**:\n```\n{}\n```\n",
                        stype, snippet
                    ));
                } else if is_recent_error {
                    retained_errors_count += 1;
                    let snippet = sanitize_markdown_snippet(&safe_truncate(
                        content_str,
                        ERROR_TOOL_SNIPPET_CHARS,
                    ));
                    output_blocks.push(format!(
                        "> ⚠️ **[Tool Execution Error / Failure ({}, Exit code: {:?})]**:\n```\n{}\n```\n",
                        stype, exit_code, snippet
                    ));
                } else if stype == "RUN_COMMAND" {
                    if content_str.starts_with("[PRUNED") {
                        pruned_tools_count += 1;
                        output_blocks.push(format!("> ℹ️ *{}*\n", content_str));
                    } else if !is_error
                        && content_str.trim().chars().count() < SHORT_CMD_RETENTION_CHARS
                    {
                        retained_short_cmds += 1;
                        let safe_cmd = sanitize_markdown_snippet(content_str.trim());
                        output_blocks.push(format!(
                            "> 📋 **[Command Output (exit 0)]**:\n```\n{}\n```\n",
                            safe_cmd
                        ));
                    } else {
                        let line_count = content_str.lines().count();
                        pruned_tools_count += 1;

                        // Check for warnings in historical output to surface in receipt
                        let warn_count = count_warnings(content_str);
                        let warn_tag = if warn_count > 0 {
                            format!(" warnings={}", warn_count)
                        } else {
                            String::new()
                        };

                        let exit_str = if let Some(code) = exit_code {
                            code.to_string()
                        } else if is_error {
                            "failed".to_string()
                        } else {
                            "0".to_string()
                        };

                        let receipt = format!(
                            "[PRUNED tool=RUN_COMMAND step={} exit={}{} lines={} archive={} line={}]",
                            step_idx, exit_str, warn_tag, line_count, master_archive_abs_str, resolved_line_no
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
                            step_idx, line_count, master_archive_abs_str, resolved_line_no
                        );
                        step_val["content"] = serde_json::json!(receipt);
                        output_blocks.push(format!("> ℹ️ *{}*\n", receipt));
                    }
                } else {
                    pruned_tools_count += 1;
                    let receipt = format!(
                        "[PRUNED tool={} step={} archive={} line={}]",
                        stype, step_idx, master_archive_abs_str, resolved_line_no
                    );
                    step_val["content"] = serde_json::json!(receipt);
                    output_blocks.push(format!("> ℹ️ *{}*\n", receipt));
                }
            }
            _ => {}
        }

        let compacted_line = if options.redact_secrets {
            let line_str = serde_json::to_string(&step_val)?;
            redact_secrets(&line_str)
        } else {
            serde_json::to_string(&step_val)?
        };
        generated_json_lines.push(compacted_line.clone());
        compacted_output.push_str(&compacted_line);
        compacted_output.push('\n');
    }

    if let Some(mut file) = file_opt {
        // P0-3 near-atomic path: stage + verify BEFORE touching the original
        // inode, then commit in place with verified rollback (P0-1 / P0-2).
        // Any error below leaves `transcript.jsonl.bak` intact for restore.
        let staged = stage_compacted_output(&abs_target, &compacted_output)?;
        match commit_staged_in_place_with_snapshot(
            &mut file,
            &abs_target,
            &backup_latest,
            &staged,
            &generated_json_lines,
            snapshot_fingerprint.as_ref(),
        ) {
            Ok(()) => {
                let _ = fs2::FileExt::unlock(&file);
            }
            Err(e) => {
                let _ = fs2::FileExt::unlock(&file);
                return Err(e);
            }
        }
    }

    let topic_slug = generate_topic_slug(&first_user_prompt);
    let suggested_filename = generate_suggested_filename(&topic_slug);

    let anchor_path = logs_dir
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("active_shake_anchor.json");
    let history_events = load_or_discover_history(logs_dir, &anchor_path);
    let timeline_section = format_history_timeline(&history_events);

    let mode_note = if is_milestone_horizon_active {
        "> - **Compaction Mode**: ⚡ Marathon Reset (/full-shake) (Turn 1 Genesis preserved; intermediate turns collapsed into Milestone Horizon; thoughts windowed; last 25 turns active).\n".to_string()
    } else if let Some(w) = options.thought_window_turns {
        if effective_assistant_turns > w {
            format!("> - **Compaction Mode**: ⚡ Full Deep Compaction (Scratchpad thoughts retained for last {} turns; older thoughts dropped).\n", w)
        } else {
            "> - **Compaction Mode**: 🟢 Standard Zero-Loss Compaction (All thoughts retained; session under 20 turns).\n".to_string()
        }
    } else {
        "> - **Compaction Mode**: 🟢 Standard Zero-Loss Compaction (100% thoughts retained).\n"
            .to_string()
    };

    let header = format!(
        "# Shaken & Pruned History: {}\n\n\
        > [!IMPORTANT]\n\
        > **Context Note for Assistant**:\n\
        > This document is a complete, verbatim transcript of earlier turns with token bloat removed via `/shake`.\n\
        > - **User prompts, Assistant explanations, and Decisions are 100% complete and verbatim.**\n\
        {}\
        > - Actions marked `[PRUNED ...]` were successfully executed. Stored stdout is archived in the master permanent log with exact line pointers (`line=N`).\n\
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

    let raw_document = format!("{}{}", header, output_blocks.join("\n\n"));
    let full_document = if options.redact_secrets {
        redact_secrets(&raw_document)
    } else {
        raw_document
    };
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

    let duration_ms = pipeline_start
        .elapsed()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64;
    let trigger_detail = if options.marathon_horizon {
        "full-marathon".to_string()
    } else if options.thought_window_turns.is_some() {
        "full-thought-window".to_string()
    } else {
        "manual".to_string()
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
        duration_ms,
        trigger_detail,
    };

    Ok((
        compacted_output,
        full_document,
        stats,
        master_archive_abs_str,
    ))
}
