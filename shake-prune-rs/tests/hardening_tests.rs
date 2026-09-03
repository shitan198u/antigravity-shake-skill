mod support;
use serde_json::{json, Value};
use shake_prune::atomic::restore_from_backup;
use shake_prune::metadata::{
    is_circuit_open, load_or_discover_history, record_compaction_failure, AnchorFilePayload,
};
use shake_prune::receipts::count_warnings;
use std::fs::{self, File};
use std::io::Write;
use std::time::Instant;
use support::{bin, TranscriptBuilder};

// P0-1/P0-2: restore helper must recover exact bytes and verify length.
#[test]
fn test_restore_from_backup_recovers_exact_bytes() {
    let tmp = tempfile::tempdir().unwrap();
    let transcript = tmp.path().join("transcript.jsonl");
    let backup = tmp.path().join("transcript.jsonl.bak");
    let original = "{\"step_index\":1,\"type\":\"USER_INPUT\",\"content\":\"hello\"}\nline2\n";
    fs::write(&transcript, original).unwrap();
    fs::write(&backup, original).unwrap();

    // Corrupt the live file, then restore via helper while holding a handle.
    fs::write(&transcript, "CORRUPT").unwrap();
    let mut handle = File::options()
        .read(true)
        .write(true)
        .open(&transcript)
        .unwrap();
    let restored = restore_from_backup(&mut handle, &backup).expect("restore must succeed");
    assert_eq!(restored, original.len() as u64);
    drop(handle);
    assert_eq!(fs::read_to_string(&transcript).unwrap(), original);
}

// P0-1: invalid staged content must never truncate the original.
#[test]
fn test_commit_rejects_invalid_json_before_truncation() {
    use shake_prune::atomic::commit_staged_in_place;
    let tmp = tempfile::tempdir().unwrap();
    let transcript = tmp.path().join("transcript.jsonl");
    let backup = tmp.path().join("transcript.jsonl.bak");
    let original = "{\"a\":1}\n";
    fs::write(&transcript, original).unwrap();
    fs::write(&backup, original).unwrap();
    let mut handle = File::options()
        .read(true)
        .write(true)
        .open(&transcript)
        .unwrap();
    let staged = b"{\"a\":1}\n".to_vec();
    // Second staged line is corrupt JSON -> pre-truncation validation must fail.
    let lines = vec!["{\"a\":1}".to_string(), "NOT_JSON".to_string()];
    let res = commit_staged_in_place(&mut handle, &transcript, &backup, &staged, &lines);
    assert!(res.is_err(), "corrupt staged lines must abort commit");
    drop(handle);
    assert_eq!(
        fs::read_to_string(&transcript).unwrap(),
        original,
        "original must be untouched when validation fails"
    );
}

// P0: compaction output must always be non-empty valid JSONL (no truncation gap).
#[test]
fn test_compaction_never_leaves_empty_transcript() {
    let tmp = tempfile::tempdir().unwrap();
    let logs = tmp
        .path()
        .join(".gemini/brain/test-nempty/.system_generated/logs");
    fs::create_dir_all(&logs).unwrap();
    let transcript = logs.join("transcript.jsonl");
    TranscriptBuilder::new()
        .user("hello")
        .tool_output("RUN_COMMAND", &"x".repeat(2000), 0)
        .assistant("done")
        .write(&transcript);
    let out = std::process::Command::new(bin())
        .arg(&transcript)
        .arg("--recent-user-turns")
        .arg("0")
        .arg("--recent-window")
        .arg("0")
        .output()
        .unwrap();
    assert!(out.status.success());
    let content = fs::read_to_string(&transcript).unwrap();
    assert!(!content.trim().is_empty(), "transcript must not be empty");
    for line in content.lines().filter(|l| !l.trim().is_empty()) {
        serde_json::from_str::<Value>(line).expect("every line must be valid JSON");
    }
    // Backup must exist and match pre-compaction size semantics (non-empty).
    let bak = logs.join("transcript.jsonl.bak");
    assert!(bak.exists());
    assert!(fs::metadata(&bak).unwrap().len() > 0);
}

// P1-5: circuit breaker opens after 3 consecutive failures.
#[test]
fn test_circuit_breaker_opens_after_repeated_failures() {
    let tmp = tempfile::tempdir().unwrap();
    let anchor = tmp.path().join("active_shake_anchor.json");
    record_compaction_failure(&anchor, "boom 1");
    record_compaction_failure(&anchor, "boom 2");
    let payload: AnchorFilePayload = serde_json::from_reader(File::open(&anchor).unwrap()).unwrap();
    assert_eq!(payload.consecutive_failures, Some(2));
    assert!(!is_circuit_open(&payload, chrono::Utc::now().timestamp()));
    record_compaction_failure(&anchor, "boom 3");
    let payload3: AnchorFilePayload =
        serde_json::from_reader(File::open(&anchor).unwrap()).unwrap();
    assert_eq!(payload3.consecutive_failures, Some(3));
    assert!(is_circuit_open(&payload3, chrono::Utc::now().timestamp()));
    assert!(payload3.last_error.unwrap().contains("boom 3"));
}

// P2-4: legacy backup timestamp parsed into real ISO date, not hardcoded.
#[test]
fn test_legacy_backup_timestamp_parses_real_date() {
    let tmp = tempfile::tempdir().unwrap();
    let logs = tmp.path().join("logs");
    fs::create_dir_all(&logs).unwrap();
    let anchor = tmp.path().join("anchor.json");
    fs::write(logs.join("transcript.jsonl.bak_20250815_123045"), "dummy").unwrap();
    let history = load_or_discover_history(&logs, &anchor);
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].timestamp_iso, "2025-08-15T12:30:45Z");
    assert_eq!(history[0].timestamp_display, "12:30:45");
}

// P2-7: warning counter must not false-positive on SWARM/WARRANTY.
#[test]
fn test_warning_counter_no_false_positives() {
    assert_eq!(count_warnings("swarm cluster ready warranty ok"), 0);
    assert!(count_warnings("warning: low disk WARN: boom") >= 2);
}

// P1-6: doctor --json emits machine-readable health.
#[test]
fn test_doctor_json_output() {
    let out = std::process::Command::new(bin())
        .arg("doctor")
        .arg("--json")
        .output()
        .unwrap();
    assert!(out.status.success());
    let val: Value = serde_json::from_slice(&out.stdout).expect("doctor --json must be JSON");
    for key in [
        "version",
        "binary_path",
        "storage_root_exists",
        "hook_registered",
    ] {
        assert!(val.get(key).is_some(), "missing key {}", key);
    }
}

// P1-6: --json compaction metrics include duration and trigger detail.
#[test]
fn test_compaction_json_includes_duration_and_trigger() {
    let tmp = tempfile::tempdir().unwrap();
    let transcript = tmp.path().join("transcript.jsonl");
    TranscriptBuilder::new()
        .user("task")
        .tool_output("RUN_COMMAND", &"y".repeat(500), 0)
        .assistant("done")
        .write(&transcript);
    let out = std::process::Command::new(bin())
        .arg(&transcript)
        .arg("--json")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with('{'))
        .expect("JSON metrics line missing");
    let val: Value = serde_json::from_str(line).unwrap();
    assert!(val.get("duration_ms").is_some());
    assert_eq!(
        val.get("trigger_detail").and_then(|v| v.as_str()),
        Some("manual")
    );
    assert!(val.get("master_archive").is_some());
}

// P1-4: master index cache is created and reused.
#[test]
fn test_master_index_cache_created() {
    let tmp = tempfile::tempdir().unwrap();
    let logs = tmp
        .path()
        .join(".gemini/brain/test-cache/.system_generated/logs");
    fs::create_dir_all(&logs).unwrap();
    let transcript = logs.join("transcript.jsonl");
    TranscriptBuilder::new()
        .user("hi")
        .tool_output("RUN_COMMAND", &"z".repeat(1000), 0)
        .assistant("ok")
        .write(&transcript);
    let out = std::process::Command::new(bin())
        .arg(&transcript)
        .arg("--recent-user-turns")
        .arg("0")
        .arg("--recent-window")
        .arg("0")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        logs.join("transcript_full.jsonl.index.json").exists(),
        "index sidecar cache must be created"
    );
}

// Fuzz robustness: unicode, huge lines, nested JSON must not crash or empty file.
#[test]
fn test_fuzz_robustness_malformed_inputs() {
    let tmp = tempfile::tempdir().unwrap();
    let logs = tmp
        .path()
        .join(".gemini/brain/test-fuzz/.system_generated/logs");
    fs::create_dir_all(&logs).unwrap();
    let transcript = logs.join("transcript.jsonl");
    {
        let mut f = File::create(&transcript).unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 1, "type": "USER_INPUT", "content": "héllo 🌍\u{0} null byte"})
        )
        .unwrap();
        writeln!(f, "{}", json!({"step_index": 2, "type": "RUN_COMMAND", "status": "DONE", "exit_code": 0, "content": "A".repeat(1_000_000)})).unwrap();
        let mut nested = json!({"a": 1});
        for _ in 0..50 {
            nested = json!({"nest": nested});
        }
        writeln!(
            f,
            "{}",
            json!({"step_index": 3, "type": "VIEW_FILE", "content": nested.to_string()})
        )
        .unwrap();
        writeln!(f, "not json at all").unwrap();
        writeln!(
            f,
            "{}",
            json!({"step_index": 4, "type": "PLANNER_RESPONSE", "content": "done"})
        )
        .unwrap();
    }
    let out = std::process::Command::new(bin())
        .arg(&transcript)
        .arg("--recent-user-turns")
        .arg("0")
        .arg("--recent-window")
        .arg("0")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "fuzzed input must not crash: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let content = fs::read_to_string(&transcript).unwrap();
    assert!(!content.trim().is_empty());
    assert!(content.contains("h\u{e9}llo") || content.contains("hello") || content.contains("h"));
}

// Performance regression: 5k-line transcript compacts in reasonable time.
#[test]
fn test_large_transcript_performance() {
    let tmp = tempfile::tempdir().unwrap();
    let logs = tmp
        .path()
        .join(".gemini/brain/test-perf/.system_generated/logs");
    fs::create_dir_all(&logs).unwrap();
    let transcript = logs.join("transcript.jsonl");
    {
        let mut f = File::create(&transcript).unwrap();
        for i in 1..=5000u64 {
            if i % 3 == 1 {
                writeln!(f, "{}", json!({"step_index": i, "type": "USER_INPUT", "content": format!("prompt {}", i)})).unwrap();
            } else if i % 3 == 2 {
                writeln!(f, "{}", json!({"step_index": i, "type": "RUN_COMMAND", "status": "DONE", "exit_code": 0, "content": format!("output {} ", i).repeat(10)})).unwrap();
            } else {
                writeln!(f, "{}", json!({"step_index": i, "type": "PLANNER_RESPONSE", "content": format!("reply {}", i)})).unwrap();
            }
        }
    }
    let start = Instant::now();
    let out = std::process::Command::new(bin())
        .arg(&transcript)
        .output()
        .unwrap();
    let elapsed = start.elapsed();
    assert!(out.status.success());
    assert!(
        elapsed.as_secs() < 30,
        "5k-line compaction took too long: {:?}",
        elapsed
    );
}
