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

    if let Ok(mut f) = crate::atomic::open_user_only_append(&log_file) {
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
/// Counts the number of raw, unpruned tool execution outputs in the transcript,
/// stopping as soon as the provided threshold is reached (P1-4).
fn count_unpruned_tools(t_path: &Path, threshold: usize) -> usize {
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
            if crate::receipts::is_tool_step_type(t) {
                let content = val.get("content").and_then(|v| v.as_str()).unwrap_or("");
                if !crate::receipts::is_pruned_receipt(content) {
                    count += 1;
                    if count >= threshold {
                        break;
                    }
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

    let mut trusted_roots: Vec<PathBuf> = Vec::new();
    let trusted_gemini = Path::new(&home).join(".gemini");
    if let Ok(c) = trusted_gemini.canonicalize() {
        trusted_roots.push(c);
    }

    if let Ok(extra) = std::env::var("SHAKE_TRUSTED_STORAGE_ROOTS") {
        for root in extra.split([':', ';', ',']) {
            let r = root.trim();
            if !r.is_empty() {
                if let Ok(c) = Path::new(r).canonicalize() {
                    trusted_roots.push(c);
                }
            }
        }
    }

    if let Ok(app_data) = std::env::var("ANTIGRAVITY_APP_DATA_DIR") {
        let a = app_data.trim();
        if !a.is_empty() {
            if let Ok(c) = Path::new(a).canonicalize() {
                trusted_roots.push(c);
            }
        }
    }

    if trusted_roots.is_empty() {
        return false;
    }

    let p_abs = if p.is_absolute() {
        p.to_path_buf()
    } else if let Ok(curr) = std::env::current_dir() {
        curr.join(p)
    } else {
        return false; // Fail closed if relative path cannot be resolved to current dir
    };

    // Mandatory canonicalization: fail closed to prevent symlink traversal or evasion
    match p_abs.canonicalize() {
        Ok(canonical) => trusted_roots.iter().any(|root| canonical.starts_with(root)),
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
    let hook_start = std::time::Instant::now();
    // 2.5s watchdog budget (P1-2). Test override via env (0 forces expiry).
    let deadline_ms: u64 = std::env::var("SHAKE_HOOK_DEADLINE_MS")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(2500);
    let hook_deadline = std::time::Duration::from_millis(deadline_ms);

    let mut stdin_buffer = String::new();
    let _ = io::stdin().take(65_536).read_to_string(&mut stdin_buffer);

    if stdin_buffer.trim().is_empty() {
        println!("{{}}");
        return Ok(());
    }
    log_diagnostic("Hook triggered with PreInvocation payload");

    let payload: HookPayload = serde_json::from_str(&stdin_buffer).unwrap_or_default();
    let config = crate::config::ShakeConfig::load();

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

    // P0-2: Auto-recover if a previous compaction crashed or was interrupted
    if let Some(t_path) = &resolved_transcript {
        match crate::atomic::recover_if_interrupted(t_path) {
            Ok(Some(recovery_msg)) => {
                log_with_level("INFO", &recovery_msg);
            }
            Err(e) => {
                log_with_level(
                    "ERROR",
                    &format!("Crash recovery failed for '{}': {}", t_path.display(), e),
                );
                println!("{{}}");
                return Ok(());
            }
            _ => {}
        }
    }

    // Candidate anchor paths (strictly restricted to trusted system directories)
    let mut candidate_paths: Vec<PathBuf> = Vec::new();
    if let Some(art_dir) = &resolved_art_dir {
        if is_trusted_storage_path(art_dir) {
            candidate_paths.push(art_dir.join("active_shake_anchor.json"));
        }
    }

    let mut found_anchor: Option<(PathBuf, AnchorFilePayload)> = None;
    for path in candidate_paths {
        if path.exists() {
            if let Ok(file) = File::open(&path) {
                if let Ok(data) = serde_json::from_reader::<_, AnchorFilePayload>(file) {
                    found_anchor = Some((path, data));
                    break;
                }
            }
        }
    }

    // P0-4: Check if auto-shake is disabled via config or environment
    if !config.auto.enabled {
        log_diagnostic("Auto-shake disabled via config/environment (auto.enabled = false)");
        return emit_anchor_or_empty(&found_anchor);
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
            const MAX_HOOK_TRANSCRIPT_BYTES: u64 = 50 * 1024 * 1024; // 50MB safety limit (S7)
            if file_size > MAX_HOOK_TRANSCRIPT_BYTES {
                log_with_level(
                    "WARN",
                    &format!(
                        "Auto-shake bypassed: transcript file size ({} bytes) exceeds 50MB safety limit",
                        file_size
                    ),
                );
                return emit_anchor_or_empty(&found_anchor);
            }
            let now_ts = Utc::now().timestamp();

            // Circuit breaker (P1-5): skip auto-shake while disabled.
            if let Some((_, anchor)) = &found_anchor {
                if is_circuit_open(anchor, now_ts) {
                    log_with_level(
                        "WARN",
                        &format!(
                            "Auto-shake bypassed: circuit breaker open after {} consecutive failures",
                            anchor.consecutive_failures.unwrap_or(0)
                        ),
                    );
                    return emit_anchor_or_empty(&found_anchor);
                }
            }

            // 1. Evaluate Cooldown & Growth Delta Guards first (O(1))
            let (cooldown_ok, growth_ok) = match &found_anchor {
                Some((_, anchor)) => {
                    let last_bytes = anchor.last_compacted_bytes.unwrap_or(0);
                    let last_attempt = anchor.last_attempt_timestamp.unwrap_or(0);
                    let cd_ok = (now_ts - last_attempt).abs() >= config.auto.cooldown_seconds;
                    let gr_ok = file_size > last_bytes + config.auto.growth_delta_bytes;
                    (cd_ok, gr_ok)
                }
                None => (true, true),
            };

            let guards_pass = cooldown_ok && growth_ok;
            let size_threshold_hit = file_size >= config.auto.size_threshold_bytes;

            // 2. Short-circuit: only count tools if guards pass and size threshold has not already triggered
            let trigger_detail: Option<&str> = if !guards_pass {
                if !cooldown_ok {
                    log_diagnostic("Auto-shake bypassed: cooldown active");
                } else if !growth_ok {
                    log_diagnostic("Auto-shake bypassed: growth delta below threshold");
                }
                None
            } else if size_threshold_hit {
                log_diagnostic("Auto-shake triggered: size threshold hit");
                Some("size")
            } else {
                if hook_start.elapsed() > hook_deadline {
                    log_with_level(
                        "WARN",
                        "Hook watchdog budget exceeded before counting tools",
                    );
                    return emit_anchor_or_empty(&found_anchor);
                }
                let unpruned_tools = count_unpruned_tools(t_path, config.auto.tool_burst_threshold);
                if unpruned_tools >= config.auto.tool_burst_threshold {
                    log_diagnostic("Auto-shake triggered: unpruned tools burst threshold hit");
                    Some("tools")
                } else {
                    None
                }
            };

            if let Some(trigger) = trigger_detail {
                if hook_start.elapsed() > hook_deadline {
                    log_with_level(
                        "WARN",
                        "Hook watchdog budget exceeded before running compaction",
                    );
                    return emit_anchor_or_empty(&found_anchor);
                }

                let mut options = CompactionOptions::from_config(&config, true, false, true);
                options.deadline = Some(hook_start + hook_deadline);

                let auto_start = std::time::Instant::now();
                match run_compaction_pipeline(t_path, &options) {
                    Ok((_compacted_jsonl, pruned_md, mut stats, master_archive_str)) => {
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
                        if let Ok(mut f) = crate::atomic::create_user_only_file(&output_path) {
                            let _ = f.write_all(pruned_md.as_bytes());
                        }
                        let _ = crate::metadata::prune_old_artifacts(
                            art_dir,
                            config.retention.artifact_retention_count,
                        );

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

                        let analysis_res = crate::analysis::analyze_transcript(
                            t_path,
                            &crate::analysis::AnalysisOptions::default(),
                        );
                        let continuity_card = if let Ok(ref analysis) = analysis_res {
                            Some(crate::continuity::ContinuityCard::build(
                                analysis,
                                &master_archive_str,
                                t_path,
                                options.redact_secrets,
                            ))
                        } else {
                            None
                        };

                        let _ = write_active_anchor(
                            &output_path,
                            &stats,
                            trigger_label,
                            &master_archive_str,
                            continuity_card.as_ref(),
                        );

                        let step_label = if stats.max_step_index > 0 {
                            stats.max_step_index
                        } else {
                            (stats.user_turns + stats.assistant_turns + stats.pruned_tools) as u64
                        };

                        let trigger_phrase = if trigger == "size" {
                            "exceeded 80k token threshold"
                        } else {
                            "unpruned tool burst >= 20"
                        };

                        let ephemeral_msg = if let Some(ref card) = continuity_card {
                            card.to_ephemeral_notice(
                                trigger_phrase,
                                &output_path.to_string_lossy(),
                                step_label,
                            )
                        } else {
                            format!(
                                "[Context auto-compacted via /shake ({}). Active state anchored in @{} (Step {}+). Treat prior raw tool stdout as archived.]",
                                trigger_phrase,
                                output_path.display(),
                                step_label
                            )
                        };

                        let anchor_path = art_dir.join("active_shake_anchor.json");

                        if is_stop_event {
                            // On Stop event, keep anchor active and injected:false so the next
                            // user message (PreInvocation) will emit the continuity anchor notice (A1).
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

                        // Mark anchor as consumed after successful emission, preserving 0600 permissions (B3, S2, D7)
                        let _ = crate::metadata::consume_anchor(&anchor_path);
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
    found_anchor: &Option<(PathBuf, AnchorFilePayload)>,
) -> Result<(), Box<dyn std::error::Error>> {
    match found_anchor {
        Some((anchor_path, anchor))
            if anchor.active.unwrap_or(false) && !anchor.injected.unwrap_or(false) =>
        {
            let now_ts = Utc::now().timestamp();
            if is_circuit_open(anchor, now_ts) {
                log_diagnostic("Suppressing anchor injection: circuit breaker is open");
                println!("{{}}");
                return Ok(());
            }

            let shaken_file = anchor.shaken_file.clone().unwrap_or_default();
            let shaken_path = Path::new(&shaken_file);
            if shaken_file.is_empty()
                || !shaken_file.ends_with(".md")
                || shaken_file
                    .chars()
                    .any(|c| c.is_control() || c == '\n' || c == '\r')
                || !shaken_path.exists()
                || !is_trusted_storage_path(shaken_path)
            {
                log_diagnostic(
                    "Anchor shaken_file failed validation, does not exist, or untrusted",
                );
                println!("{{}}");
                return Ok(());
            }

            let anchored_step = match &anchor.anchored_at_step {
                Some(serde_json::Value::Number(n)) => n.to_string(),
                Some(serde_json::Value::String(s)) => {
                    let sanitized: String = s
                        .chars()
                        .filter(|c| !c.is_control() && *c != '\n' && *c != '\r')
                        .collect();
                    sanitized
                }
                _ => "recent".to_string(),
            };

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

            // Mark anchor as consumed after successful emission, preserving 0600 permissions (B3, S2, D7, B12c)
            let _ = crate::metadata::consume_anchor(anchor_path);
        }
        _ => {
            println!("{{}}");
        }
    }

    Ok(())
}
