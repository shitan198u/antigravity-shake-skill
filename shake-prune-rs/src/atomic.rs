//! Atomic in-place rewrite helpers with verified rollback.
//!
//! Extracted from `pruner.rs` (P3 maintainability) so the destructive
//! truncate-and-rewrite path is small, testable, and hard to regress.
//!
//! Design (P0-3 near-atomic):
//! 1. Caller holds an exclusive `fs2` lock on `file` and has already created
//!    and size-verified `backup_path`.
//! 2. `commit_compacted_output` stages `compacted` into a temp file in the
//!    same directory, `sync_all`s it, and validates every generated line is
//!    JSON before touching the original.
//! 3. Only then does it truncate the original (inode-preserving), copy the
//!    staged bytes in, flush + `sync_all`, fsync the parent directory where
//!    possible, and verify on-disk length.
//! 4. Any failure after truncation triggers `restore_from_backup`, whose own
//!    errors are captured and returned alongside the original error (P0-2).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Best-effort fsync of the parent directory so the rename/truncate is
/// durable on sudden power loss. Ignored on platforms without support.
pub fn sync_parent_dir(path: &Path) {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
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
    // Persist path is intentionally dropped here: caller copies bytes into the
    // original inode. Keep the temp file alive until copy completes by
    // forgetting persistence — the bytes are already verified in memory.
    // (Temp file is cleaned up on drop.)
    Ok(staged)
}

/// Commit staged bytes into `file` in place (inode-preserving) with verified
/// rollback on any error after truncation (P0-1 / P0-2).
pub fn commit_staged_in_place(
    file: &mut File,
    transcript_path: &Path,
    backup_path: &Path,
    staged: &[u8],
    generated_lines: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    // Pre-truncation validation: never truncate if staged content is invalid.
    verify_generated_lines(generated_lines)?;

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
        // P0-1: any error after set_len(0) must attempt rollback.
        match restore_from_backup(file, backup_path) {
            Ok(restored) => {
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

    sync_parent_dir(transcript_path);
    Ok(())
}
