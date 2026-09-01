use crate::models::PruningStats;
use chrono::Utc;
use serde_json::json;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

fn get_secure_tmp_path(target: &Path) -> std::path::PathBuf {
    let pid = std::process::id();
    let nanos = Utc::now().timestamp_nanos_opt().unwrap_or(0);
    target.with_extension(format!("{}.{}.tmp", pid, nanos))
}

pub fn write_artifact_metadata(markdown_path: &Path, summary: &str) -> std::io::Result<()> {
    let filename_str = markdown_path.file_name().unwrap_or_default().to_string_lossy();
    let meta_filename = format!("{}.metadata.json", filename_str);
    let meta_path = match markdown_path.parent() {
        Some(p) => p.join(meta_filename),
        None => Path::new(&meta_filename).to_path_buf(),
    };
    let tmp_path = get_secure_tmp_path(&meta_path);
    let now_iso = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);

    let meta_data = json!({
        "summary": summary,
        "updatedAt": now_iso,
        "userFacing": true,
        "requestFeedback": false
    });

    {
        let mut file = File::create(&tmp_path)?;
        file.write_all(serde_json::to_string_pretty(&meta_data)?.as_bytes())?;
        file.flush()?;
    }

    // Atomic rename
    fs::rename(&tmp_path, &meta_path)?;
    Ok(())
}

pub fn write_active_anchor(markdown_path: &Path, stats: &PruningStats) -> std::io::Result<()> {
    if let Some(parent_dir) = markdown_path.parent() {
        let anchor_path = parent_dir.join("active_shake_anchor.json");
        let tmp_path = get_secure_tmp_path(&anchor_path);
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
            "topic": stats.topic_slug,
            "token_savings_pct": stats.reduction_pct,
            "raw_tokens": stats.raw_tokens,
            "pruned_tokens": stats.pruned_tokens,
            "timestamp": now_iso
        });

        {
            let mut file = File::create(&tmp_path)?;
            file.write_all(serde_json::to_string_pretty(&anchor_data)?.as_bytes())?;
            file.flush()?;
        }

        // Atomic rename
        fs::rename(&tmp_path, &anchor_path)?;
    }
    Ok(())
}
