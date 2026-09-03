use serde_json::json;
use std::fs::{self, File};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

fn get_binary_path() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // drop test binary name
    if path.ends_with("deps") {
        path.pop();
    }
    let bin_name = if cfg!(windows) {
        "shake-prune.exe"
    } else {
        "shake-prune"
    };
    path.push(bin_name);
    path
}

#[cfg(unix)]
#[test]
fn test_inode_preservation() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let logs_dir = tmp_dir
        .path()
        .join(".gemini/brain/test-session/.system_generated/logs");
    fs::create_dir_all(&logs_dir).unwrap();
    let transcript_path = logs_dir.join("transcript.jsonl");

    // Write a multi-turn transcript with bloat
    {
        let mut f = File::create(&transcript_path).unwrap();
        writeln!(f, "{}", json!({"step_index": 1, "type": "USER_INPUT", "source": "USER_EXPLICIT", "content": "<USER_REQUEST>Hello</USER_REQUEST>"})).unwrap();
        writeln!(f, "{}", json!({"step_index": 2, "type": "RUN_COMMAND", "status": "DONE", "exit_code": 0, "content": "long verbose output\n".repeat(20)})).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 3, "type": "PLANNER_RESPONSE", "content": "Assistant reply"})
        )
        .unwrap();
        writeln!(f, "{}", json!({"step_index": 4, "type": "RUN_COMMAND", "status": "DONE", "exit_code": 0, "content": "recent command"})).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 5, "type": "PLANNER_RESPONSE", "content": "Final reply"})
        )
        .unwrap();
    }

    let inode_before = fs::metadata(&transcript_path).unwrap().ino();

    let bin = get_binary_path();
    let output = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .arg("--recent-user-turns")
        .arg("0")
        .arg("--recent-window")
        .arg("0")
        .output()
        .expect("Failed to execute shake-prune");

    assert!(
        output.status.success(),
        "shake-prune failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let inode_after = fs::metadata(&transcript_path).unwrap().ino();
    assert_eq!(
        inode_before, inode_after,
        "Inode changed! In-place truncate-and-rewrite violated."
    );
}

#[test]
fn test_safety_retention_invariants() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let logs_dir = tmp_dir
        .path()
        .join(".gemini/brain/test-safety/.system_generated/logs");
    fs::create_dir_all(&logs_dir).unwrap();
    let transcript_path = logs_dir.join("transcript.jsonl");

    let user_msg = "Please refactor the crypto module with zero breaking changes.";
    let assistant_msg = "I have refactored the module with zero breaking changes.";
    let error_content = "FATAL ERROR: Segment fault in libc.so.6 at 0x7fff";

    {
        let mut f = File::create(&transcript_path).unwrap();
        writeln!(f, "{}", json!({"step_index": 1, "type": "USER_INPUT", "source": "USER_EXPLICIT", "content": format!("<USER_REQUEST>{}</USER_REQUEST>", user_msg)})).unwrap();
        writeln!(f, "{}", json!({"step_index": 2, "type": "RUN_COMMAND", "status": "FAILED", "exit_code": 1, "content": error_content})).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 3, "type": "PLANNER_RESPONSE", "content": assistant_msg})
        )
        .unwrap();
        for i in 4..=10 {
            writeln!(f, "{}", json!({"step_index": i, "type": "RUN_COMMAND", "status": "DONE", "exit_code": 0, "content": format!("recent step {}", i)})).unwrap();
        }
    }

    let bin = get_binary_path();
    let output = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .arg("--recent-user-turns")
        .arg("0")
        .arg("--recent-window")
        .arg("0")
        .output()
        .expect("Failed to execute shake-prune");

    assert!(output.status.success());

    let compacted = fs::read_to_string(&transcript_path).unwrap();

    // User prompt is 100% verbatim
    assert!(
        compacted.contains(user_msg),
        "User prompt was corrupted or pruned!"
    );
    // Assistant text is 100% verbatim
    assert!(
        compacted.contains(assistant_msg),
        "Assistant response was corrupted or pruned!"
    );
    // Error stack trace is 100% verbatim
    assert!(
        compacted.contains(error_content),
        "Non-zero exit error trace was lost!"
    );
}

#[test]
fn test_active_working_window_preservation() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let logs_dir = tmp_dir
        .path()
        .join(".gemini/brain/test-window/.system_generated/logs");
    fs::create_dir_all(&logs_dir).unwrap();
    let transcript_path = logs_dir.join("transcript.jsonl");

    let old_bloat = "OLD_BLOAT_THAT_MUST_BE_PRUNED\n".repeat(50);
    let recent_bloat = "RECENT_BLOAT_THAT_MUST_BE_KEPT\n".repeat(50);

    {
        let mut f = File::create(&transcript_path).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 1, "type": "USER_INPUT", "content": "Turn 1"})
        )
        .unwrap();
        writeln!(f, "{}", json!({"step_index": 2, "type": "RUN_COMMAND", "status": "DONE", "exit_code": 0, "content": old_bloat})).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 3, "type": "PLANNER_RESPONSE", "content": "Turn 1 reply"})
        )
        .unwrap();

        // 6 recent steps
        writeln!(f, "{}", json!({"step_index": 4, "type": "RUN_COMMAND", "status": "DONE", "exit_code": 0, "content": recent_bloat})).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 5, "type": "PLANNER_RESPONSE", "content": "Turn 2 reply"})
        )
        .unwrap();
        writeln!(f, "{}", json!({"step_index": 6, "type": "RUN_COMMAND", "status": "DONE", "exit_code": 0, "content": "recent cmd 2"})).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 7, "type": "PLANNER_RESPONSE", "content": "Turn 3 reply"})
        )
        .unwrap();
        writeln!(f, "{}", json!({"step_index": 8, "type": "RUN_COMMAND", "status": "DONE", "exit_code": 0, "content": "recent cmd 3"})).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 9, "type": "PLANNER_RESPONSE", "content": "Turn 4 reply"})
        )
        .unwrap();
    }

    let bin = get_binary_path();
    let output = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .arg("--recent-user-turns")
        .arg("0")
        .output()
        .expect("Failed to execute shake-prune");

    assert!(output.status.success());
    let compacted = fs::read_to_string(&transcript_path).unwrap();

    // Old bloat must be pruned to structured receipt
    assert!(
        !compacted.contains("OLD_BLOAT_THAT_MUST_BE_PRUNED"),
        "Old bloat was not pruned!"
    );
    assert!(
        compacted.contains("[PRUNED tool=RUN_COMMAND step=2"),
        "Structured receipt missing for step 2!"
    );

    // Recent bloat within last 6 steps must be intact
    assert!(
        compacted.contains("RECENT_BLOAT_THAT_MUST_BE_KEPT"),
        "Active working window (step 4) was improperly pruned!"
    );
}

#[test]
fn test_thought_windowing_full_shake() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let logs_dir = tmp_dir
        .path()
        .join(".gemini/brain/test-thought/.system_generated/logs");
    fs::create_dir_all(&logs_dir).unwrap();
    let transcript_path = logs_dir.join("transcript.jsonl");

    // 25 turns: turns 1-5 should have thoughts pruned, turns 6-25 should keep thoughts
    {
        let mut f = File::create(&transcript_path).unwrap();
        for i in 1..=25 {
            writeln!(f, "{}", json!({"step_index": i * 2 - 1, "type": "USER_INPUT", "content": format!("User prompt {}", i)})).unwrap();
            writeln!(f, "{}", json!({"step_index": i * 2, "type": "PLANNER_RESPONSE", "thinking": format!("Thought scratchpad {}", i), "content": format!("Assistant answer {}", i)})).unwrap();
        }
    }

    let bin = get_binary_path();
    let output = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .arg("--full")
        .arg("--thought-window")
        .arg("20")
        .output()
        .expect("Failed to execute shake-prune");

    assert!(output.status.success());
    let compacted = fs::read_to_string(&transcript_path).unwrap();

    // Turns 1-5: thinking removed
    for i in 1..=5 {
        assert!(
            !compacted.contains(&format!("\"thinking\":\"Thought scratchpad {}\"", i)),
            "Turn {} thought should have been dropped!",
            i
        );
        assert!(
            compacted.contains(&format!("\"content\":\"Assistant answer {}\"", i)),
            "Turn {} answer was lost!",
            i
        );
    }

    // Turns 6-25: thinking preserved
    for i in 6..=25 {
        assert!(
            compacted.contains(&format!("\"thinking\":\"Thought scratchpad {}\"", i)),
            "Turn {} thought should have been retained!",
            i
        );
        assert!(
            compacted.contains(&format!("\"content\":\"Assistant answer {}\"", i)),
            "Turn {} answer was lost!",
            i
        );
    }
}

#[test]
fn test_single_master_backup_cleanup() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let logs_dir = tmp_dir
        .path()
        .join(".gemini/brain/test-cleanup/.system_generated/logs");
    fs::create_dir_all(&logs_dir).unwrap();
    let transcript_path = logs_dir.join("transcript.jsonl");
    let full_transcript = logs_dir.join("transcript_full.jsonl");

    // Create initial transcripts
    {
        let mut f = File::create(&transcript_path).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 1, "type": "USER_INPUT", "content": "Run 1"})
        )
        .unwrap();
        writeln!(f, "{}", json!({"step_index": 2, "type": "RUN_COMMAND", "status": "DONE", "exit_code": 0, "content": "stdout bloat\n".repeat(30)})).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 3, "type": "PLANNER_RESPONSE", "content": "ok"})
        )
        .unwrap();
    }
    fs::copy(&transcript_path, &full_transcript).unwrap();

    // Create 5 legacy redundant timestamped backups
    for i in 1..=5 {
        let legacy_bak = logs_dir.join(format!("transcript.jsonl.bak_20260902_00000{}", i));
        fs::write(&legacy_bak, "dummy legacy backup").unwrap();
    }

    let bin = get_binary_path();
    let output = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .arg("--recent-user-turns")
        .arg("0")
        .arg("--recent-window")
        .arg("0")
        .output()
        .expect("Failed to execute shake-prune");

    assert!(output.status.success());

    // Verify all legacy timestamped backups are purged
    for entry in fs::read_dir(&logs_dir).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        assert!(
            !name.contains(".bak_"),
            "Legacy backup {} was not purged!",
            name
        );
    }

    // Verify single atomic fallback transcript.jsonl.bak exists
    assert!(
        logs_dir.join("transcript.jsonl.bak").exists(),
        "Atomic fallback transcript.jsonl.bak must exist!"
    );

    // Verify receipt points to permanent transcript_full.jsonl
    let compacted = fs::read_to_string(&transcript_path).unwrap();
    assert!(
        compacted.contains("transcript_full.jsonl"),
        "Receipt should point to master transcript_full.jsonl!"
    );
}

#[test]
fn test_security_allowlist_rejection() {
    let bin = get_binary_path();
    let tmp_dir = tempfile::tempdir().unwrap();
    let transcript_path = tmp_dir.path().join("transcript.jsonl");
    {
        let mut f = File::create(&transcript_path).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 1, "type": "USER_INPUT", "content": "test"})
        )
        .unwrap();
    }

    // Attempt to write output to /etc
    let output = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .arg("/etc/malicious.md")
        .output()
        .expect("Failed to run binary");

    assert!(
        !output.status.success(),
        "Output path outside allowlist must fail!"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Security Error: Output path"),
        "Expected security allowlist error, got: {}",
        stderr
    );
}

#[test]
fn test_dry_run_flag() {
    let bin = get_binary_path();
    let tmp_dir = tempfile::tempdir().unwrap();
    let logs_dir = tmp_dir
        .path()
        .join(".gemini/brain/test-dryrun/.system_generated/logs");
    fs::create_dir_all(&logs_dir).unwrap();
    let transcript_path = logs_dir.join("transcript.jsonl");

    let initial_content = "{\"step_index\":1,\"type\":\"USER_INPUT\",\"content\":\"hello\"}\n";
    fs::write(&transcript_path, initial_content).unwrap();

    let output = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .arg("--dry-run")
        .output()
        .expect("Failed to run binary");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Dry Run Active"),
        "Report should state Dry Run Active!"
    );

    // Verify transcript was NOT modified
    let after_content = fs::read_to_string(&transcript_path).unwrap();
    assert_eq!(initial_content, after_content);

    // Verify no backup files were created
    assert!(!logs_dir.join("transcript.jsonl.bak").exists());
}

#[test]
fn test_version_flag() {
    let bin = get_binary_path();
    let output = std::process::Command::new(&bin)
        .arg("--version")
        .output()
        .expect("Failed to run binary");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("shake-prune 0.1.10"),
        "Expected version 0.1.10, got: {}",
        stdout
    );
}

#[test]
fn test_user_turn_working_window() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let logs_dir = tmp_dir
        .path()
        .join(".gemini/brain/test-turns/.system_generated/logs");
    fs::create_dir_all(&logs_dir).unwrap();
    let transcript_path = logs_dir.join("transcript.jsonl");

    // Create 15 conversational turns
    // Turns 1-5: should be pruned (older than 10 recent user turns)
    // Turns 6-15: should be kept 100% unpruned
    {
        let mut f = File::create(&transcript_path).unwrap();
        for i in 1..=15 {
            writeln!(f, "{}", json!({"step_index": i * 3 - 2, "type": "USER_INPUT", "content": format!("User turn {}", i)})).unwrap();
            let tool_output = format!("TOOL_OUTPUT_DATA_FOR_TURN_{}_END\n", i).repeat(20);
            writeln!(f, "{}", json!({"step_index": i * 3 - 1, "type": "RUN_COMMAND", "status": "DONE", "exit_code": 0, "content": tool_output})).unwrap();
            writeln!(f, "{}", json!({"step_index": i * 3, "type": "PLANNER_RESPONSE", "content": format!("Reply turn {}", i)})).unwrap();
        }
    }

    let bin = get_binary_path();
    let output = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .arg("--recent-user-turns")
        .arg("10")
        .output()
        .expect("Failed to execute shake-prune");

    assert!(output.status.success());
    let compacted = fs::read_to_string(&transcript_path).unwrap();

    // Turns 1-5 tool outputs must be pruned to structured receipts with line= pointers
    for i in 1..=5 {
        assert!(
            !compacted.contains(&format!("TOOL_OUTPUT_DATA_FOR_TURN_{}_END", i)),
            "Turn {} tool output should have been pruned!",
            i
        );
        assert!(
            compacted.contains(&format!("[PRUNED tool=RUN_COMMAND step={}", i * 3 - 1)),
            "Receipt missing for turn {}",
            i
        );
        assert!(
            compacted.contains(&format!("line={}", i * 3 - 1)),
            "Exact line number missing for turn {}",
            i
        );
    }

    // Turns 6-15 tool outputs must be 100% unpruned
    for i in 6..=15 {
        assert!(
            compacted.contains(&format!("TOOL_OUTPUT_DATA_FOR_TURN_{}_END", i)),
            "Turn {} tool output must remain in active working window!",
            i
        );
    }
}

#[test]
fn test_ephemeral_message_deduplication() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let logs_dir = tmp_dir
        .path()
        .join(".gemini/brain/test-eph/.system_generated/logs");
    fs::create_dir_all(&logs_dir).unwrap();
    let transcript_path = logs_dir.join("transcript.jsonl");

    {
        let mut f = File::create(&transcript_path).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 1, "type": "USER_INPUT", "content": "Hello"})
        )
        .unwrap();

        // 10 historical duplicate hook messages
        for i in 1..=10 {
            writeln!(f, "{}", json!({"step_index": i + 1, "type": "EPHEMERAL_MESSAGE", "content": format!("OLD_HOOK_NOTICE_{}", i)})).unwrap();
        }

        // Latest active anchor message
        writeln!(f, "{}", json!({"step_index": 12, "type": "EPHEMERAL_MESSAGE", "content": "LATEST_ACTIVE_ANCHOR_NOTICE"})).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 13, "type": "PLANNER_RESPONSE", "content": "Ready"})
        )
        .unwrap();
    }

    let bin = get_binary_path();
    let output = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .arg("--recent-user-turns")
        .arg("0")
        .arg("--recent-window")
        .arg("0")
        .output()
        .expect("Failed to execute shake-prune");

    assert!(output.status.success());
    let compacted = fs::read_to_string(&transcript_path).unwrap();

    // All old hook notices must be pruned
    for i in 1..=10 {
        assert!(
            !compacted.contains(&format!("OLD_HOOK_NOTICE_{}", i)),
            "Historical hook notice {} was not deduplicated!",
            i
        );
    }

    // Latest anchor notice must be retained
    assert!(
        compacted.contains("LATEST_ACTIVE_ANCHOR_NOTICE"),
        "Latest active anchor notice was improperly dropped!"
    );
}

#[test]
fn test_exact_line_number_indexing() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let logs_dir = tmp_dir
        .path()
        .join(".gemini/brain/test-index/.system_generated/logs");
    fs::create_dir_all(&logs_dir).unwrap();
    let transcript_path = logs_dir.join("transcript.jsonl");

    // Line 1: USER_INPUT
    // Line 2: RUN_COMMAND (bloat)
    // Line 3: PLANNER_RESPONSE
    {
        let mut f = File::create(&transcript_path).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 1, "type": "USER_INPUT", "content": "test line index"})
        )
        .unwrap();
        writeln!(f, "{}", json!({"step_index": 2, "type": "RUN_COMMAND", "status": "DONE", "exit_code": 0, "content": "line_index_target_bloat\n".repeat(40)})).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 3, "type": "PLANNER_RESPONSE", "content": "done"})
        )
        .unwrap();

        // Push step 2 outside the 6-step window
        for i in 4..=12 {
            writeln!(f, "{}", json!({"step_index": i, "type": "PLANNER_RESPONSE", "content": format!("pad {}", i)})).unwrap();
        }
    }

    let bin = get_binary_path();
    let output = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .arg("--recent-user-turns")
        .arg("0")
        .arg("--recent-window")
        .arg("0")
        .output()
        .expect("Failed to execute shake-prune");

    assert!(output.status.success());
    let compacted = fs::read_to_string(&transcript_path).unwrap();

    // Verify receipt has line=2
    assert!(
        compacted.contains("line=2]"),
        "Receipt did not contain exact line=2 pointer! Compacted: {}",
        compacted
    );

    // Read line 2 of the backup file and verify it contains the original bloat
    let bak_path = logs_dir.join("transcript.jsonl.bak");
    let bak_content = fs::read_to_string(&bak_path).unwrap();
    let line_2 = bak_content.lines().nth(1).expect("Backup must have line 2");
    assert!(
        line_2.contains("line_index_target_bloat"),
        "Backup line 2 did not match target content!"
    );
}

#[test]
fn test_heredoc_commandline_compaction() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let logs_dir = tmp_dir
        .path()
        .join(".gemini/brain/test-heredoc/.system_generated/logs");
    fs::create_dir_all(&logs_dir).unwrap();
    let transcript_path = logs_dir.join("transcript.jsonl");

    let giant_heredoc =
        "cat << 'EOF' > large_file.rs\n".to_string() + &"// code line\n".repeat(40) + "EOF";

    {
        let mut f = File::create(&transcript_path).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 1, "type": "USER_INPUT", "content": "write giant file"})
        )
        .unwrap();
        writeln!(
            f,
            "{}",
            json!({
                "step_index": 2,
                "type": "PLANNER_RESPONSE",
                "content": "writing file",
                "tool_calls": [{
                    "id": "call_1",
                    "name": "run_command",
                    "args": {
                        "CommandLine": giant_heredoc,
                        "Cwd": "/tmp"
                    }
                }]
            })
        )
        .unwrap();
        writeln!(f, "{}", json!({"step_index": 3, "type": "RUN_COMMAND", "status": "DONE", "exit_code": 0, "content": "written\n".repeat(40)})).unwrap();

        // Pad with steps so step 2 is outside recent window
        for i in 4..=15 {
            writeln!(f, "{}", json!({"step_index": i, "type": "PLANNER_RESPONSE", "content": format!("pad {}", i)})).unwrap();
        }
    }

    let bin = get_binary_path();
    let output = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .arg("--recent-user-turns")
        .arg("0")
        .arg("--recent-window")
        .arg("0")
        .output()
        .expect("Failed to execute shake-prune");

    assert!(output.status.success());
    let compacted = fs::read_to_string(&transcript_path).unwrap();

    // Verify heredoc in tool_calls CommandLine was compacted to receipt with line=2
    assert!(
        !compacted.contains("// code line"),
        "Raw heredoc lines were not pruned from CommandLine args!"
    );
    assert!(
        compacted.contains("[PRUNED heredoc command="),
        "Heredoc receipt missing in compacted stream!"
    );
    assert!(
        compacted.contains("large_file.rs"),
        "Filename missing in heredoc receipt!"
    );
    assert!(
        compacted.contains("line=2]"),
        "Heredoc receipt missing exact line=2 pointer!"
    );
}

#[test]
fn test_full_shake_marathon_milestone_horizon() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let logs_dir = tmp_dir
        .path()
        .join(".gemini/brain/test-horizon/.system_generated/logs");
    fs::create_dir_all(&logs_dir).unwrap();
    let transcript_path = logs_dir.join("transcript.jsonl");

    // Create 35 user turns
    {
        let mut f = File::create(&transcript_path).unwrap();
        for i in 1..=35 {
            writeln!(f, "{}", json!({"step_index": i * 2 - 1, "type": "USER_INPUT", "content": format!("User turn {} unique prompt", i)})).unwrap();
            writeln!(f, "{}", json!({"step_index": i * 2, "type": "PLANNER_RESPONSE", "thinking": format!("Thought for turn {}", i), "content": format!("Reply for turn {}", i)})).unwrap();
        }
    }

    let bin = get_binary_path();
    let output = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .arg("--full")
        .output()
        .expect("Failed to execute shake-prune");

    assert!(output.status.success());
    let compacted = fs::read_to_string(&transcript_path).unwrap();

    // 1. Genesis Turn 1 MUST be preserved 100% verbatim
    assert!(
        compacted.contains("User turn 1 unique prompt"),
        "Turn 1 Genesis prompt was improperly dropped!"
    );
    assert!(
        compacted.contains("Reply for turn 1"),
        "Turn 1 assistant reply was improperly dropped!"
    );

    // 2. Synthesized Milestone Block MUST exist
    assert!(
        compacted.contains("Historical Milestone Horizon"),
        "Milestone Horizon block was not synthesized!"
    );

    // 3. Middle turns (e.g. Turn 5) should be collapsed into the milestone
    assert!(
        !compacted.contains("User turn 5 unique prompt"),
        "Intermediate Turn 5 should have been collapsed into Milestone Horizon!"
    );

    // 4. Last 25 user turns (Turns 11-35) MUST be preserved verbatim
    for i in 11..=35 {
        assert!(
            compacted.contains(&format!("User turn {} unique prompt", i)),
            "Turn {} must be retained in active horizon!",
            i
        );
        assert!(
            compacted.contains(&format!("Reply for turn {}", i)),
            "Reply {} must be retained in active horizon!",
            i
        );
    }
}

#[test]
fn test_autonomous_loop_20_tools_cap() {
    let temp_dir = tempfile::tempdir().unwrap();
    let transcript_path = temp_dir.path().join("transcript.jsonl");

    // Simulate an autonomous loop within a single user prompt:
    // 1 user prompt, followed by 30 tool executions (tools 1 to 30)
    // Tool #5 failed with exit_code: 1
    {
        let mut f = File::create(&transcript_path).unwrap();
        writeln!(f, "{}", json!({"step_index": 1, "type": "USER_INPUT", "content": "Fix the entire test suite autonomously"})).unwrap();
        for i in 1..=30 {
            let is_err = i == 5;
            let exit = if is_err { 1 } else { 0 };
            let status = if is_err { "ERROR" } else { "DONE" };
            let content = if is_err {
                format!("CRITICAL_COMPILATION_ERROR_IN_TOOL_{}", i)
            } else {
                format!("Raw tool output for command execution number {} with lengthy compiler output {}", i, "x".repeat(300))
            };
            writeln!(
                f,
                "{}",
                json!({
                    "step_index": i + 1,
                    "type": "RUN_COMMAND",
                    "status": status,
                    "exit_code": exit,
                    "content": content
                })
            )
            .unwrap();
        }
    }

    let bin = get_binary_path();
    let output = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .output()
        .expect("Failed to execute shake-prune");

    assert!(output.status.success());
    let compacted = fs::read_to_string(&transcript_path).unwrap();

    // 1. User prompt must be preserved
    assert!(compacted.contains("Fix the entire test suite autonomously"));

    // 2. Tools 1 to 10 (older than the last 20) should be converted to receipts (except tool 5 which failed)
    for i in 1..=10 {
        if i == 5 {
            assert!(
                compacted.contains("CRITICAL_COMPILATION_ERROR_IN_TOOL_5"),
                "Failed tool 5 must be preserved verbatim even if older than 20 tools!"
            );
        } else {
            assert!(!compacted.contains(&format!("Raw tool output for command execution number {} with lengthy compiler output {}", i, "x".repeat(300))), "Tool {} should have been compacted into a receipt!", i);
        }
    }

    // 3. Tools 11 to 30 (the last 20 tool executions) MUST be preserved 100% unpruned
    for i in 11..=30 {
        assert!(
            compacted.contains(&format!(
                "Raw tool output for command execution number {} with lengthy compiler output {}",
                i,
                "x".repeat(300)
            )),
            "Tool {} must be preserved verbatim in the 20-tool cap window!",
            i
        );
    }
}

#[test]
fn test_ancient_error_pruned_after_30_tools() {
    let temp_dir = tempfile::tempdir().unwrap();
    let transcript_path = temp_dir.path().join("transcript.jsonl");

    // Simulate 45 tools:
    // Tool #5 failed (40 tools ago -> older than 30 -> must be pruned to receipt)
    // Tool #25 failed (20 tools ago -> within last 30 -> must be preserved verbatim)
    {
        let mut f = File::create(&transcript_path).unwrap();
        writeln!(f, "{}", json!({"step_index": 1, "type": "USER_INPUT", "content": "Run long regression test suite autonomously"})).unwrap();
        for i in 1..=45 {
            let is_err = i == 5 || i == 25;
            let exit = if is_err { 1 } else { 0 };
            let status = if is_err { "ERROR" } else { "DONE" };
            let content = if is_err {
                format!("FULL_UNCLAMPED_COMPILER_STACK_TRACE_FOR_TOOL_{}", i)
            } else {
                format!("Raw tool output for command execution number {} with lengthy compiler output {}", i, "x".repeat(300))
            };
            writeln!(
                f,
                "{}",
                json!({
                    "step_index": i + 1,
                    "type": "RUN_COMMAND",
                    "status": status,
                    "exit_code": exit,
                    "content": content
                })
            )
            .unwrap();
        }
    }

    let bin = get_binary_path();
    let output = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .output()
        .expect("Failed to execute shake-prune");

    assert!(output.status.success());
    let compacted = fs::read_to_string(&transcript_path).unwrap();

    // 1. Tool 5 (older than 30 tools) MUST be pruned to receipt with exit=1
    assert!(
        !compacted.contains("FULL_UNCLAMPED_COMPILER_STACK_TRACE_FOR_TOOL_5"),
        "Ancient tool 5 error should have been pruned!"
    );
    assert!(
        compacted.contains("[PRUNED tool=RUN_COMMAND step=6 exit=1"),
        "Receipt for ancient tool 5 missing or wrong exit code!"
    );

    // 2. Tool 25 (20 tools ago, within last 30) MUST be preserved 100% verbatim and unclamped
    assert!(
        compacted.contains("FULL_UNCLAMPED_COMPILER_STACK_TRACE_FOR_TOOL_25"),
        "Recent tool 25 error within last 30 tools must be preserved verbatim!"
    );
}

#[test]
fn test_marathon_thought_windowing_with_milestone_horizon() {
    let temp_dir = tempfile::tempdir().unwrap();
    let transcript_path = temp_dir.path().join("transcript.jsonl");

    // Create 35 conversational turns (> 30 turns: activates Milestone Horizon)
    // Each turn has user input + planner response with thinking
    {
        let mut f = File::create(&transcript_path).unwrap();
        for i in 1..=35 {
            writeln!(
                f,
                "{}",
                json!({
                    "step_index": i * 2 - 1,
                    "type": "USER_INPUT",
                    "content": format!("Marathon user turn {}", i)
                })
            )
            .unwrap();
            writeln!(
                f,
                "{}",
                json!({
                    "step_index": i * 2,
                    "type": "PLANNER_RESPONSE",
                    "thinking": format!("Detailed thought scratchpad for turn {}", i),
                    "content": format!("Assistant answer for turn {}", i)
                })
            )
            .unwrap();
        }
    }

    let bin = get_binary_path();
    let output = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .arg("--full")
        .arg("--thought-window")
        .arg("20")
        .output()
        .expect("Failed to execute shake-prune");

    assert!(output.status.success());
    let compacted = fs::read_to_string(&transcript_path).unwrap();

    // 1. Milestone Horizon must have synthesized
    assert!(
        compacted.contains("Historical Milestone Horizon"),
        "Milestone block missing!"
    );

    // 2. Turn 1 Genesis prompt must exist
    assert!(compacted.contains("Marathon user turn 1"));

    // 3. Genesis Turn 1 thoughts MUST be preserved 100% verbatim
    assert!(
        compacted.contains("\"Detailed thought scratchpad for turn 1\""),
        "Turn 1 Genesis thought must be preserved verbatim!"
    );

    // 4. Intermediate thoughts (e.g. Turn 12 in Milestone Horizon) should be windowed out
    assert!(
        !compacted.contains("\"Detailed thought scratchpad for turn 12\""),
        "Intermediate Turn 12 thought should have been windowed out!"
    );

    // 4. Recent thoughts in the last 20 assistant turns (e.g. Turn 25 to 35) MUST be preserved verbatim!
    for i in 25..=35 {
        assert!(
            compacted.contains(&format!("Detailed thought scratchpad for turn {}", i)),
            "Thought for recent marathon turn {} must be preserved!",
            i
        );
    }
}

#[test]
fn test_permanent_master_archive_initialization() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let logs_dir = tmp_dir
        .path()
        .join(".gemini/brain/test-init-full/.system_generated/logs");
    fs::create_dir_all(&logs_dir).unwrap();
    let transcript_path = logs_dir.join("transcript.jsonl");
    let full_transcript_path = logs_dir.join("transcript_full.jsonl");

    // Ensure transcript_full.jsonl does NOT exist
    assert!(!full_transcript_path.exists());

    // Create 12 steps
    {
        let mut f = File::create(&transcript_path).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 1, "type": "USER_INPUT", "content": "test init full"})
        )
        .unwrap();
        let large_cmd = format!(
            "echo bloat {}
",
            "x".repeat(300)
        );
        writeln!(f, "{}", json!({"step_index": 2, "type": "RUN_COMMAND", "status": "DONE", "exit_code": 0, "content": large_cmd})).unwrap();
        for i in 3..=12 {
            writeln!(f, "{}", json!({"step_index": i, "type": "PLANNER_RESPONSE", "content": format!("Reply {}", i)})).unwrap();
        }
    }

    let bin = get_binary_path();
    let output = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .arg("--recent-user-turns")
        .arg("0")
        .output()
        .expect("Failed to execute shake-prune");

    assert!(output.status.success());

    // 1. transcript_full.jsonl MUST have been initialized automatically!
    assert!(
        full_transcript_path.exists(),
        "Permanent master archive was not initialized!"
    );
    let full_content = fs::read_to_string(&full_transcript_path).unwrap();
    assert!(full_content.contains("test init full"));

    // 2. Receipts in compacted transcript MUST point to transcript_full.jsonl, NEVER .bak!
    let compacted = fs::read_to_string(&transcript_path).unwrap();
    assert!(
        compacted.contains("transcript_full.jsonl"),
        "Receipt should point to transcript_full.jsonl!"
    );
    assert!(
        !compacted.contains("transcript.jsonl.bak"),
        "Receipt should NEVER point to temporary .bak fallback!"
    );
}

#[test]
fn test_system_path_denylist_rejections() {
    let bin = get_binary_path();

    // 1. Invalid suffix like foo.jsonl.bak.txt must be rejected
    let tmp_dir = tempfile::tempdir().unwrap();
    let bad_suffix = tmp_dir.path().join("foo.jsonl.bak.txt");
    fs::write(&bad_suffix, "dummy").unwrap();

    let output = std::process::Command::new(&bin)
        .arg(&bad_suffix)
        .output()
        .expect("Failed to execute shake-prune");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Invalid file type"),
        "Invalid extension .bak.txt was not rejected! stderr: {}",
        stderr
    );
}

#[test]
fn test_duplicate_step_index_robustness() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let logs_dir = tmp_dir
        .path()
        .join(".gemini/brain/test-dups/.system_generated/logs");
    fs::create_dir_all(&logs_dir).unwrap();
    let transcript_path = logs_dir.join("transcript.jsonl");

    // Create transcript with duplicate step_index values
    {
        let mut f = File::create(&transcript_path).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 1, "type": "USER_INPUT", "content": "hello"})
        )
        .unwrap();
        writeln!(f, "{}", json!({"step_index": 2, "type": "RUN_COMMAND", "status": "DONE", "exit_code": 0, "content": "echo first step 2 bloat ".repeat(20)})).unwrap();
        writeln!(f, "{}", json!({"step_index": 2, "type": "RUN_COMMAND", "status": "DONE", "exit_code": 0, "content": "echo duplicate step 2 bloat ".repeat(20)})).unwrap();
        for i in 3..=10 {
            writeln!(f, "{}", json!({"step_index": i, "type": "PLANNER_RESPONSE", "content": format!("turn {}", i)})).unwrap();
        }
    }

    let bin = get_binary_path();
    let output = std::process::Command::new(&bin)
        .arg(&transcript_path)
        .arg("--recent-user-turns")
        .arg("0")
        .output()
        .expect("Failed to run shake-prune");

    assert!(
        output.status.success(),
        "Duplicate step_index must not crash compaction!"
    );
    let compacted = fs::read_to_string(&transcript_path).unwrap();
    assert!(compacted.contains("transcript_full.jsonl"));
}

#[test]
fn test_restore_subcommand() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let transcript_path = tmp_dir.path().join("transcript.jsonl");
    let bak_path = tmp_dir.path().join("transcript.jsonl.bak");

    // Write original and backup
    fs::write(&transcript_path, "MODIFIED_COMPACTED_CONTENT").unwrap();
    fs::write(&bak_path, "ORIGINAL_UNPRUNED_BACKUP_CONTENT").unwrap();

    let bin = get_binary_path();
    let output = std::process::Command::new(&bin)
        .arg("restore")
        .arg(&transcript_path)
        .output()
        .expect("Failed to execute shake-prune restore");

    assert!(output.status.success());
    let restored = fs::read_to_string(&transcript_path).unwrap();
    assert_eq!(restored, "ORIGINAL_UNPRUNED_BACKUP_CONTENT");
}

#[test]
fn test_doctor_subcommand() {
    let bin = get_binary_path();
    let output = std::process::Command::new(&bin)
        .arg("doctor")
        .output()
        .expect("Failed to execute shake-prune doctor");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Antigravity /shake Diagnostic Doctor"));
    assert!(stdout.contains("shake-prune"));
}
