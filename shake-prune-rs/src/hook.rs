use crate::metadata::{write_active_anchor, write_artifact_metadata, AnchorFilePayload};
use crate::pruner::{run_compaction_pipeline, CompactionOptions};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::panic;
use std::path::{Path, PathBuf};

// 200k tokens * ~3.3 bytes/token = 660,000 bytes
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
            if matches!(t, "RUN_COMMAND" | "VIEW_FILE" | "SEARCH_WEB" | "GREP_SEARCH" | "CODE_ACTION") {
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
    let _ = io::stdin().read_to_string(&mut stdin_buffer);

    if stdin_buffer.trim().is_empty() {
        println!("{{}}");
        return Ok(());
    }

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
                if let Some(conv_dir) = p.parent().and_then(|p| p.parent()).and_then(|p| p.parent()) {
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
                    if data.active.unwrap_or(false) {
                        found_anchor = Some(data);
                        break;
                    }
                }
            }
        }
    }

    // ⚡ PROACTIVE AUTO-SHAKE WITH GROWTH DELTA & COOLDOWN GUARDS
    if let (Some(t_path), Some(art_dir)) = (&resolved_transcript, &resolved_art_dir) {
        if let Ok(meta) = fs::metadata(t_path) {
            let unpruned_tools = count_unpruned_tools(t_path);
            let size_threshold_hit = meta.len() >= AUTO_SHAKE_TOKEN_THRESHOLD_BYTES;
            let tools_threshold_hit = unpruned_tools >= AUTO_SHAKE_TOOL_RUN_THRESHOLD;

            if size_threshold_hit || tools_threshold_hit {
                let now_ts = Utc::now().timestamp();
                let should_auto_shake = match &found_anchor {
                    Some(anchor) => {
                        let last_bytes = anchor.last_compacted_bytes.unwrap_or(0);
                        let last_attempt = anchor.last_attempt_timestamp.unwrap_or(0);
                        let cooldown_ok = (now_ts - last_attempt).abs() >= AUTO_SHAKE_COOLDOWN_SECONDS;
                        let growth_ok = meta.len() > last_bytes + AUTO_SHAKE_GROWTH_DELTA_BYTES;
                        cooldown_ok && growth_ok
                    }
                    None => true,
                };

                if should_auto_shake {
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

                    if let Ok((_compacted_jsonl, pruned_md, stats, backup_file_str)) =
                        run_compaction_pipeline(t_path, &options)
                    {
                        let output_path = art_dir.join(&stats.suggested_filename);
                        if let Ok(mut f) = File::create(&output_path) {
                            let _ = f.write_all(pruned_md.as_bytes());
                        }

                        let summary_text = format!(
                            "Auto-compacted verbatim history at 200k token threshold for topic '{}'. Saved {:.1}% context tokens.",
                            stats.topic_slug.replace('_', " "),
                            stats.reduction_pct
                        );
                        let _ = write_artifact_metadata(&output_path, &summary_text);
                        let _ = write_active_anchor(&output_path, &stats, "Auto (200k Threshold)", &backup_file_str);

                        let ephemeral_msg = format!(
                            "[Context auto-compacted via /shake (exceeded 200k token threshold). Active state anchored in @{} (Step {}+). Treat prior raw tool stdout as archived.]",
                            output_path.display(),
                            stats.user_turns + stats.assistant_turns + stats.pruned_tools
                        );

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
                }
            }
        }
    }

    // Normal anchor message injection if under threshold or already compacted
    match found_anchor {
        Some(anchor) => {
            let shaken_file = anchor.shaken_file.unwrap_or_default();
            let anchored_step = match anchor.anchored_at_step {
                Some(serde_json::Value::Number(n)) => n.to_string(),
                Some(serde_json::Value::String(s)) => s,
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
        None => {
            println!("{{}}");
        }
    }

    Ok(())
}
