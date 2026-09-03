fn log_with_level(level: &str, msg: &str) {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    if home.is_empty() {
        return;
    }
    let logs_dir = Path::new(&home).join(".gemini/logs");
    let _ = fs::create_dir_all(&logs_dir);
    let log_file = logs_dir.join("shake_hook.log");

    if let Ok(meta) = fs::metadata(&log_file) {
        if meta.len() > 1_000_000 {
            // Rotate: .1 -> .2, current -> .1 (keeps two generations, P3-3).
            let rotated1 = logs_dir.join("shake_hook.log.1");
            let rotated2 = logs_dir.join("shake_hook.log.2");
            let _ = fs::remove_file(&rotated2);
            let _ = fs::rename(&rotated1, &rotated2);
            let _ = fs::rename(&log_file, &rotated1);
        }
    }

    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
    {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(f, "[{}] [{}] {}", ts, level, msg);
    }
}

fn log_diagnostic(msg: &str) {
    log_with_level("INFO", msg);
}

use crate::format_bytes;
use crate::metadata::{
    is_circuit_open, record_compaction_failure, write_active_anchor, write_artifact_metadata,
    AnchorFilePayload,
};
use crate::pruner::{run_compaction_pipeline, CompactionOptions};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::panic;
use std::path::{Path, PathBuf};

// 80k tokens * ~3.3 bytes/token = 264,000 bytes
// Proactive 80k tokens threshold (~264,000 bytes)
const AUTO_SHAKE_TOKEN_THRESHOLD_BYTES: u64 = 264_000;
// Tool execution burst threshold (triggers after 20 unpruned tool executions)
const AUTO_SHAKE_TOOL_RUN_THRESHOLD: usize = 20;
// Minimum new unpruned growth (25 KB) required before triggering another auto-compaction
const AUTO_SHAKE_GROWTH_DELTA_BYTES: u64 = 25_000;
// Minimum seconds between auto-compaction attempts (3 minutes) to prevent thrashing
const AUTO_SHAKE_COOLDOWN_SECONDS: i64 = 180;

#[derive(Deserialize, Debug, Default)]
struct HookPayload {
    #[serde(rename = "conversationId")]
    conversation_id: Option<String>,
    #[serde(rename = "transcriptPath")]
    transcript_path: Option<String>,
    #[serde(rename = "artifactDirectoryPath")]
    artifact_directory_path: Option<String>,
    #[serde(rename = "invocationNum")]
    invocation_num: Option<u64>,
    #[serde(rename = "terminationReason")]
    termination_reason: Option<String>,
    #[serde(rename = "fullyIdle")]
    fully_idle: Option<bool>,
    #[serde(rename = "executionNum")]
    execution_num: Option<u64>,
}

/// Strictly validates that a directory path is within the user's system-managed ~/.gemini directory
/// to completely prevent context poisoning from arbitrary workspace git repositories.
/// Counts the number of raw, unpruned tool execution outputs in the transcript.
fn count_unpruned_tools(t_path: &Path) -> usize {
    let file = match File::open(t_path) {
        Ok(f) => f,
        Err(_) => return 0,
    };
    let reader = io::BufReader::new(file);
    use io::BufRead;
    let mut count = 0;
    for l in reader.lines().map_while(Result::ok) {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&l) {
            let t = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if matches!(
                t,
                "RUN_COMMAND" | "VIEW_FILE" | "SEARCH_WEB" | "GREP_SEARCH" | "CODE_ACTION"
            ) {
                let content = val.get("content").and_then(|v| v.as_str()).unwrap_or("");
                if !content.starts_with("[PRUNED tool=") {
                    count += 1;
                }
            }
        }
    }
    count
}

fn is_trusted_storage_path(p: &Path) -> bool {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    if home.is_empty() {
        return false;
    }
    let trusted_gemini = Path::new(&home).join(".gemini");
    let trusted_canonical = match trusted_gemini.canonicalize() {
        Ok(c) => c,
        Err(_) => return false, // Fail closed if ~/.gemini does not exist or cannot be resolved
    };

    let p_abs = if p.is_absolute() {
        p.to_path_buf()
    } else if let Ok(curr) = std::env::current_dir() {
        curr.join(p)
    } else {
        return false; // Fail closed if relative path cannot be resolved to current dir
    };

    // Mandatory canonicalization: fail closed to prevent symlink traversal or evasion
    match p_abs.canonicalize() {
        Ok(canonical) => canonical.starts_with(&trusted_canonical),
        Err(_) => false,
    }
}

pub fn handle_hook() {
    // True Panic-Safe Fail-Open: catch_unwind active with unwind panic strategy
    let result = panic::catch_unwind(|| {
        if run_hook_safely().is_err() {
            println!("{{}}");
        }
    });

    if result.is_err() {
        println!("{{}}");
    }
}

fn run_hook_safely() -> Result<(), Box<dyn std::error::Error>> {
    let mut stdin_buffer = String::new();
    let _ = io::stdin().take(65_536).read_to_string(&mut stdin_buffer);

    if stdin_buffer.trim().is_empty() {
        println!("{{}}");
        return Ok(());
    }
    log_diagnostic("Hook triggered with PreInvocation payload");

    let payload: HookPayload = serde_json::from_str(&stdin_buffer).unwrap_or_default();

    let mut resolved_transcript: Option<PathBuf> = None;
    let mut resolved_art_dir: Option<PathBuf> = None;

    if let Some(art_dir) = &payload.artifact_directory_path {
        let p = PathBuf::from(art_dir);
        if is_trusted_storage_path(&p) {
            resolved_art_dir = Some(p.clone());
            let possible_t = p.join(".system_generated/logs/transcript.jsonl");
            if possible_t.exists() {
                resolved_transcript = Some(possible_t);
            }
        }
    }

    if resolved_transcript.is_none() {
        if let Some(t_path) = &payload.transcript_path {
            let p = PathBuf::from(t_path);
            if p.exists() && is_trusted_storage_path(&p) {
                resolved_transcript = Some(p.clone());
                if let Some(conv_dir) = p.parent().and_then(|p| p.parent()).and_then(|p| p.parent())
                {
                    if resolved_art_dir.is_none() && is_trusted_storage_path(conv_dir) {
                        resolved_art_dir = Some(conv_dir.to_path_buf());
                    }
                }
            }
        }
    }

    if resolved_transcript.is_none() {
        if let Some(conv_id) = &payload.conversation_id {
            let mut home_dirs = Vec::new();
            if let Ok(home) = std::env::var("HOME") {
                home_dirs.push(PathBuf::from(home));
            }
            if let Ok(uprof) = std::env::var("USERPROFILE") {
                home_dirs.push(PathBuf::from(uprof));
            }

            let base_dirs = [
                ".gemini/antigravity-ide/brain",
                ".gemini/antigravity/brain",
                ".gemini/antigravity-cli/brain",
            ];
            for h in &home_dirs {
                for base in &base_dirs {
                    let conv_dir = h.join(base).join(conv_id);
                    let t_path = conv_dir.join(".system_generated/logs/transcript.jsonl");
                    if t_path.exists() && is_trusted_storage_path(&t_path) {
                        resolved_transcript = Some(t_path);
                        resolved_art_dir = Some(conv_dir);
                        break;
                    }
                }
                if resolved_transcript.is_some() {
                    break;
                }
            }
        }
    }

    // Candidate anchor paths (strictly restricted to trusted system directories)
    let mut candidate_paths: Vec<PathBuf> = Vec::new();
    if let Some(art_dir) = &resolved_art_dir {
        if is_trusted_storage_path(art_dir) {
            candidate_paths.push(art_dir.join("active_shake_anchor.json"));
        }
    }

    let mut found_anchor: Option<AnchorFilePayload> = None;
    for path in candidate_paths {
        if path.exists() {
            if let Ok(file) = File::open(&path) {
                if let Ok(data) = serde_json::from_reader::<_, AnchorFilePayload>(file) {
                    // Load anchor state regardless of active status so circuit breaker
                    // and cooldown guards function on fresh sessions with failures (§4.1).
                    found_anchor = Some(data);
                    break;
                }
            }
        }
    }

    let is_stop_event = payload.termination_reason.is_some()
        || payload.fully_idle.is_some()
        || payload.execution_num.is_some();
    let is_turn_start = payload.invocation_num.map(|n| n <= 1).unwrap_or(true);

    // 🛑 MID-TURN TOOL SEQUENCE GUARD:
    // If running under PreInvocation and invocationNum > 1, the agent is actively executing
    // in a multi-step tool sequence. Bypassing compaction and notice injection completely
    // ensures the active tool chain is never disturbed mid-flight and context is not spammed.
    if !is_stop_event && !is_turn_start {
        log_diagnostic(
            "Auto-shake bypassed: mid-turn tool sequence active (waiting for turn completion / next user turn)",
        );
        println!("{{}}");
        return Ok(());
    }

    // ⚡ PROACTIVE AUTO-SHAKE WITH GROWTH DELTA & COOLDOWN GUARDS
    if let (Some(t_path), Some(art_dir)) = (&resolved_transcript, &resolved_art_dir) {
        if let Ok(meta) = fs::metadata(t_path) {
            let file_size = meta.len();
            let now_ts = Utc::now().timestamp();

            // Circuit breaker (P1-5): skip auto-shake while disabled.
            if let Some(anchor) = &found_anchor {
                if is_circuit_open(anchor, now_ts) {
                    log_with_level(
                        "WARN",
                        &format!(
                            "Auto-shake bypassed: circuit breaker open after {} consecutive failures",
                            anchor.consecutive_failures.unwrap_or(0)
                        ),
                    );
                    // Fall through to normal anchor injection below.
                    return emit_anchor_or_empty(&found_anchor);
                }
            }

            // 1. Evaluate Cooldown & Growth Delta Guards first (O(1))
            let (cooldown_ok, growth_ok) = match &found_anchor {
                Some(anchor) => {
                    let last_bytes = anchor.last_compacted_bytes.unwrap_or(0);
                    let last_attempt = anchor.last_attempt_timestamp.unwrap_or(0);
                    let cd_ok = (now_ts - last_attempt).abs() >= AUTO_SHAKE_COOLDOWN_SECONDS;
                    let gr_ok = file_size > last_bytes + AUTO_SHAKE_GROWTH_DELTA_BYTES;
                    (cd_ok, gr_ok)
                }
                None => (true, true),
            };

            let guards_pass = cooldown_ok && growth_ok;
            let size_threshold_hit = file_size >= AUTO_SHAKE_TOKEN_THRESHOLD_BYTES;

            // 2. Short-circuit: only count tools if guards pass and size threshold has not already triggered
            // P2-3: track WHICH threshold fired so messages/metrics are accurate.
            let trigger_detail: Option<&str> = if !guards_pass {
                if !cooldown_ok {
                    log_diagnostic("Auto-shake bypassed: cooldown active");
                } else if !growth_ok {
                    log_diagnostic("Auto-shake bypassed: growth delta below threshold");
                }
                None
            } else if size_threshold_hit {
                log_diagnostic("Auto-shake triggered: size threshold (>= 264 KB) hit");
                Some("size")
            } else {
                let unpruned_tools = count_unpruned_tools(t_path);
                if unpruned_tools >= AUTO_SHAKE_TOOL_RUN_THRESHOLD {
                    log_diagnostic(
                        "Auto-shake triggered: unpruned tools burst threshold (>= 20 tools) hit",
                    );
                    Some("tools")
                } else {
                    None
                }
            };

            if let Some(trigger) = trigger_detail {
                let options = CompactionOptions {
                    recent_user_turns: 10,
                    recent_tools_cap: 20,
                    recent_errors_cap: 30,
                    recent_window_steps: 6,
                    thought_window_turns: None,
                    marathon_horizon: false,
                    in_place: true,
                    dry_run: false,
                };

                let auto_start = std::time::Instant::now();
                match run_compaction_pipeline(t_path, &options) {
                    Ok((_compacted_jsonl, pruned_md, mut stats, master_archive_str)) => {
                        // Tag hook trigger detail for anchor metrics (P1-6).
                        stats.trigger_detail = format!("auto-{}", trigger);
                        let elapsed_ms = auto_start.elapsed().as_millis() as u64;
                        log_with_level(
                            "INFO",
                            &format!(
                                "Auto-shake complete trigger={} before={} after={} saved={:.1}% duration_ms={} archive={}",
                                trigger,
                                stats.this_run_before_bytes,
                                stats.this_run_after_bytes,
                                stats.this_run_savings_pct,
                                elapsed_ms,
                                master_archive_str,
                            ),
                        );
                        let output_path = art_dir.join(&stats.suggested_filename);
                        if let Ok(mut f) = File::create(&output_path) {
                            let _ = f.write_all(pruned_md.as_bytes());
                        }

                        let trigger_label = if trigger == "size" {
                            "Auto (80k Threshold)"
                        } else {
                            "Auto (Tool Burst)"
                        };
                        let summary_text = if trigger == "size" {
                            format!(
                                "Auto-compacted transcript at 80k token threshold for topic '{}'. Saved {:.1}% prompt payload ({} -> {}).",
                                stats.topic_slug.replace('_', " "),
                                stats.this_run_savings_pct,
                                format_bytes(stats.this_run_before_bytes),
                                format_bytes(stats.this_run_after_bytes)
                            )
                        } else {
                            format!(
                                "Auto-compacted transcript on unpruned tool burst (>= 20 tools) for topic '{}'. Saved {:.1}% prompt payload ({} -> {}).",
                                stats.topic_slug.replace('_', " "),
                                stats.this_run_savings_pct,
                                format_bytes(stats.this_run_before_bytes),
                                format_bytes(stats.this_run_after_bytes)
                            )
                        };
                        let _ = write_artifact_metadata(&output_path, &summary_text);
                        let _ = write_active_anchor(
                            &output_path,
                            &stats,
                            trigger_label,
                            &master_archive_str,
                        );

                        let ephemeral_msg = if trigger == "size" {
                            format!(
                                "[Context auto-compacted via /shake (exceeded 80k token threshold). Active state anchored in @{} (Step {}+). Treat prior raw tool stdout as archived.]",
                                output_path.display(),
                                stats.user_turns + stats.assistant_turns + stats.pruned_tools
                            )
                        } else {
                            format!(
                                "[Context auto-compacted via /shake (unpruned tool burst >= 20). Active state anchored in @{} (Step {}+). Treat prior raw tool stdout as archived.]",
                                output_path.display(),
                                stats.user_turns + stats.assistant_turns + stats.pruned_tools
                            )
                        };

                        if is_stop_event {
                            println!("{{}}");
                            return Ok(());
                        }

                        let response = json!({
                            "injectSteps": [
                                {
                                    "ephemeralMessage": ephemeral_msg
                                }
                            ]
                        });

                        println!("{}", serde_json::to_string(&response)?);
                        return Ok(());
                    }
                    Err(e) => {
                        let elapsed_ms = auto_start.elapsed().as_millis() as u64;
                        log_with_level(
                            "ERROR",
                            &format!(
                                "Auto-shake failed trigger={} duration_ms={} error={}",
                                trigger, elapsed_ms, e
                            ),
                        );
                        // Record failure for circuit breaker (P1-5 / P1-6).
                        let anchor_path = art_dir.join("active_shake_anchor.json");
                        record_compaction_failure(&anchor_path, &e.to_string());
                    }
                }
            }
        }
    }

    // On Stop event, the agent is now idle; conclude silently without prompt injection
    if is_stop_event {
        println!("{{}}");
        return Ok(());
    }

    // On PreInvocation turn start (invocationNum == 1), inject the anchor notice once
    emit_anchor_or_empty(&found_anchor)
}

/// Shared tail: emit stored anchor notice or fail-open `{}`.
fn emit_anchor_or_empty(
    found_anchor: &Option<AnchorFilePayload>,
) -> Result<(), Box<dyn std::error::Error>> {
    match found_anchor {
        Some(anchor) if anchor.active.unwrap_or(false) => {
            let shaken_file = anchor.shaken_file.clone().unwrap_or_default();
            let anchored_step = match &anchor.anchored_at_step {
                Some(serde_json::Value::Number(n)) => n.to_string(),
                Some(serde_json::Value::String(s)) => s.clone(),
                _ => "recent".to_string(),
            };

            if shaken_file.is_empty() {
                println!("{{}}");
                return Ok(());
            }

            let ephemeral_msg = format!(
                "[Context compacted via /shake. Active state anchored in @{} (Step {}+). Treat prior raw tool stdout as archived.]",
                shaken_file, anchored_step
            );

            let response = json!({
                "injectSteps": [
                    {
                        "ephemeralMessage": ephemeral_msg
                    }
                ]
            });

            println!("{}", serde_json::to_string(&response)?);
        }
        _ => {
            println!("{{}}");
        }
    }

    Ok(())
}
