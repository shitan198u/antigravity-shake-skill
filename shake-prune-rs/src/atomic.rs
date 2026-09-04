//! Atomic in-place rewrite helpers with verified rollback, pre-commit
//! change detection, and crash recovery journaling.
//!
//! Design:
//! 1. Caller holds an exclusive `fs2` lock on `file` and has already created
//!    and size-verified `backup_path`.
//! 2. `stage_compacted_output` stages `compacted` into a temp file in the
//!    same directory, `sync_all`s it, and validates every generated line is
//!    JSON before touching the original.
//! 3. Pre-commit change detection (P0-3): Verifies file size and mtime have
//!    not changed between snapshot read and commit, detecting non-cooperative
//!    concurrent writers.
//! 4. Intent Journaling (P0-2): Writes `.shake_in_progress` intent marker before
//!    truncation. If process is SIGKILLed during rewrite, next startup/hook
//!    detects interrupted state and auto-recovers from backup.
//! 5. Truncates original (inode-preserving), writes staged bytes, flushes and
//!    fsyncs, sets 0600 permissions, fsyncs parent dir, and verifies length.
//! 6. Cleans up `.shake_in_progress` on successful verification.
//! 7. Any failure after truncation triggers `restore_from_backup`.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Best-effort fsync of the parent directory so the rename/truncate is
/// durable on sudden power loss. Ignored on platforms without support.
pub fn sync_parent_dir(path: &Path) {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
}

/// Sets restrictive permissions (0600: user read/write only) on Unix (P1-4).
pub fn set_user_only_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o600);
            let _ = std::fs::set_permissions(path, perms);
        }
    }
}

/// Snapshot fingerprint used for pre-commit concurrent modification detection (P0-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotFingerprint {
    pub len: u64,
    pub mtime_nanos: u128,
}

impl SnapshotFingerprint {
    pub fn from_file(file: &File) -> Result<Self, Box<dyn std::error::Error>> {
        let meta = file.metadata()?;
        let len = meta.len();
        let mtime_nanos = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        Ok(Self { len, mtime_nanos })
    }

    pub fn from_path(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let meta = std::fs::metadata(path)?;
        let len = meta.len();
        let mtime_nanos = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        Ok(Self { len, mtime_nanos })
    }

    pub fn verify_unmodified(&self, file: &File) -> Result<(), String> {
        let current = SnapshotFingerprint::from_file(file)
            .map_err(|e| format!("Failed to stat transcript before truncation: {}", e))?;
        if current.len != self.len || current.mtime_nanos != self.mtime_nanos {
            return Err(format!(
                "Concurrent modification detected! Original size={} bytes, mtime_ns={}; current size={} bytes, mtime_ns={}. Compaction aborted without truncation to prevent data loss.",
                self.len, self.mtime_nanos, current.len, current.mtime_nanos
            ));
        }
        Ok(())
    }
}

/// Path to intent marker `.shake_in_progress` in the transcript directory.
pub fn intent_marker_path(transcript_path: &Path) -> PathBuf {
    let parent = transcript_path.parent().unwrap_or_else(|| Path::new("."));
    parent.join(".shake_in_progress")
}

/// Writes intent marker before destructive truncation (P0-2).
pub fn write_intent_marker(
    transcript_path: &Path,
    backup_path: &Path,
    staged_bytes: usize,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let marker = intent_marker_path(transcript_path);
    let payload = serde_json::json!({
        "pid": std::process::id(),
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        "transcript_path": transcript_path.to_string_lossy(),
        "backup_path": backup_path.to_string_lossy(),
        "staged_bytes": staged_bytes,
    });
    let mut f = File::create(&marker)?;
    f.write_all(payload.to_string().as_bytes())?;
    f.flush()?;
    f.sync_all()?;
    set_user_only_permissions(&marker);
    Ok(marker)
}

/// Removes intent marker on successful completion (P0-2).
pub fn remove_intent_marker(transcript_path: &Path) {
    let marker = intent_marker_path(transcript_path);
    let _ = std::fs::remove_file(marker);
}

/// Checks if an interrupted compaction left the transcript corrupted or empty,
/// and automatically recovers from `transcript.jsonl.bak` (P0-2).
pub fn recover_if_interrupted(
    transcript_path: &Path,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let marker = intent_marker_path(transcript_path);
    let marker_exists = marker.exists();
    let target_meta = std::fs::metadata(transcript_path).ok();
    let is_empty_or_missing = target_meta.map(|m| m.len() == 0).unwrap_or(true);

    let backup_path = transcript_path.with_extension("jsonl.bak");
    let backup_len = std::fs::metadata(&backup_path)
        .map(|m| m.len())
        .unwrap_or(0);

    if (marker_exists || is_empty_or_missing) && backup_len > 0 {
        // Validate backup lines before restoring
        let bak_file = File::open(&backup_path)?;
        let reader = std::io::BufReader::new(bak_file);
        use std::io::BufRead;
        let mut valid_lines = 0;
        for line in reader.lines() {
            let l = line?;
            if !l.trim().is_empty() {
                if serde_json::from_str::<serde_json::Value>(&l).is_err() {
                    return Err(format!(
                        "Cannot auto-recover: backup file '{}' contains invalid JSON lines",
                        backup_path.display()
                    )
                    .into());
                }
                valid_lines += 1;
            }
        }
        if valid_lines == 0 {
            return Ok(None);
        }

        let mut file = File::options()
            .read(true)
            .write(true)
            .open(transcript_path)?;
        fs2::FileExt::lock_exclusive(&file)?;
        let restored = restore_from_backup(&mut file, &backup_path)?;
        set_user_only_permissions(transcript_path);
        remove_intent_marker(transcript_path);
        sync_parent_dir(transcript_path);
        return Ok(Some(format!(
            "Interrupted compaction recovered: restored '{}' from backup '{}' ({} bytes, {} lines)",
            transcript_path.display(),
            backup_path.display(),
            restored,
            valid_lines
        )));
    } else if marker_exists {
        remove_intent_marker(transcript_path);
    }
    Ok(None)
}

/// Restore `file` (rewound) from `backup_path` and verify the restored byte
/// length matches the backup. Returns restored bytes on success.
pub fn restore_from_backup(
    file: &mut File,
    backup_path: &Path,
) -> Result<u64, Box<dyn std::error::Error>> {
    let backup_len = std::fs::metadata(backup_path).map(|m| m.len()).unwrap_or(0);
    file.set_len(0).map_err(|e| {
        format!(
            "Rollback failed to truncate transcript before restore: {}",
            e
        )
    })?;
    file.seek(SeekFrom::Start(0))
        .map_err(|e| format!("Rollback failed to seek transcript before restore: {}", e))?;
    let mut bak = File::open(backup_path).map_err(|e| {
        format!(
            "Rollback failed to open backup '{}': {}",
            backup_path.display(),
            e
        )
    })?;
    let copied = std::io::copy(&mut bak, &mut *file)
        .map_err(|e| format!("Rollback failed while copying backup bytes: {}", e))?;
    file.flush()
        .map_err(|e| format!("Rollback failed to flush restored transcript: {}", e))?;
    file.sync_all()
        .map_err(|e| format!("Rollback failed to sync restored transcript: {}", e))?;
    // Read-back verification: on-disk length must match the backup.
    let restored_len = file
        .metadata()
        .map(|m| m.len())
        .map_err(|e| format!("Rollback failed to stat restored transcript: {}", e))?;
    if restored_len != backup_len || copied != backup_len {
        return Err(format!(
            "Rollback byte-length mismatch (backup {} bytes, copied {} bytes, on-disk {} bytes). Original backup preserved at '{}'; run `shake-prune restore`.",
            backup_len,
            copied,
            restored_len,
            backup_path.display()
        )
        .into());
    }
    set_user_only_permissions(backup_path);
    Ok(copied)
}

/// Validate staged lines are all JSON before any destructive write.
pub fn verify_generated_lines(lines: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    for line in lines {
        serde_json::from_str::<serde_json::Value>(line)
            .map_err(|e| format!("Corrupt JSON line generated during compaction: {}", e))?;
    }
    Ok(())
}

/// Stage `compacted` to a temp file in the same directory, sync it, and
/// return the staged bytes. Fails before the original is touched.
pub fn stage_compacted_output(
    transcript_path: &Path,
    compacted: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let bytes = compacted.as_bytes().to_vec();
    let parent = transcript_path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::Builder::new()
        .prefix(".shake_stage_")
        .tempfile_in(parent)
        .map_err(|e| format!("Failed to create staging temp file: {}", e))?;
    tmp.write_all(&bytes)
        .map_err(|e| format!("Failed to write staging temp file: {}", e))?;
    tmp.flush()
        .map_err(|e| format!("Failed to flush staging temp file: {}", e))?;
    tmp.as_file()
        .sync_all()
        .map_err(|e| format!("Failed to sync staging temp file: {}", e))?;
    // Read-back check of the staged file.
    let mut staged = Vec::with_capacity(bytes.len());
    {
        use std::io::Seek;
        let f = tmp.as_file_mut();
        f.seek(SeekFrom::Start(0))
            .map_err(|e| format!("Failed to rewind staging temp file: {}", e))?;
        f.read_to_end(&mut staged)
            .map_err(|e| format!("Failed to read back staging temp file: {}", e))?;
    }
    if staged != bytes {
        return Err(format!(
            "Staging byte verification failed (expected {} bytes, staged {} bytes)",
            bytes.len(),
            staged.len()
        )
        .into());
    }
    Ok(staged)
}

/// Commit staged bytes into `file` in place (inode-preserving) with verified
/// rollback on any error after truncation.
pub fn commit_staged_in_place(
    file: &mut File,
    transcript_path: &Path,
    backup_path: &Path,
    staged: &[u8],
    generated_lines: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    commit_staged_in_place_with_snapshot(
        file,
        transcript_path,
        backup_path,
        staged,
        generated_lines,
        None,
    )
}

/// Commit staged bytes into `file` in place (inode-preserving) with verified
/// rollback and pre-commit concurrent modification check (P0-1, P0-2, P0-3).
pub fn commit_staged_in_place_with_snapshot(
    file: &mut File,
    transcript_path: &Path,
    backup_path: &Path,
    staged: &[u8],
    generated_lines: &[String],
    snapshot: Option<&SnapshotFingerprint>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Pre-truncation validation: never truncate if staged content is invalid.
    verify_generated_lines(generated_lines)?;

    // P0-3: Pre-commit change detection. If file size or mtime has changed since
    // snapshot was read, an uncooperative external process appended new data.
    if let Some(snap) = snapshot {
        snap.verify_unmodified(file)?;
    }

    // P0-2: Write intent marker before destructive truncation so any power-loss/SIGKILL
    // can be detected and auto-recovered on the next run.
    let _ = write_intent_marker(transcript_path, backup_path, staged.len());

    let write_result: Result<(), Box<dyn std::error::Error>> =
        (|| -> Result<(), Box<dyn std::error::Error>> {
            file.set_len(0)?;
            file.seek(SeekFrom::Start(0))?;
            file.write_all(staged)?;
            file.flush()?;
            file.sync_all()?;
            Ok(())
        })();

    if let Err(write_err) = write_result {
        // Any error after set_len(0) must attempt rollback.
        match restore_from_backup(file, backup_path) {
            Ok(restored) => {
                remove_intent_marker(transcript_path);
                sync_parent_dir(transcript_path);
                return Err(format!(
                    "Failed to rewrite transcript in place ({}). Rolled back {} bytes from backup '{}'.",
                    write_err,
                    restored,
                    backup_path.display()
                )
                .into());
            }
            Err(rb_err) => {
                return Err(format!(
                    "CRITICAL: rewrite failed ({}) AND rollback failed ({}). Backup preserved at '{}'; run `shake-prune restore <transcript>` immediately.",
                    write_err,
                    rb_err,
                    backup_path.display()
                )
                .into());
            }
        }
    }

    // Post-write integrity verification (length + JSON validity).
    let verify_res: Result<(), Box<dyn std::error::Error>> = (|| {
        let written_len = file.metadata()?.len();
        if written_len == 0 && !staged.is_empty() {
            return Err("Compacted file size on disk is unexpectedly 0 bytes".into());
        }
        if written_len != staged.len() as u64 {
            return Err(format!(
                "Physical written disk length ({} bytes) does not match expected length ({} bytes)",
                written_len,
                staged.len()
            )
            .into());
        }
        verify_generated_lines(generated_lines)?;
        Ok(())
    })();

    if let Err(verify_err) = verify_res {
        eprintln!("CRITICAL ERROR: {}", verify_err);
        eprintln!("Initiating immediate rollback from atomic backup...");
        match restore_from_backup(file, backup_path) {
            Ok(_) => {
                remove_intent_marker(transcript_path);
                sync_parent_dir(transcript_path);
                return Err(format!(
                    "Critical: Post-compaction integrity verification failed ({}). Automatically rolled back from backup.",
                    verify_err
                )
                .into());
            }
            Err(rb_err) => {
                return Err(format!(
                    "Critical: Post-compaction integrity verification failed ({}) AND rollback failed ({}). Backup preserved at '{}'; run `shake-prune restore <transcript>` immediately.",
                    verify_err,
                    rb_err,
                    backup_path.display()
                )
                .into());
            }
        }
    }

    // Success: remove intent marker, harden permissions, and sync parent dir
    remove_intent_marker(transcript_path);
    set_user_only_permissions(transcript_path);
    set_user_only_permissions(backup_path);
    sync_parent_dir(transcript_path);
    Ok(())
}
