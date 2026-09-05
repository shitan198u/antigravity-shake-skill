use crate::models::{CompactionEvent, PruningStats};
use chrono::{Local, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::Builder;

#[derive(Deserialize, Serialize, Debug, Default, Clone)]
pub struct AnchorFilePayload {
    pub active: Option<bool>,
    pub injected: Option<bool>,
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
    /// Optional continuity card for deterministic state tracking (v0.2.0).
    #[serde(default)]
    pub continuity: Option<crate::continuity::ContinuityCard>,
}

/// Parse `transcript.jsonl.bak_YYYYMMDD_HHMMSS` suffix into
/// `(iso_8601, display_HH:MM:SS)`. Falls back to raw suffix on parse failure (B8).
pub fn parse_legacy_backup_timestamp(ts_part: &str) -> (String, String) {
    let bytes = ts_part.as_bytes();
    if bytes.len() >= 15
        && bytes.get(8) == Some(&b'_')
        && bytes[0..8].iter().all(|b| b.is_ascii_digit())
        && bytes[9..15].iter().all(|b| b.is_ascii_digit())
    {
        let y = std::str::from_utf8(&bytes[0..4]).unwrap_or("0000");
        let m = std::str::from_utf8(&bytes[4..6]).unwrap_or("00");
        let d = std::str::from_utf8(&bytes[6..8]).unwrap_or("00");
        let hh = std::str::from_utf8(&bytes[9..11]).unwrap_or("00");
        let mm = std::str::from_utf8(&bytes[11..13]).unwrap_or("00");
        let ss = std::str::from_utf8(&bytes[13..15]).unwrap_or("00");
        return (
            format!("{}-{}-{}T{}:{}:{}Z", y, m, d, hh, mm, ss),
            format!("{}:{}:{}", hh, mm, ss),
        );
    }
    // Safe fallback: take first 20 chars without panicking on char boundaries
    let safe_snippet: String = ts_part.chars().take(20).collect();
    (ts_part.to_string(), safe_snippet)
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
    continuity: Option<&crate::continuity::ContinuityCard>,
) -> std::io::Result<()> {
    if let Some(parent_dir) = markdown_path.parent() {
        let anchor_path = parent_dir.join("active_shake_anchor.json");
        let logs_dir = parent_dir.join(".system_generated/logs");
        let now_iso = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
        let now_display = Local::now().format("%H:%M:%S").to_string();

        let mut history = load_or_discover_history(&logs_dir, &anchor_path);

        let anchored_step = if stats.max_step_index > 0 {
            stats.max_step_index
        } else {
            (stats.user_turns + stats.assistant_turns + stats.pruned_tools) as u64
        };

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
            "injected": false,
            "shaken_file": markdown_path.to_string_lossy(),
            "anchored_at_step": anchored_step,
            "last_compacted_bytes": t_size,
            "last_attempt_timestamp": Utc::now().timestamp(),
            "topic": stats.topic_slug,
            "token_savings_pct": stats.this_run_savings_pct,
            "raw_tokens": stats.raw_tokens,
            "pruned_tokens": stats.pruned_tokens,
            "timestamp": now_iso,
            "compaction_history": history,
            "consecutive_failures": 0,
            "circuit_disabled_until": 0,
            "last_error": null,
            "continuity": continuity,
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

/// Mark anchor file as consumed and injected with strict 0600 permissions (B3, S2, D7).
pub fn consume_anchor(anchor_path: &Path) -> std::io::Result<()> {
    if !anchor_path.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(anchor_path)?;
    let mut json_val = serde_json::from_str::<serde_json::Value>(&content)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    if let Some(obj) = json_val.as_object_mut() {
        obj.insert("active".to_string(), json!(false));
        obj.insert("injected".to_string(), json!(true));
        obj.insert("injected_at".to_string(), json!(Utc::now().timestamp()));
        let updated = serde_json::to_string_pretty(&json_val)?;

        let parent = anchor_path.parent().unwrap_or_else(|| Path::new("."));
        let mut tmp_file = Builder::new()
            .prefix(".shake_anchor_")
            .tempfile_in(parent)?;
        tmp_file.write_all(updated.as_bytes())?;
        tmp_file.flush()?;
        tmp_file.persist(anchor_path).map_err(|e| e.error)?;
        crate::atomic::set_user_only_permissions(anchor_path);
    }
    Ok(())
}

static RE_TIMESTAMPED_ARTIFACT: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();

/// Prune old timestamped `shake_<topic>_<timestamp>.md` artifacts down to `keep_count` (B2).
/// Does not delete `shake_latest.md` or arbitrary custom files (e.g. `shake_notes.md`).
/// Also deletes corresponding `.metadata.json` sidecars.
pub fn prune_old_artifacts(artifact_dir: &Path, keep_count: usize) -> std::io::Result<usize> {
    if !artifact_dir.exists() || keep_count == 0 {
        return Ok(0);
    }

    let re_artifact = RE_TIMESTAMPED_ARTIFACT
        .get_or_init(|| regex::Regex::new(r"^shake_.*_\d{8}_\d{6}\.md$").unwrap());

    let mut timestamped_artifacts: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();

    if let Ok(entries) = fs::read_dir(artifact_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(fname) = path.file_name().and_then(|s| s.to_str()) {
                if re_artifact.is_match(fname) && fname != "shake_latest.md" {
                    let mtime = entry
                        .metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    timestamped_artifacts.push((mtime, path));
                }
            }
        }
    }

    // Sort ascending by modification time (oldest first)
    timestamped_artifacts.sort_by_key(|(mtime, _)| *mtime);

    let mut pruned_count = 0;
    if timestamped_artifacts.len() > keep_count {
        let to_remove = timestamped_artifacts.len() - keep_count;
        for (_, md_path) in timestamped_artifacts.into_iter().take(to_remove) {
            let _ = fs::remove_file(&md_path);
            let fname_str = md_path.file_name().unwrap_or_default().to_string_lossy();
            let meta_path = md_path.with_file_name(format!("{}.metadata.json", fname_str));
            let _ = fs::remove_file(meta_path);
            pruned_count += 1;
        }
    }

    Ok(pruned_count)
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
