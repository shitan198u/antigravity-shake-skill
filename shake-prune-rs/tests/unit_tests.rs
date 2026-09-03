use serde_json::json;
use shake_prune::pruner::{
    compact_tool_call_args, estimate_tokens, extract_user_request_text,
    index_master_full_transcript, sanitize_markdown_snippet, shell_quote,
};
use shake_prune::slug::{extract_conversation_id, generate_topic_slug};
use std::fs::File;
use std::io::Write;

#[test]
fn test_slug_removes_xml_tags_and_urls() {
    let raw = "<USER_REQUEST>Fix https://github.com/rust-lang/rust please</USER_REQUEST>";
    let slug = generate_topic_slug(raw);
    assert!(!slug.contains("http"));
    assert!(!slug.contains("user_request"));
    assert!(slug.contains("fix"));
}

#[test]
fn test_slug_empty_fallback() {
    let slug = generate_topic_slug("");
    assert_eq!(slug, "session");

    let slug2 = generate_topic_slug("   \n\t  ");
    assert_eq!(slug2, "session");
}

#[test]
fn test_slug_word_limit() {
    let long_prompt = "one two three four five six seven eight nine ten";
    let slug = generate_topic_slug(long_prompt);
    let word_count = slug.split('_').count();
    assert!(
        word_count <= 4,
        "Slug should be at most 4 words, got: {}",
        slug
    );
}

#[test]
fn test_conversation_id_extraction() {
    let unix_path = "/home/user/.gemini/antigravity-ide/brain/123e4567-e89b-12d3-a456-426614174000/.system_generated/logs/transcript.jsonl";
    assert_eq!(
        extract_conversation_id(unix_path),
        "123e4567-e89b-12d3-a456-426614174000"
    );

    let win_path = r"C:\Users\Name\.gemini\antigravity\brain\abcdef01-2345-6789-abcd-ef0123456789\.system_generated\logs\transcript.jsonl";
    assert_eq!(
        extract_conversation_id(win_path),
        "abcdef01-2345-6789-abcd-ef0123456789"
    );

    let unknown = "/tmp/other/transcript.jsonl";
    assert_eq!(extract_conversation_id(unknown), "unknown-session");
}

#[test]
fn test_markdown_sanitization_html_entities() {
    let input = "<script>alert('xss')</script> & <div>test</div>";
    let sanitized = sanitize_markdown_snippet(input);
    assert!(!sanitized.contains("<script>"));
    assert!(sanitized.contains("&lt;script&gt;"));
    assert!(sanitized.contains("&amp;"));
    assert!(sanitized.contains("&lt;div&gt;"));
}

#[test]
fn test_markdown_sanitization_backticks() {
    let input = "Code with ```triple backticks``` inside";
    let sanitized = sanitize_markdown_snippet(input);
    assert!(!sanitized.contains("```"));
    assert!(sanitized.contains("` ` `"));
}

#[test]
fn test_shell_quote_escaping() {
    let normal = "/path/to/file.md";
    assert_eq!(shell_quote(normal), "'/path/to/file.md'");

    let with_quote = "/path/to/bob's file.md";
    assert_eq!(shell_quote(with_quote), "'/path/to/bob'\\''s file.md'");
}

#[test]
fn test_token_estimation() {
    assert_eq!(estimate_tokens(0), 1);
    assert_eq!(estimate_tokens(33), 10);
    assert_eq!(estimate_tokens(3300), 1000);
}

#[test]
fn test_compact_tool_call_args_write_to_file() {
    let mut args = serde_json::Map::new();
    args.insert("TargetFile".into(), json!("/path/to/target.rs"));
    args.insert("CodeContent".into(), json!("x".repeat(500)));

    compact_tool_call_args(
        "write_to_file",
        &mut args,
        42,
        "/archive/transcript_full.jsonl",
        100,
    );

    let code_content = args.get("CodeContent").unwrap().as_str().unwrap();
    assert!(code_content.contains("[PRUNED tool=write_to_file step=42"));
    assert!(code_content.contains("archive=/archive/transcript_full.jsonl line=100]"));
}

#[test]
fn test_compact_tool_call_args_replace_file_content() {
    let mut args = serde_json::Map::new();
    args.insert("TargetFile".into(), json!("/path/to/file.rs"));
    args.insert("ReplacementContent".into(), json!("y".repeat(500)));
    args.insert("TargetContent".into(), json!("z".repeat(500)));

    compact_tool_call_args(
        "replace_file_content",
        &mut args,
        88,
        "/archive/transcript_full.jsonl",
        200,
    );

    let repl = args.get("ReplacementContent").unwrap().as_str().unwrap();
    assert!(repl.contains("[PRUNED tool=replace_file_content step=88"));
    assert!(repl.contains("archive=/archive/transcript_full.jsonl line=200]"));

    let target = args.get("TargetContent").unwrap().as_str().unwrap();
    assert_eq!(target, "[Original target code snippet]");
}

#[test]
fn test_compact_tool_call_args_run_command_heredoc() {
    let mut args = serde_json::Map::new();
    let cmd = "cat << 'EOF' > test.rs\nfn main() {\n    println!(\"hello\");\n}\nEOF";
    args.insert("CommandLine".into(), json!(cmd));

    compact_tool_call_args(
        "run_command",
        &mut args,
        15,
        "/archive/transcript_full.jsonl",
        50,
    );

    let cmd_str = args.get("CommandLine").unwrap().as_str().unwrap();
    assert!(cmd_str.contains("[PRUNED heredoc command="));
    assert!(cmd_str.contains("test.rs"));
    assert!(cmd_str.contains("line=50]"));
}

#[test]
fn test_index_master_full_transcript_skips_synthetic_and_milestones() {
    let tmp = tempfile::tempdir().unwrap();
    let full_path = tmp.path().join("transcript_full.jsonl");

    {
        let mut f = File::create(&full_path).unwrap();
        // Line 1: Normal step 1
        writeln!(f, "{}", json!({"step_index": 1, "type": "USER_INPUT"})).unwrap();
        // Line 2: Synthetic milestone step (should be skipped)
        writeln!(f, "{}", json!({"step_index": 2, "type": "PLANNER_RESPONSE", "synthetic": true, "is_milestone": true})).unwrap();
        // Line 3: Real step 2
        writeln!(
            f,
            "{}",
            json!({"step_index": 2, "type": "PLANNER_RESPONSE", "content": "real"})
        )
        .unwrap();
        // Line 4: Duplicate step 2 (first occurrence should win)
        writeln!(
            f,
            "{}",
            json!({"step_index": 2, "type": "PLANNER_RESPONSE", "content": "duplicate"})
        )
        .unwrap();
        // Line 5: Step 3
        writeln!(f, "{}", json!({"step_index": 3, "type": "USER_INPUT"})).unwrap();
    }

    let map = index_master_full_transcript(&full_path);
    assert_eq!(map.get(&1), Some(&1));
    assert_eq!(
        map.get(&2),
        Some(&3),
        "Line 3 (real step 2) should be mapped, ignoring synthetic line 2!"
    );
    assert_eq!(map.get(&3), Some(&5));
}

#[test]
fn test_extract_user_request_text() {
    let raw = "<USER_REQUEST>\n  Please fix the build\n</USER_REQUEST>";
    assert_eq!(extract_user_request_text(raw), "Please fix the build");

    let no_tag = "Just a raw user string";
    assert_eq!(extract_user_request_text(no_tag), "Just a raw user string");
}
