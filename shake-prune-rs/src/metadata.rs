use crate::models::PruningStats;
use chrono::Utc;
use serde_json::json;
use std::io::Write;
use std::path::Path;
use tempfile::Builder;

pub fn write_artifact_metadata(markdown_path: &Path, summary: &str) -> std::io::Result<()> {
    let parent_dir = markdown_path.parent().unwrap_or_else(|| Path::new("."));
    let filename_str = markdown_path.file_name().unwrap_or_default().to_string_lossy();
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

    // Atomic persist with exclusive permissions
    tmp_file.persist(&meta_path).map_err(|e| e.error)?;
    Ok(())
}

pub fn write_active_anchor(markdown_path: &Path, stats: &PruningStats) -> std::io::Result<()> {
    if let Some(parent_dir) = markdown_path.parent() {
        let anchor_path = parent_dir.join("active_shake_anchor.json");
        let now_iso = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);

        let t_size = parent_dir.join(".system_generated/logs/transcript.jsonl")
            .metadata()
            .map(|m| m.len())
            .unwrap_or(stats.pruned_bytes as u64);

        let anchor_data = json!({
            "active": true,
            "shaken_file": markdown_path.to_string_lossy(),
            "anchored_at_step": stats.user_turns + stats.assistant_turns + stats.pruned_tools,
            "last_compacted_bytes": t_size,
            "last_attempt_timestamp": Utc::now().timestamp(),
            "topic": stats.topic_slug,
            "token_savings_pct": stats.reduction_pct,
            "raw_tokens": stats.raw_tokens,
            "pruned_tokens": stats.pruned_tokens,
            "timestamp": now_iso
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
