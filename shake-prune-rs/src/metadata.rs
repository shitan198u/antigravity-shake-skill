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
    /// Consecutive auto-compaction failures (circuit breaker, P1-5).
    #[serde(default)]
    pub consecutive_failures: Option<u32>,
    /// Unix timestamp until which auto-shake is disabled after repeated failures.
    #[serde(default)]
    pub circuit_disabled_until: Option<i64>,
    /// Last failure message for diagnostics.
    #[serde(default)]
    pub last_error: Option<String>,
}

/// Parse `transcript.jsonl.bak_YYYYMMDD_HHMMSS` suffix into
/// `(iso_8601, display_HH:MM:SS)`. Falls back to raw suffix on parse failure.
fn parse_legacy_backup_timestamp(ts_part: &str) -> (String, String) {
    if ts_part.len() >= 15
        && ts_part.as_bytes().get(8) == Some(&b'_')
        && ts_part[..8].chars().all(|c| c.is_ascii_digit())
        && ts_part[9..15].chars().all(|c| c.is_ascii_digit())
    {
        let (y, m, d) = (&ts_part[0..4], &ts_part[4..6], &ts_part[6..8]);
        let (hh, mm, ss) = (&ts_part[9..11], &ts_part[11..13], &ts_part[13..15]);
        return (
            format!("{}-{}-{}T{}:{}:{}Z", y, m, d, hh, mm, ss),
            format!("{}:{}:{}", hh, mm, ss),
        );
    }
    // Fallback: preserve old HH:MM:SS slicing when possible.
    let display = if ts_part.len() >= 15 {
        format!(
            "{}:{}:{}",
            &ts_part[9..11],
            &ts_part[11..13],
            &ts_part[13..15]
        )
    } else {
        ts_part.to_string()
    };
    (ts_part.to_string(), display)
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
                    let (iso_time, display_time) = parse_legacy_backup_timestamp(ts_part);
                    let file_bytes = entry.metadata().map(|m| m.len() as usize).unwrap_or(0);
                    history.push(CompactionEvent {
                        timestamp_iso: iso_time,
                        timestamp_display: display_time,
                        trigger: "Checkpoint Snapshot".to_string(),
                        anchored_step: 0,
                        bytes_before: file_bytes,
                        bytes_after: 0,
                        reduction_pct: 0.0,
                        backup_file: path.to_string_lossy().to_string(),
                        artifact_file: "".to_string(),
                        duration_ms: None,
                        trigger_detail: Some("checkpoint".to_string()),
                    });
                }
            }
        }
    }

    history.sort_by(|a, b| a.timestamp_iso.cmp(&b.timestamp_iso));
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
    crate::atomic::set_user_only_permissions(&meta_path);
    Ok(())
}

pub fn write_active_anchor(
    markdown_path: &Path,
    stats: &PruningStats,
    trigger_type: &str,
    master_archive_path: &str,
) -> std::io::Result<()> {
    if let Some(parent_dir) = markdown_path.parent() {
        let anchor_path = parent_dir.join("active_shake_anchor.json");
        let logs_dir = parent_dir.join(".system_generated/logs");
        let now_iso = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
        let now_display = Local::now().format("%H:%M:%S").to_string();

        let mut history = load_or_discover_history(&logs_dir, &anchor_path);

        let anchored_step = (stats.user_turns + stats.assistant_turns + stats.pruned_tools) as u64;

        // `master_archive_path` is transcript_full.jsonl (permanent archive), not
        // the ephemeral transcript.jsonl.bak crash fallback (P2-5 naming fix).
        let new_event = CompactionEvent {
            timestamp_iso: now_iso.clone(),
            timestamp_display: now_display,
            trigger: trigger_type.to_string(),
            anchored_step,
            bytes_before: stats.this_run_before_bytes,
            bytes_after: stats.this_run_after_bytes,
            reduction_pct: stats.this_run_savings_pct,
            backup_file: master_archive_path.to_string(),
            artifact_file: markdown_path.to_string_lossy().to_string(),
            duration_ms: Some(stats.duration_ms),
            trigger_detail: Some(stats.trigger_detail.clone()),
        };

        history.push(new_event);
        if history.len() > 30 {
            let drop_count = history.len() - 30;
            history.drain(0..drop_count);
        }

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
            "compaction_history": history,
            "consecutive_failures": 0,
            "circuit_disabled_until": 0,
            "last_error": null,
        });

        let mut tmp_file = Builder::new()
            .prefix(".shake_anchor_")
            .tempfile_in(parent_dir)?;

        tmp_file.write_all(serde_json::to_string_pretty(&anchor_data)?.as_bytes())?;
        tmp_file.flush()?;

        tmp_file.persist(&anchor_path).map_err(|e| e.error)?;
        crate::atomic::set_user_only_permissions(&anchor_path);
    }
    Ok(())
}

/// Circuit breaker: auto-shake is disabled while `now < circuit_disabled_until`.
pub fn is_circuit_open(anchor: &AnchorFilePayload, now_ts: i64) -> bool {
    anchor.circuit_disabled_until.unwrap_or(0) > now_ts
}

/// Record an auto-compaction failure in the anchor file.
///
/// After 3 consecutive failures, disables auto-shake for 30 minutes (P1-5).
/// Best-effort: never fails the hook.
pub fn record_compaction_failure(anchor_path: &Path, err_msg: &str) {
    let now = Utc::now().timestamp();
    let mut payload = if anchor_path.exists() {
        File::open(anchor_path)
            .ok()
            .and_then(|f| serde_json::from_reader::<_, AnchorFilePayload>(f).ok())
            .unwrap_or_default()
    } else {
        AnchorFilePayload::default()
    };
    let failures = payload.consecutive_failures.unwrap_or(0).saturating_add(1);
    payload.consecutive_failures = Some(failures);
    payload.last_attempt_timestamp = Some(now);
    payload.last_error = Some(err_msg.chars().take(500).collect());
    if failures >= 3 {
        // 30-minute backoff after 3 consecutive failures.
        payload.circuit_disabled_until = Some(now + 1800);
    }
    if let Some(parent) = anchor_path.parent() {
        let _ = fs::create_dir_all(parent);
        if let Ok(mut tmp) = Builder::new().prefix(".shake_anchor_").tempfile_in(parent) {
            if tmp
                .write_all(
                    serde_json::to_string_pretty(&payload)
                        .unwrap_or_default()
                        .as_bytes(),
                )
                .is_ok()
            {
                let _ = tmp.flush();
                let _ = tmp.persist(anchor_path).map(|_| ());
            }
        }
    }
}
