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
