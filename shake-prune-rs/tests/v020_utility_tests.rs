mod support;
use std::fs;
use std::process::Command;
use support::*;

#[test]
fn test_subcommand_preview_is_read_only() {
    let tmp = tempfile::tempdir().unwrap();
    let home = fake_home(&tmp);
    let transcript = trusted_transcript_path(&home, "conv_preview");
    let artifact_dir = home.join(".gemini/antigravity-ide/brain/conv_preview");

    TranscriptBuilder::new()
        .user("fix login bug")
        .assistant("analyzing logs")
        .tool_output("RUN_COMMAND", &"error line\n".repeat(100), 1)
        .assistant("found the bug")
        .write(&transcript);

    let orig_bytes = fs::metadata(&transcript).unwrap().len();

    // 1. Run preview
    let output = Command::new(bin())
        .args(["preview", transcript.to_str().unwrap()])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .output()
        .expect("failed to run shake-prune preview");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Context Compaction Preview"));
    assert!(stdout.contains("Continuity Anchor Preview"));
    assert!(stdout.contains("fix login bug"));

    // 2. Verify transcript is 100% UNCHANGED and NO artifacts written
    let after_bytes = fs::metadata(&transcript).unwrap().len();
    assert_eq!(orig_bytes, after_bytes);

    let latest_md = artifact_dir.join("shake_latest.md");
    assert!(
        !latest_md.exists(),
        "preview must not write shake_latest.md"
    );

    let bak_file = transcript.with_extension("jsonl.bak");
    assert!(!bak_file.exists(), "preview must not create .bak backup");

    let anchor_file = artifact_dir.join("active_shake_anchor.json");
    assert!(!anchor_file.exists(), "preview must not write anchor");
}

#[test]
fn test_subcommand_preview_json() {
    let tmp = tempfile::tempdir().unwrap();
    let home = fake_home(&tmp);
    let transcript = trusted_transcript_path(&home, "conv_preview_json");

    let mut builder = TranscriptBuilder::new().user("refactor component");
    for _ in 0..25 {
        builder = builder.tool_output(
            "RUN_COMMAND",
            &"cargo build error line output\n".repeat(20),
            0,
        );
    }
    builder = builder.assistant("done");
    builder.write(&transcript);

    let output = Command::new(bin())
        .args(["preview", transcript.to_str().unwrap(), "--json"])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .output()
        .expect("failed to run shake-prune preview --json");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(val["mode_resolved"], "standard");
    assert!(val["estimated_savings_pct"].as_f64().unwrap() > 0.0);
    assert_eq!(val["continuity"]["task"], "refactor component");
}

#[test]
fn test_subcommand_status() {
    let tmp = tempfile::tempdir().unwrap();
    let home = fake_home(&tmp);
    let transcript = trusted_transcript_path(&home, "conv_status");

    TranscriptBuilder::new()
        .user("small session")
        .assistant("done")
        .write(&transcript);

    // Text status
    let output = Command::new(bin())
        .args(["status", transcript.to_str().unwrap()])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Transcript Status"));
    assert!(stdout.contains("Context Size"));

    // JSON status
    let output_json = Command::new(bin())
        .args(["status", transcript.to_str().unwrap(), "--json"])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .output()
        .unwrap();

    assert!(output_json.status.success());
    let val: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&output_json.stdout)).unwrap();
    assert_eq!(val["user_turns"], 1);
    assert_eq!(val["recommendation"]["compact_recommended"], false);
}

#[test]
fn test_subcommand_run_creates_shake_latest_md_by_default() {
    let tmp = tempfile::tempdir().unwrap();
    let home = fake_home(&tmp);
    let transcript = trusted_transcript_path(&home, "conv_default_artifact");
    let artifact_dir = home.join(".gemini/antigravity-ide/brain/conv_default_artifact");

    TranscriptBuilder::new()
        .user("build auth flow")
        .assistant("working on it")
        .tool_output("RUN_COMMAND", &"compile step\n".repeat(40), 0)
        .assistant("auth flow completed")
        .write(&transcript);

    let output = Command::new(bin())
        .args(["run", transcript.to_str().unwrap()])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .output()
        .unwrap();

    assert!(output.status.success());

    // By default, must create shake_latest.md
    let latest_md = artifact_dir.join("shake_latest.md");
    assert!(
        latest_md.exists(),
        "shake-prune run must create shake_latest.md by default"
    );

    let anchor_file = artifact_dir.join("active_shake_anchor.json");
    assert!(anchor_file.exists());
    let anchor_content = fs::read_to_string(&anchor_file).unwrap();
    let anchor_val: serde_json::Value = serde_json::from_str(&anchor_content).unwrap();
    assert!(anchor_val["shaken_file"]
        .as_str()
        .unwrap()
        .ends_with("shake_latest.md"));
    assert_eq!(anchor_val["continuity"]["task"], "build auth flow");
}

#[test]
fn test_subcommand_run_timestamped_artifact_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let home = fake_home(&tmp);
    let transcript = trusted_transcript_path(&home, "conv_ts_artifact");
    let artifact_dir = home.join(".gemini/antigravity-ide/brain/conv_ts_artifact");

    TranscriptBuilder::new()
        .user("setup database schema")
        .assistant("schema applied")
        .tool_output("RUN_COMMAND", &"migration logs\n".repeat(30), 0)
        .assistant("done")
        .write(&transcript);

    let output = Command::new(bin())
        .args([
            "run",
            transcript.to_str().unwrap(),
            "--timestamped-artifact",
        ])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .output()
        .unwrap();

    assert!(output.status.success());

    // Should create shake_YYYYMMDD_...md
    let mut found_ts = false;
    for entry in fs::read_dir(&artifact_dir).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("shake_") && name.ends_with(".md") && name != "shake_latest.md" {
            found_ts = true;
            break;
        }
    }
    assert!(found_ts, "Expected timestamped artifact file to be created");
}

#[test]
fn test_subcommand_undo_restores_original_transcript() {
    let tmp = tempfile::tempdir().unwrap();
    let home = fake_home(&tmp);
    let transcript = trusted_transcript_path(&home, "conv_undo");

    let mut builder = TranscriptBuilder::new()
        .user("important task")
        .assistant("running diagnostics");
    for _ in 0..25 {
        builder = builder.tool_output("RUN_COMMAND", &"detailed output\n".repeat(20), 0);
    }
    builder = builder.assistant("complete");
    builder.write(&transcript);

    let orig_content = fs::read_to_string(&transcript).unwrap();

    // 1. Run compaction
    let run_out = Command::new(bin())
        .args(["run", transcript.to_str().unwrap()])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .output()
        .unwrap();
    assert!(run_out.status.success());

    let compacted_content = fs::read_to_string(&transcript).unwrap();
    assert_ne!(orig_content, compacted_content);
    assert!(compacted_content.contains("[PRUNED tool="));

    // 2. Run undo
    let undo_out = Command::new(bin())
        .args(["undo", transcript.to_str().unwrap()])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .output()
        .unwrap();
    assert!(undo_out.status.success());

    let restored_content = fs::read_to_string(&transcript).unwrap();
    assert_eq!(orig_content, restored_content);
}

#[test]
fn test_subcommand_show_inspects_archive() {
    let tmp = tempfile::tempdir().unwrap();
    let home = fake_home(&tmp);
    let transcript = trusted_transcript_path(&home, "conv_show");

    TranscriptBuilder::new()
        .user("run tests")
        .assistant("executing")
        .tool_output("RUN_COMMAND", "secret_archived_output_marker_12345", 0)
        .assistant("tests passed")
        .write(&transcript);

    // Compact so master archive is created and pruned
    let run_out = Command::new(bin())
        .args(["run", transcript.to_str().unwrap()])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .output()
        .unwrap();
    assert!(run_out.status.success());

    // Show step 3
    let show_out = Command::new(bin())
        .args([
            "show",
            transcript.to_str().unwrap(),
            "--step",
            "3",
            "--pretty",
        ])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .output()
        .unwrap();
    assert!(show_out.status.success());
    let stdout = String::from_utf8_lossy(&show_out.stdout);
    assert!(stdout.contains("Master Archive Record"));
    assert!(stdout.contains("secret_archived_output_marker_12345"));
}
