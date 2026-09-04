mod support;
use serde_json::{json, Value};
use shake_prune::atomic::{
    commit_staged_in_place_with_snapshot, recover_if_interrupted, write_intent_marker,
    SnapshotFingerprint,
};
use shake_prune::config::ShakeConfig;
use shake_prune::pruner::redact_secrets;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use support::{bin, run_shake};

/// P0-1: Master archive multi-turn synchronization test.
///
/// Simulates a real multi-turn session across multiple sequential compactions:
/// 1. Create initial turns 1..3 with large tool output, compact.
/// 2. Verify master archive contains steps 1..3 and receipts point to exact lines.
/// 3. Append new turns 4..6 with another large tool output.
/// 4. Compact again.
/// 5. Verify every receipt in the active transcript resolves to the EXACT line in
///    `transcript_full.jsonl`, and that reading those lines returns the exact original unpruned data.
#[test]
fn test_multi_turn_multi_compaction_archive_resolution() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let logs_dir = tmp_dir
        .path()
        .join(".gemini/brain/session-multi-turn/.system_generated/logs");
    fs::create_dir_all(&logs_dir).unwrap();
    let transcript_path = logs_dir.join("transcript.jsonl");
    let full_archive_path = logs_dir.join("transcript_full.jsonl");

    let tool_output_1 = "FIRST_LARGE_TOOL_OUTPUT_STEP_2\n".repeat(30);
    let tool_output_2 = "SECOND_LARGE_TOOL_OUTPUT_STEP_5\n".repeat(40);

    // Phase 1: Write initial turns (steps 1, 2, 3)
    {
        let mut f = File::create(&transcript_path).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 1, "type": "USER_INPUT", "source": "USER_EXPLICIT", "content": "<USER_REQUEST>Start build</USER_REQUEST>"})
        )
        .unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 2, "type": "RUN_COMMAND", "status": "DONE", "exit_code": 0, "content": tool_output_1})
        )
        .unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 3, "type": "PLANNER_RESPONSE", "content": "Build completed successfully."})
        )
        .unwrap();
    }

    // First compaction
    let out1 = run_shake(&[
        transcript_path.to_str().unwrap(),
        "--recent-user-turns",
        "0",
        "--recent-window",
        "0",
    ]);
    assert!(
        out1.status.success(),
        "First compaction failed: {}",
        String::from_utf8_lossy(&out1.stderr)
    );

    assert!(
        full_archive_path.exists(),
        "transcript_full.jsonl must exist after first compaction"
    );

    // Phase 2: Append new user, tool, and assistant turns (steps 4, 5, 6)
    {
        let mut f = OpenOptions::new()
            .append(true)
            .open(&transcript_path)
            .unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 4, "type": "USER_INPUT", "source": "USER_EXPLICIT", "content": "<USER_REQUEST>Run tests</USER_REQUEST>"})
        )
        .unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 5, "type": "RUN_COMMAND", "status": "DONE", "exit_code": 0, "content": tool_output_2})
        )
        .unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 6, "type": "PLANNER_RESPONSE", "content": "Tests passed."})
        )
        .unwrap();
    }

    // Second compaction
    let out2 = run_shake(&[
        transcript_path.to_str().unwrap(),
        "--recent-user-turns",
        "0",
        "--recent-window",
        "0",
    ]);
    assert!(
        out2.status.success(),
        "Second compaction failed: {}",
        String::from_utf8_lossy(&out2.stderr)
    );

    // Read all lines of the master archive
    let master_file = File::open(&full_archive_path).unwrap();
    let master_lines: Vec<String> = BufReader::new(master_file)
        .lines()
        .map(|l| l.unwrap())
        .collect();

    // Verify all steps 1..6 exist in master archive
    assert!(
        master_lines.len() >= 6,
        "Master archive must contain all steps from both compaction runs"
    );

    // Parse compacted transcript and verify receipts
    let compacted_content = fs::read_to_string(&transcript_path).unwrap();
    let receipt_regex =
        regex::Regex::new(r"\[PRUNED tool=(\w+) step=(\d+).*?archive=(.*?) line=(\d+)\]").unwrap();

    let mut found_receipts = 0;
    for line in compacted_content.lines() {
        if let Ok(val) = serde_json::from_str::<Value>(line) {
            if let Some(content) = val.get("content").and_then(|v| v.as_str()) {
                for cap in receipt_regex.captures_iter(content) {
                    found_receipts += 1;
                    let step: u64 = cap[2].parse().unwrap();
                    let archive_path = &cap[3];
                    let line_no: usize = cap[4].parse().unwrap();

                    assert_eq!(
                        Path::new(archive_path),
                        full_archive_path.canonicalize().unwrap().as_path(),
                        "Receipt must point to canonical transcript_full.jsonl"
                    );

                    assert!(
                        line_no > 0 && line_no <= master_lines.len(),
                        "Receipt line_no {} out of bounds (master lines: {})",
                        line_no,
                        master_lines.len()
                    );

                    let archived_line = &master_lines[line_no - 1];
                    let archived_val: Value =
                        serde_json::from_str(archived_line).unwrap_or_else(|_| {
                            panic!("Line {} in master archive is not valid JSON", line_no)
                        });

                    assert_eq!(
                        archived_val.get("step_index").and_then(|v| v.as_u64()),
                        Some(step),
                        "Receipt line {} in archive must match step_index {}",
                        line_no,
                        step
                    );

                    // Verify the archived line has the unpruned verbatim output
                    if step == 2 {
                        assert!(
                            archived_line.contains("FIRST_LARGE_TOOL_OUTPUT_STEP_2"),
                            "Archived line for step 2 must contain original tool output"
                        );
                    } else if step == 5 {
                        assert!(
                            archived_line.contains("SECOND_LARGE_TOOL_OUTPUT_STEP_5"),
                            "Archived line for step 5 must contain original tool output"
                        );
                    }
                }
            }
        }
    }

    assert_eq!(
        found_receipts, 2,
        "Both tool outputs (step 2 and step 5) must be pruned to receipts"
    );
}

/// P0-2: Crash recovery from `.shake_in_progress` intent marker.
#[test]
fn test_crash_recovery_from_intent_marker() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let logs_dir = tmp_dir.path().join("logs");
    fs::create_dir_all(&logs_dir).unwrap();
    let transcript = logs_dir.join("transcript.jsonl");
    let backup = logs_dir.join("transcript.jsonl.bak");

    let original_valid_jsonl = "{\"step_index\":1,\"type\":\"USER_INPUT\",\"content\":\"Valid question\"}\n{\"step_index\":2,\"type\":\"PLANNER_RESPONSE\",\"content\":\"Valid answer\"}\n";
    fs::write(&backup, original_valid_jsonl).unwrap();

    // Simulate crash after set_len(0): transcript is left empty (0 bytes)
    fs::write(&transcript, "").unwrap();

    // Write intent marker
    write_intent_marker(&transcript, &backup, 500).unwrap();

    // Trigger recovery
    let result = recover_if_interrupted(&transcript).expect("recovery must not fail");
    assert!(result.is_some(), "Recovery message must be produced");
    let msg = result.unwrap();
    assert!(msg.contains("Interrupted compaction recovered"));

    // Check transcript restored to original content
    let restored_content = fs::read_to_string(&transcript).unwrap();
    assert_eq!(restored_content, original_valid_jsonl);

    // Intent marker must be cleaned up
    let marker = logs_dir.join(".shake_in_progress");
    assert!(
        !marker.exists(),
        "Intent marker must be removed after successful recovery"
    );
}

/// P0-3: Pre-commit change detection aborts on concurrent external append.
#[test]
fn test_pre_commit_concurrent_write_aborts_without_truncation() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let transcript = tmp_dir.path().join("transcript.jsonl");
    let backup = tmp_dir.path().join("transcript.jsonl.bak");

    let original = "{\"step_index\":1,\"type\":\"USER_INPUT\",\"content\":\"Original\"}\n";
    fs::write(&transcript, original).unwrap();
    fs::write(&backup, original).unwrap();

    let mut handle = File::options()
        .read(true)
        .write(true)
        .open(&transcript)
        .unwrap();

    // Capture snapshot fingerprint
    let snapshot = SnapshotFingerprint::from_file(&handle).unwrap();

    // Simulate external uncooperative process appending to transcript
    {
        let mut external = OpenOptions::new().append(true).open(&transcript).unwrap();
        writeln!(
            external,
            "{{\"step_index\":2,\"type\":\"USER_INPUT\",\"content\":\"Concurrent Write\"}}"
        )
        .unwrap();
    }

    // Now attempt commit with the old snapshot fingerprint
    let staged = b"{\"step_index\":1,\"type\":\"USER_INPUT\",\"content\":\"Compacted\"}\n".to_vec();
    let lines =
        vec!["{\"step_index\":1,\"type\":\"USER_INPUT\",\"content\":\"Compacted\"}".to_string()];

    let res = commit_staged_in_place_with_snapshot(
        &mut handle,
        &transcript,
        &backup,
        &staged,
        &lines,
        Some(&snapshot),
    );

    assert!(
        res.is_err(),
        "Commit must abort when concurrent write is detected"
    );
    let err_str = res.err().unwrap().to_string();
    assert!(
        err_str.contains("Concurrent modification detected"),
        "Error message must clearly identify concurrent modification: {}",
        err_str
    );

    // Ensure the external append was NOT truncated or lost
    drop(handle);
    let current_content = fs::read_to_string(&transcript).unwrap();
    assert!(
        current_content.contains("Concurrent Write"),
        "External appended write must be fully preserved!"
    );
}

struct EnvGuard {
    saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl EnvGuard {
    fn new(keys: &[&'static str]) -> Self {
        let mut saved = Vec::new();
        for &k in keys {
            saved.push((k, std::env::var_os(k)));
            std::env::remove_var(k);
        }
        Self { saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (k, val) in &self.saved {
            match val {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }
}

/// P0-4: Configuration file and environment variable overrides.
#[test]
fn test_config_file_and_env_overrides() {
    let _guard = EnvGuard::new(&[
        "SHAKE_AUTO_DISABLE",
        "SHAKE_RECENT_USER_TURNS",
        "SHAKE_TOOLS_CAP",
        "SHAKE_ERRORS_CAP",
        "SHAKE_TOKEN_THRESHOLD_BYTES",
        "SHAKE_TOOL_BURST_THRESHOLD",
        "SHAKE_COOLDOWN_SECONDS",
        "SHAKE_GROWTH_DELTA_BYTES",
        "SHAKE_SECRET_REDACTION",
    ]);

    let tmp_dir = tempfile::tempdir().unwrap();
    let config_file = tmp_dir.path().join("shake.toml");

    let toml_data = r#"
    [auto]
    enabled = true
    size_threshold_bytes = 123456
    tool_burst_threshold = 12
    cooldown_seconds = 45
    growth_delta_bytes = 7890

    [retention]
    recent_user_turns = 7
    recent_tools_cap = 14
    recent_errors_cap = 21
    "#;
    fs::write(&config_file, toml_data).unwrap();

    let mut config = ShakeConfig::load_from_file(&config_file).unwrap();
    assert_eq!(config.auto.size_threshold_bytes, 123456);
    assert_eq!(config.auto.tool_burst_threshold, 12);
    assert_eq!(config.retention.recent_user_turns, 7);

    // Test environment variable overrides
    std::env::set_var("SHAKE_AUTO_DISABLE", "1");
    std::env::set_var("SHAKE_RECENT_USER_TURNS", "99");
    std::env::set_var("SHAKE_SECRET_REDACTION", "true");

    config.apply_env_overrides();

    assert!(
        !config.auto.enabled,
        "SHAKE_AUTO_DISABLE=1 must disable auto-shake"
    );
    assert_eq!(config.retention.recent_user_turns, 99);
    assert!(config.privacy.redact_secrets);
}

/// P1-3: Secret redaction helper test.
#[test]
fn test_secret_redaction_patterns() {
    let mock_gh = format!("{}_{}", "ghp", "MOCKTOKENVALUE1234567890123456789012");
    let mock_aws = format!("{}{}", "AKIA", "00000000MOCKTEST");
    let mock_bearer = format!(
        "{} {}",
        "Bearer", "mock_bearer_token_string_sample_value_123"
    );
    let mock_auth = format!(
        "{}: Basic {}",
        "Authorization", "mock_user_pass_credentials"
    );

    let text = format!(
        "Token {} and key {} and {} and {}",
        mock_gh, mock_aws, mock_bearer, mock_auth
    );
    let redacted = redact_secrets(&text);

    assert!(!redacted.contains("ghp_"));
    assert!(redacted.contains("[REDACTED_GH_TOKEN]"));
    assert!(!redacted.contains("AKIA00000000MOCKTEST"));
    assert!(redacted.contains("[REDACTED_AWS_KEY]"));
    assert!(!redacted.contains("mock_bearer_token"));
    assert!(redacted.contains("Bearer [REDACTED]"));
    assert!(!redacted.contains("mock_user_pass_credentials"));
    assert!(redacted.contains("Authorization: [REDACTED]"));
}

/// P2-5: Selective ephemeral message deduplication test.
#[test]
fn test_selective_ephemeral_message_deduplication() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let logs_dir = tmp_dir
        .path()
        .join(".gemini/brain/test-selective-eph/.system_generated/logs");
    fs::create_dir_all(&logs_dir).unwrap();
    let transcript_path = logs_dir.join("transcript.jsonl");

    {
        let mut f = File::create(&transcript_path).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 1, "type": "USER_INPUT", "content": "Question"})
        )
        .unwrap();
        // Shake notification 1
        writeln!(f, "{}", json!({"step_index": 2, "type": "EPHEMERAL_MESSAGE", "content": "[Context auto-compacted via /shake (notice 1)]"})).unwrap();
        // Third-party notification (must be preserved!)
        writeln!(f, "{}", json!({"step_index": 3, "type": "EPHEMERAL_MESSAGE", "content": "IMPORTANT_BUILD_NOTIFICATION"})).unwrap();
        // Shake notification 2 (latest shake notice)
        writeln!(f, "{}", json!({"step_index": 4, "type": "EPHEMERAL_MESSAGE", "content": "[Context auto-compacted via /shake (notice 2)]"})).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 5, "type": "PLANNER_RESPONSE", "content": "Answer"})
        )
        .unwrap();
    }

    let bin = bin();
    let output = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .arg("--recent-user-turns")
        .arg("0")
        .arg("--recent-window")
        .arg("0")
        .output()
        .unwrap();

    if !output.status.success() {
        eprintln!("STDOUT:\n{}", String::from_utf8_lossy(&output.stdout));
        eprintln!("STDERR:\n{}", String::from_utf8_lossy(&output.stderr));
    }
    assert!(output.status.success());
    let compacted = fs::read_to_string(&transcript_path).unwrap();

    // Old shake notice 1 must be pruned
    assert!(
        !compacted.contains("notice 1"),
        "Old shake notice 1 should be deduplicated"
    );

    // Latest shake notice 2 must be retained
    assert!(
        compacted.contains("notice 2"),
        "Latest shake notice 2 must be retained"
    );

    // Non-shake notification MUST be preserved verbatim!
    assert!(
        compacted.contains("IMPORTANT_BUILD_NOTIFICATION"),
        "Non-shake ephemeral messages must not be dropped"
    );
}

/// P2-9: Hardened restore subcommand creates `.pre_restore` backup.
#[test]
fn test_hardened_restore_creates_pre_restore_backup() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let transcript = tmp_dir.path().join("transcript.jsonl");
    let backup = tmp_dir.path().join("transcript.jsonl.bak");
    let pre_restore = tmp_dir.path().join("transcript.jsonl.pre_restore");

    let current_content =
        "{\"step_index\":1,\"type\":\"USER_INPUT\",\"content\":\"Current Content\"}\n";
    let backup_content =
        "{\"step_index\":1,\"type\":\"USER_INPUT\",\"content\":\"Original Backup Content\"}\n";

    fs::write(&transcript, current_content).unwrap();
    fs::write(&backup, backup_content).unwrap();

    let output = std::process::Command::new(bin())
        .arg("restore")
        .arg(&transcript)
        .output()
        .expect("Failed to execute restore");

    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Successfully restored"),
        "Stdout must confirm successful restore"
    );

    // Verify pre-restore snapshot was created with current content
    assert!(
        pre_restore.exists(),
        ".pre_restore backup must be created before restoring"
    );
    assert_eq!(fs::read_to_string(&pre_restore).unwrap(), current_content);

    // Verify transcript restored with backup content
    assert_eq!(fs::read_to_string(&transcript).unwrap(), backup_content);
}

/// CodeRabbit review fix: Records without explicit step_index or synthetic milestone markers
/// must not trigger a critical integrity failure when master archive index resolves only explicit steps.
#[test]
fn test_unindexed_synthetic_record_does_not_abort_compaction() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let logs_dir = tmp_dir
        .path()
        .join(".gemini/brain/test-synthetic/.system_generated/logs");
    fs::create_dir_all(&logs_dir).unwrap();
    let transcript_path = logs_dir.join("transcript.jsonl");

    {
        let mut f = File::create(&transcript_path).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 1, "type": "USER_INPUT", "content": "Question 1"})
        )
        .unwrap();
        // Record with tool output but no explicit step_index
        writeln!(
            f,
            "{}",
            json!({"type": "RUN_COMMAND", "content": "x".repeat(300), "exit_code": 0})
        )
        .unwrap();
        // Synthetic milestone record
        writeln!(
            f,
            "{}",
            json!({"is_milestone": true, "type": "PLANNER_RESPONSE", "content": "Milestone Checkpoint"})
        )
        .unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 4, "type": "USER_INPUT", "content": "Question 2"})
        )
        .unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 5, "type": "PLANNER_RESPONSE", "content": "Answer 2"})
        )
        .unwrap();
    }

    let bin = bin();
    let output = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .arg("--recent-user-turns")
        .arg("1")
        .arg("--recent-window")
        .arg("0")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "Compaction must succeed without integrity panic: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// P0-6: Master archive append must not corrupt JSONL if transcript_full.jsonl lacks trailing newline.
#[test]
fn test_master_archive_append_missing_trailing_newline() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let logs_dir = tmp_dir
        .path()
        .join(".gemini/brain/test-newline/.system_generated/logs");
    fs::create_dir_all(&logs_dir).unwrap();
    let transcript_path = logs_dir.join("transcript.jsonl");
    let full_path = logs_dir.join("transcript_full.jsonl");

    // Existing master archive WITHOUT trailing newline
    fs::write(
        &full_path,
        json!({"step_index": 1, "type": "USER_INPUT", "content": "Question 1"}).to_string(),
    )
    .unwrap();

    // Active transcript containing step 1 and step 2
    {
        let mut f = File::create(&transcript_path).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 1, "type": "USER_INPUT", "content": "Question 1"})
        )
        .unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 2, "type": "PLANNER_RESPONSE", "content": "Answer 1"})
        )
        .unwrap();
    }

    let bin = bin();
    let output = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .output()
        .unwrap();

    assert!(output.status.success(), "Compaction must succeed");

    // Verify all lines in transcript_full.jsonl are valid JSON and not concatenated
    let content = fs::read_to_string(&full_path).unwrap();
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 2, "Must contain exactly 2 distinct lines");
    for line in lines {
        assert!(
            serde_json::from_str::<Value>(line).is_ok(),
            "Line must be valid JSON: '{}'",
            line
        );
    }
}

/// P1-3: Duplicate step_index values in active transcript must not cause duplicate entries in master archive.
#[test]
fn test_master_archive_duplicate_step_index_deduplication() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let logs_dir = tmp_dir
        .path()
        .join(".gemini/brain/test-dup-step/.system_generated/logs");
    fs::create_dir_all(&logs_dir).unwrap();
    let transcript_path = logs_dir.join("transcript.jsonl");
    let full_path = logs_dir.join("transcript_full.jsonl");

    // Active transcript has two entries with step_index: 2
    {
        let mut f = File::create(&transcript_path).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 1, "type": "USER_INPUT", "content": "Question 1"})
        )
        .unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 2, "type": "RUN_COMMAND", "content": "first call", "exit_code": 0})
        )
        .unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 2, "type": "RUN_COMMAND", "content": "duplicate step", "exit_code": 0})
        )
        .unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 3, "type": "PLANNER_RESPONSE", "content": "Done"})
        )
        .unwrap();
    }

    let bin = bin();
    let output = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .output()
        .unwrap();

    assert!(output.status.success(), "Compaction must succeed");

    // Master archive must contain only one line with step_index: 2
    let content = fs::read_to_string(&full_path).unwrap();
    let mut step_2_count = 0;
    for line in content.lines().filter(|l| !l.trim().is_empty()) {
        if let Ok(val) = serde_json::from_str::<Value>(line) {
            if val.get("step_index").and_then(|v| v.as_u64()) == Some(2) {
                step_2_count += 1;
            }
        }
    }
    assert_eq!(
        step_2_count, 1,
        "step_index 2 must appear exactly once in master archive"
    );
}

/// P0-4: Output path must reject dotfiles, non-.md files, and preexisting non-shake files unless --force.
#[test]
fn test_output_path_security_restrictions() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let logs_dir = tmp_dir
        .path()
        .join(".gemini/brain/test-out-sec/.system_generated/logs");
    fs::create_dir_all(&logs_dir).unwrap();
    let transcript_path = logs_dir.join("transcript.jsonl");

    fs::write(
        &transcript_path,
        format!(
            "{}\n",
            json!({"step_index": 1, "type": "USER_INPUT", "content": "Hello"})
        ),
    )
    .unwrap();

    let bin = bin();

    // 1. Disallow dotfile
    let dotfile_out = logs_dir.join(".bashrc");
    let out = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .arg(&dotfile_out)
        .output()
        .unwrap();
    assert!(!out.status.success(), "Dotfile output must fail");
    assert!(String::from_utf8_lossy(&out.stderr).contains("hidden dotfile"));

    // 2. Disallow non-.md file
    let txt_out = logs_dir.join("report.txt");
    let out = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .arg(&txt_out)
        .output()
        .unwrap();
    assert!(!out.status.success(), "Non-.md output must fail");
    assert!(String::from_utf8_lossy(&out.stderr).contains(".md extension"));

    // 3. Disallow overwriting preexisting non-shake file without --force
    let existing_file = logs_dir.join("my_notes.md");
    fs::write(&existing_file, "Preexisting user notes").unwrap();

    let out = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .arg(&existing_file)
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "Overwriting non-shake file must fail without --force"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("already exists"));

    // 4. Overwrite allowed with --force
    let out = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .arg(&existing_file)
        .arg("--force")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "Overwriting with --force must succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// P0-5: Anchor notice consumption lifecycle prevents repeated turn-by-turn prompt spam.
#[test]
fn test_anchor_notice_consumption_lifecycle() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let art_dir = tmp_dir.path().join(".gemini/brain/test-anchor-lifecycle");
    fs::create_dir_all(&art_dir).unwrap();

    let md_file = art_dir.join("shake_summary.md");
    fs::write(&md_file, "# Summary").unwrap();

    let anchor_file = art_dir.join("active_shake_anchor.json");
    let anchor_payload = json!({
        "active": true,
        "injected": false,
        "shaken_file": md_file.to_string_lossy(),
        "anchored_at_step": 42,
    });
    fs::write(&anchor_file, anchor_payload.to_string()).unwrap();

    let bin = bin();

    let payload = json!({
        "artifactDirectoryPath": art_dir.to_string_lossy(),
        "invocationNum": 1
    })
    .to_string();

    // First call: anchor is active, emits injection notice
    let mut cmd = std::process::Command::new(&bin);
    cmd.arg("--hook")
        .env("HOME", tmp_dir.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    use std::io::Write;
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let stdout1 = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout1.contains("injectSteps"),
        "Turn 1 must inject anchor notice"
    );

    // Second call: anchor has been consumed, must emit {}
    let mut cmd2 = std::process::Command::new(&bin);
    cmd2.arg("--hook")
        .env("HOME", tmp_dir.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());
    let mut child2 = cmd2.spawn().unwrap();
    child2
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    let out2 = child2.wait_with_output().unwrap();
    assert!(out2.status.success());
    let stdout2 = String::from_utf8_lossy(&out2.stdout);
    assert_eq!(
        stdout2.trim(),
        "{}",
        "Turn 2 must NOT re-inject anchor notice"
    );
}

/// P1-5 / 7.4: When circuit breaker is open, anchor notice injection must be suppressed.
#[test]
fn test_anchor_circuit_breaker_suppresses_injection() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let art_dir = tmp_dir.path().join(".gemini/brain/test-cb-suppression");
    fs::create_dir_all(&art_dir).unwrap();

    let md_file = art_dir.join("shake_summary.md");
    fs::write(&md_file, "# Summary").unwrap();

    let anchor_file = art_dir.join("active_shake_anchor.json");
    let future_ts = chrono::Utc::now().timestamp() + 3600;
    let anchor_payload = json!({
        "active": true,
        "injected": false,
        "shaken_file": md_file.to_string_lossy(),
        "anchored_at_step": 10,
        "consecutive_failures": 5,
        "circuit_disabled_until": future_ts,
    });
    fs::write(&anchor_file, anchor_payload.to_string()).unwrap();

    let bin = bin();
    let payload = json!({
        "artifactDirectoryPath": art_dir.to_string_lossy(),
        "invocationNum": 1
    })
    .to_string();

    let mut cmd = std::process::Command::new(&bin);
    cmd.arg("--hook")
        .env("HOME", tmp_dir.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    use std::io::Write;
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        stdout.trim(),
        "{}",
        "Circuit breaker open must suppress anchor injection"
    );
}
