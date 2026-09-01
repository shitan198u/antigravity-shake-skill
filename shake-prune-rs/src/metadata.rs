use crate::models::PruningStats;
use chrono::Utc;
use serde_json::json;
use std::fs::File;
use std::io::Write;
use std::path::Path;

pub fn write_artifact_metadata(markdown_path: &Path, summary: &str) -> std::io::Result<()> {
    let meta_path = markdown_path.with_extension("md.metadata.json");
    let now_iso = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    let meta_data = json!({
        "artifactType": "ARTIFACT_TYPE_OTHER",
        "summary": summary,
        "updatedAt": now_iso
    });

    let mut file = File::create(meta_path)?;
    file.write_all(serde_json::to_string_pretty(&meta_data)?.as_bytes())?;
    Ok(())
}

pub fn write_active_anchor(markdown_path: &Path, stats: &PruningStats) -> std::io::Result<()> {
    if let Some(parent_dir) = markdown_path.parent() {
        let anchor_path = parent_dir.join("active_shake_anchor.json");
        let now_iso = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        let anchor_data = json!({
            "active": true,
            "shaken_file": markdown_path.to_string_lossy(),
            "anchored_at_step": stats.user_turns + stats.assistant_turns + stats.pruned_tools,
            "topic": stats.topic_slug,
            "token_savings_pct": stats.reduction_pct,
            "raw_tokens": stats.raw_tokens,
            "pruned_tokens": stats.pruned_tokens,
            "timestamp": now_iso
        });

        let mut file = File::create(anchor_path)?;
        file.write_all(serde_json::to_string_pretty(&anchor_data)?.as_bytes())?;
    }
    Ok(())
}
