use crate::models::{CompactionEvent, PruningStats};
use chrono::{Local, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use tempfile::Builder;

#[derive(Deserialize, Serialize, Debug, Default, Clone)]
pub struct AnchorFilePayload {
    pub active: Option<bool>,
    pub shaken_file: Option<String>,
    pub anchored_at_step: Option<serde_json::Value>,
    pub last_compacted_bytes: Option<u64>,
    pub last_attempt_timestamp: Option<i64>,
    pub topic: Option<String>,
    pub token_savings_pct: Option<f64>,
    pub raw_tokens: Option<usize>,
    pub pruned_tokens: Option<usize>,
    pub timestamp: Option<String>,
    #[serde(default)]
    pub compaction_history: Vec<CompactionEvent>,
}

pub fn load_or_discover_history(logs_dir: &Path, current_anchor: &Path) -> Vec<CompactionEvent> {
    let mut history: Vec<CompactionEvent> = Vec::new();

    // 1. Try reading existing history from anchor
    if current_anchor.exists() {
        if let Ok(file) = File::open(current_anchor) {
            if let Ok(data) = serde_json::from_reader::<_, AnchorFilePayload>(file) {
                history = data.compaction_history;
            }
        }
    }

    // 2. Discover timestamped .bak files on disk to backfill any missing prior events
    if let Ok(entries) = fs::read_dir(logs_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name.starts_with("transcript.jsonl.bak_") {
                let ts_part = name.trim_start_matches("transcript.jsonl.bak_");
                let exists = history.iter().any(|h| h.backup_file.contains(ts_part));
                if !exists {
                    let display_time = if ts_part.len() >= 15 {
                        format!(
                            "{}:{}:{}",
                            &ts_part[9..11],
                            &ts_part[11..13],
                            &ts_part[13..15]
                        )
                    } else {
                        ts_part.to_string()
                    };
                    let file_bytes = entry.metadata().map(|m| m.len() as usize).unwrap_or(0);
                    history.push(CompactionEvent {
                        timestamp_iso: format!("2026-09-02T{}Z", display_time),
                        timestamp_display: display_time,
                        trigger: "Checkpoint Snapshot".to_string(),
                        anchored_step: 0,
                        bytes_before: file_bytes,
                        bytes_after: 0,
                        reduction_pct: 0.0,
                        backup_file: path.to_string_lossy().to_string(),
                        artifact_file: "".to_string(),
                    });
                }
            }
        }
    }

    history.sort_by(|a, b| a.timestamp_display.cmp(&b.timestamp_display));
    history
}

pub fn write_artifact_metadata(markdown_path: &Path, summary: &str) -> std::io::Result<()> {
    let parent_dir = markdown_path.parent().unwrap_or_else(|| Path::new("."));
    let filename_str = markdown_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let meta_filename = format!("{}.metadata.json", filename_str);
    let meta_path = parent_dir.join(meta_filename);
    let now_iso = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);

    let meta_data = json!({
        "summary": summary,
        "updatedAt": now_iso,
        "userFacing": true,
        "requestFeedback": false
    });

    let mut tmp_file = Builder::new()
        .prefix(".shake_meta_")
        .tempfile_in(parent_dir)?;

    tmp_file.write_all(serde_json::to_string_pretty(&meta_data)?.as_bytes())?;
    tmp_file.flush()?;

    tmp_file.persist(&meta_path).map_err(|e| e.error)?;
    Ok(())
}

pub fn write_active_anchor(
    markdown_path: &Path,
    stats: &PruningStats,
    trigger_type: &str,
    backup_file_path: &str,
) -> std::io::Result<()> {
    if let Some(parent_dir) = markdown_path.parent() {
        let anchor_path = parent_dir.join("active_shake_anchor.json");
        let logs_dir = parent_dir.join(".system_generated/logs");
        let now_iso = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
        let now_display = Local::now().format("%H:%M:%S").to_string();

        let mut history = load_or_discover_history(&logs_dir, &anchor_path);

        let anchored_step = (stats.user_turns + stats.assistant_turns + stats.pruned_tools) as u64;

        let new_event = CompactionEvent {
            timestamp_iso: now_iso.clone(),
            timestamp_display: now_display,
            trigger: trigger_type.to_string(),
            anchored_step,
            bytes_before: stats.this_run_before_bytes,
            bytes_after: stats.this_run_after_bytes,
            reduction_pct: stats.this_run_savings_pct,
            backup_file: backup_file_path.to_string(),
            artifact_file: markdown_path.to_string_lossy().to_string(),
        };

        history.push(new_event);

        let t_size = logs_dir
            .join("transcript.jsonl")
            .metadata()
            .map(|m| m.len())
            .unwrap_or(stats.this_run_after_bytes as u64);

        let anchor_data = json!({
            "active": true,
            "shaken_file": markdown_path.to_string_lossy(),
            "anchored_at_step": anchored_step,
            "last_compacted_bytes": t_size,
            "last_attempt_timestamp": Utc::now().timestamp(),
            "topic": stats.topic_slug,
            "token_savings_pct": stats.reduction_pct,
            "raw_tokens": stats.raw_tokens,
            "pruned_tokens": stats.pruned_tokens,
            "timestamp": now_iso,
            "compaction_history": history
        });

        let mut tmp_file = Builder::new()
            .prefix(".shake_anchor_")
            .tempfile_in(parent_dir)?;

        tmp_file.write_all(serde_json::to_string_pretty(&anchor_data)?.as_bytes())?;
        tmp_file.flush()?;

        tmp_file.persist(&anchor_path).map_err(|e| e.error)?;
    }
    Ok(())
}
