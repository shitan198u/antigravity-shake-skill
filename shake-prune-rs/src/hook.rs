use crate::metadata::{write_active_anchor, write_artifact_metadata};
use crate::pruner::{compact_transcript_inplace, prune_transcript};
use serde::Deserialize;
use serde_json::json;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::PathBuf;

// 200k tokens * ~3.3 bytes/token = 660,000 bytes
const AUTO_SHAKE_TOKEN_THRESHOLD_BYTES: u64 = 660_000;
// Minimum new unpruned growth (50 KB) required before triggering another auto-compaction
const AUTO_SHAKE_GROWTH_DELTA_BYTES: u64 = 50_000;

#[derive(Deserialize, Debug, Default)]
struct HookPayload {
    #[serde(rename = "conversationId")]
    conversation_id: Option<String>,
    #[serde(rename = "transcriptPath")]
    transcript_path: Option<String>,
    #[serde(rename = "artifactDirectoryPath")]
    artifact_directory_path: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
struct AnchorData {
    active: Option<bool>,
    shaken_file: Option<String>,
    anchored_at_step: Option<serde_json::Value>,
    last_compacted_bytes: Option<u64>,
}

pub fn handle_hook() {
    // Fail-open guarantee: any failure cleanly outputs empty JSON with exit 0
    if let Err(_) = run_hook_safely() {
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
    
    // Resolve conversation directory, transcript path, and artifact directory
    let mut resolved_transcript: Option<PathBuf> = None;
    let mut resolved_art_dir: Option<PathBuf> = None;

    if let Some(art_dir) = &payload.artifact_directory_path {
        let p = PathBuf::from(art_dir);
        resolved_art_dir = Some(p.clone());
        let possible_t = p.join(".system_generated/logs/transcript.jsonl");
        if possible_t.exists() {
            resolved_transcript = Some(possible_t);
        }
    }

    if resolved_transcript.is_none() {
        if let Some(t_path) = &payload.transcript_path {
            let p = PathBuf::from(t_path);
            if p.exists() {
                resolved_transcript = Some(p.clone());
                if let Some(conv_dir) = p.parent().and_then(|p| p.parent()).and_then(|p| p.parent()) {
                    if resolved_art_dir.is_none() {
                        resolved_art_dir = Some(conv_dir.to_path_buf());
                    }
                }
            }
        }
    }

    // Fallback discovery across known global brain directories if conversation_id is provided
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
                    if t_path.exists() {
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

    // Check existing anchor state
    let mut candidate_paths: Vec<PathBuf> = Vec::new();
    if let Some(art_dir) = &resolved_art_dir {
        candidate_paths.push(art_dir.join("active_shake_anchor.json"));
    }

    let mut found_anchor: Option<AnchorData> = None;
    for path in candidate_paths {
        if path.exists() {
            if let Ok(file) = File::open(&path) {
                if let Ok(data) = serde_json::from_reader::<_, AnchorData>(file) {
                    if data.active.unwrap_or(false) {
                        found_anchor = Some(data);
                        break;
                    }
                }
            }
        }
    }

    // ⚡ PROACTIVE AUTO-SHAKE WITH GROWTH DELTA GUARD:
    // Only triggers if:
    // 1. Transcript exceeds 200k tokens (660 KB)
    // 2. AND (has never been shaken OR has grown by at least 50 KB of new tool logs since last shake)
    if let (Some(t_path), Some(art_dir)) = (&resolved_transcript, &resolved_art_dir) {
        if let Ok(meta) = fs::metadata(t_path) {
            if meta.len() >= AUTO_SHAKE_TOKEN_THRESHOLD_BYTES {
                let should_auto_shake = match &found_anchor {
                    Some(anchor) => {
                        let last_bytes = anchor.last_compacted_bytes.unwrap_or(0);
                        meta.len() > last_bytes + AUTO_SHAKE_GROWTH_DELTA_BYTES
                    }
                    None => true,
                };

                if should_auto_shake {
                    if let Ok((pruned_md, stats)) = prune_transcript(t_path, 6) {
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
                        let _ = write_active_anchor(&output_path, &stats);
                        let _ = compact_transcript_inplace(t_path, 6);

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

    // Normal anchor message injection if already compacted or under threshold
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
