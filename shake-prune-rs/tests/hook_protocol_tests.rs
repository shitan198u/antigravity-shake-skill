mod support;
use std::io::Write;
use std::process::{Command, Stdio};
use support::{bin, fake_home, trusted_transcript_path, TranscriptBuilder};

#[test]
fn test_hook_empty_stdin_returns_empty_json() {
    let output = Command::new(bin())
        .arg("--hook")
        .stdin(Stdio::null())
        .output()
        .expect("Failed to run shake-prune --hook");

    assert!(output.status.success(), "Hook must always exit with code 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "{}");
}

#[test]
fn test_hook_invalid_json_stdin_fails_open() {
    let mut child = Command::new(bin())
        .arg("--hook")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to spawn hook process");

    {
        let stdin = child.stdin.as_mut().expect("Failed to open stdin");
        stdin
            .write_all(b"this is not json { invalid [ garbage")
            .unwrap();
    }

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "Hook must fail-open with exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "{}");
}

#[test]
fn test_hook_untrusted_path_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let home = fake_home(&tmp);

    // Path outside ~/.gemini
    let untrusted_t = tmp.path().join("untrusted_transcript.jsonl");
    TranscriptBuilder::new()
        .user("hello")
        .assistant("world")
        .write(&untrusted_t);

    let payload = serde_json::json!({
        "transcriptPath": untrusted_t.to_string_lossy()
    });

    let mut child = Command::new(bin())
        .arg("--hook")
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        "{}",
        "Untrusted path outside ~/.gemini must be rejected"
    );
}

#[test]
fn test_hook_size_threshold_triggers_auto_shake() {
    let tmp = tempfile::tempdir().unwrap();
    let home = fake_home(&tmp);
    let transcript = trusted_transcript_path(&home, "conv_size_test");

    // Generate transcript exceeding 264 KB (~80k tokens)
    let big_output = "x".repeat(300_000);
    TranscriptBuilder::new()
        .user("Large task execution")
        .tool_output("RUN_COMMAND", &big_output, 0)
        .assistant("Completed large task")
        .write(&transcript);

    let payload = serde_json::json!({
        "transcriptPath": transcript.to_string_lossy()
    });

    let mut child = Command::new(bin())
        .arg("--hook")
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("injectSteps"),
        "Hook should trigger auto-compaction and inject notice on large transcript. Got: {}",
        stdout
    );
    assert!(stdout.contains("80k token threshold"));
}

#[test]
fn test_hook_corrupt_anchor_fails_open() {
    let tmp = tempfile::tempdir().unwrap();
    let home = fake_home(&tmp);
    let transcript = trusted_transcript_path(&home, "conv_corrupt_anchor");

    TranscriptBuilder::new()
        .user("task")
        .assistant("done")
        .write(&transcript);

    // Write corrupt anchor file in the conversation directory
    let conv_dir = transcript
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let anchor_path = conv_dir.join("active_shake_anchor.json");
    std::fs::write(&anchor_path, "corrupt json content").unwrap();

    let payload = serde_json::json!({
        "transcriptPath": transcript.to_string_lossy(),
        "artifactDirectoryPath": conv_dir.to_string_lossy()
    });

    let mut child = Command::new(bin())
        .arg("--hook")
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "Hook must fail-open on corrupt anchor"
    );
}

#[test]
fn test_hook_tool_burst_trigger_uses_tool_message() {
    let tmp = tempfile::tempdir().unwrap();
    let home = fake_home(&tmp);
    let transcript = trusted_transcript_path(&home, "conv_tool_burst");
    // Under 264KB but >= 20 unpruned tools -> tools trigger, not size trigger.
    let mut builder = TranscriptBuilder::new().user("autonomous loop task");
    for _ in 0..25 {
        builder = builder.tool_output("RUN_COMMAND", &"x".repeat(500), 0);
    }
    builder = builder.assistant("done");
    builder.write(&transcript);

    let payload = serde_json::json!({
        "transcriptPath": transcript.to_string_lossy()
    });

    let mut child = Command::new(bin())
        .arg("--hook")
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("injectSteps"),
        "tool burst should trigger auto-compaction, got: {}",
        stdout
    );
    assert!(
        stdout.contains("tool burst"),
        "tool-triggered message must name tool burst, not 80k threshold. Got: {}",
        stdout
    );
    assert!(
        !stdout.contains("80k token threshold"),
        "tool-triggered message must not claim size threshold. Got: {}",
        stdout
    );
}

#[test]
fn test_hook_bounded_stdin_handles_oversized_payload() {
    // 256 KB of garbage data (exceeding 64 KB limit)
    let huge_payload = "x".repeat(256 * 1024);

    let mut child = Command::new(bin())
        .arg("--hook")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(huge_payload.as_bytes());
    }

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "Hook must handle oversized stdin and exit 0"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        "{}",
        "Oversized invalid payload must safely return empty json"
    );
}
#[test]
fn test_hook_circuit_breaker_engages_for_uncompacted_session() {
    let tmp = tempfile::tempdir().unwrap();
    let home = fake_home(&tmp);
    let conv = "conv_breaker_fresh";
    let transcript = trusted_transcript_path(&home, conv);
    let conv_dir = home.join(".gemini/antigravity-ide/brain").join(conv);
    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();

    let big_output = "x".repeat(300_000);
    TranscriptBuilder::new()
        .user("Large task execution")
        .tool_output("RUN_COMMAND", &big_output, 0)
        .assistant("Completed large task")
        .write(&transcript);

    let anchor_path = conv_dir.join("active_shake_anchor.json");
    let now = chrono::Utc::now().timestamp();
    let anchor_json = serde_json::json!({
        "consecutive_failures": 3,
        "circuit_disabled_until": now + 1800,
        "last_error": "disk failure simulation"
    });
    std::fs::write(&anchor_path, anchor_json.to_string()).unwrap();

    let payload = serde_json::json!({
        "transcriptPath": transcript.to_string_lossy(),
        "artifactDirectoryPath": conv_dir.to_string_lossy()
    });

    let mut child = Command::new(bin())
        .arg("--hook")
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "Hook must exit 0 when circuit breaker is open"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        "{}",
        "Must emit empty json without injecting anchor"
    );

    let after_len = std::fs::metadata(&transcript).unwrap().len();
    assert!(
        after_len >= 300_000,
        "Transcript must remain uncompacted while breaker is open"
    );

    let log_path = home.join(".gemini/logs/shake_hook.log");
    assert!(log_path.exists(), "Hook log should exist");
    let log_content = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        log_content.contains("circuit breaker open"),
        "Log must record circuit breaker bypass. Got: {}",
        log_content
    );
}
#[test]
fn test_hook_mid_turn_invocation_bypasses_compaction_and_injection() {
    let tmp = tempfile::tempdir().unwrap();
    let home = fake_home(&tmp);
    let conv = "conv_mid_turn";
    let transcript = trusted_transcript_path(&home, conv);
    let conv_dir = home.join(".gemini/antigravity-ide/brain").join(conv);
    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();

    let big_output = "x".repeat(300_000);
    TranscriptBuilder::new()
        .user("Large task execution")
        .tool_output("RUN_COMMAND", &big_output, 0)
        .assistant("Processing step 1")
        .write(&transcript);

    // Mid-turn tool sequence (invocationNum: 2)
    let payload = serde_json::json!({
        "transcriptPath": transcript.to_string_lossy(),
        "artifactDirectoryPath": conv_dir.to_string_lossy(),
        "invocationNum": 2
    });

    let mut child = Command::new(bin())
        .arg("--hook")
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        "{}",
        "Mid-turn tool sequence (invocationNum > 1) must immediately output empty JSON"
    );

    // Transcript must NOT be compacted mid-flight
    let len = std::fs::metadata(&transcript).unwrap().len();
    assert!(
        len >= 300_000,
        "Transcript must remain unpruned during active mid-turn tool sequence"
    );
}

#[test]
fn test_hook_stop_event_compacts_in_background_silently() {
    let tmp = tempfile::tempdir().unwrap();
    let home = fake_home(&tmp);
    let conv = "conv_stop_event";
    let transcript = trusted_transcript_path(&home, conv);
    let conv_dir = home.join(".gemini/antigravity-ide/brain").join(conv);
    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();

    let mut builder = TranscriptBuilder::new().user("Historic bulky task");
    for _ in 0..10 {
        builder = builder.tool_output("RUN_COMMAND", &"x".repeat(35_000), 0);
    }
    builder = builder.user("Recent turn");
    for _ in 0..10 {
        builder = builder.tool_output("RUN_COMMAND", &"y".repeat(500), 0);
    }
    builder = builder.assistant("All done! Execution complete.");
    builder.write(&transcript);

    // Stop event payload when agent finishes responding
    let payload = serde_json::json!({
        "transcriptPath": transcript.to_string_lossy(),
        "artifactDirectoryPath": conv_dir.to_string_lossy(),
        "terminationReason": "model_stop",
        "fullyIdle": true
    });

    let mut child = Command::new(bin())
        .arg("--hook")
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        "{}",
        "Stop hook must conclude silently with {{}}"
    );

    let log_path = home.join(".gemini/logs/shake_hook.log");
    let log_text = std::fs::read_to_string(&log_path).unwrap_or_default();
    assert!(
        log_text.contains("Auto-shake complete"),
        "Stop hook must complete auto-shake compaction. Log: {}",
        log_text
    );

    let anchor_file = conv_dir.join("active_shake_anchor.json");
    assert!(
        anchor_file.exists(),
        "Anchor file must be created by Stop hook"
    );
    let anchor_data: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&anchor_file).unwrap()).unwrap();
    assert_eq!(
        anchor_data.get("active").and_then(|v| v.as_bool()),
        Some(true),
        "Anchor must remain active after Stop event so PreInvocation can inject it"
    );
    assert_eq!(
        anchor_data.get("injected").and_then(|v| v.as_bool()),
        Some(false),
        "Anchor must NOT be marked injected during Stop event"
    );
}

/// P0: Lock contention must fail open with exit 0 + {} and leave transcript intact.
#[test]
fn test_hook_lock_contention_fails_open() {
    use fs2::FileExt;
    let tmp = tempfile::tempdir().unwrap();
    let home = fake_home(&tmp);
    let transcript = trusted_transcript_path(&home, "conv_lock_contention");
    TranscriptBuilder::new()
        .user("Contended task")
        .tool_output("RUN_COMMAND", &"x".repeat(300_000), 0)
        .assistant("done")
        .write(&transcript);
    let before = std::fs::read(&transcript).unwrap();
    // Hold the exclusive lock for the duration of the hook run.
    let _guard = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&transcript)
        .unwrap();
    _guard.lock_exclusive().unwrap();
    let payload = serde_json::json!({ "transcriptPath": transcript.to_string_lossy() });
    let mut child = Command::new(bin())
        .arg("--hook")
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "contended hook must exit 0");
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "{}");
    drop(_guard);
    let after = std::fs::read(&transcript).unwrap();
    assert_eq!(before, after, "contended hook must not modify transcript");
}

/// P0: Expired watchdog budget must fail open with exit 0 + {}.
#[test]
fn test_hook_watchdog_expiry_fails_open() {
    let tmp = tempfile::tempdir().unwrap();
    let home = fake_home(&tmp);
    let transcript = trusted_transcript_path(&home, "conv_watchdog");
    TranscriptBuilder::new()
        .user("Watchdog task")
        .tool_output("RUN_COMMAND", &"x".repeat(300_000), 0)
        .assistant("done")
        .write(&transcript);
    let payload = serde_json::json!({ "transcriptPath": transcript.to_string_lossy() });
    let mut child = Command::new(bin())
        .arg("--hook")
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("SHAKE_HOOK_DEADLINE_MS", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "watchdog-expired hook must exit 0");
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "{}");
}
