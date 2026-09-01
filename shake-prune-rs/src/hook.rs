use serde::Deserialize;
use serde_json::json;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

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
}

pub fn handle_hook() {
    // Fail-open guarantee: any failure must cleanly output empty JSON with exit 0
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

    let mut candidate_paths: Vec<PathBuf> = Vec::new();

    if let Some(art_dir) = &payload.artifact_directory_path {
        candidate_paths.push(Path::new(art_dir).join("active_shake_anchor.json"));
    }

    if let Some(t_path) = &payload.transcript_path {
        if let Some(parent2) = Path::new(t_path).parent().and_then(|p| p.parent()) {
            candidate_paths.push(parent2.join("active_shake_anchor.json"));
        }
    }

    if let Some(conv_id) = &payload.conversation_id {
        if let Ok(home) = std::env::var("HOME") {
            let base_dirs = [
                ".gemini/antigravity-ide/brain",
                ".gemini/antigravity/brain",
                ".gemini/antigravity-cli/brain",
            ];
            for base in &base_dirs {
                candidate_paths.push(Path::new(&home).join(base).join(conv_id).join("active_shake_anchor.json"));
            }
        }
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
