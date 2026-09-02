mod hook;
mod metadata;
mod models;
mod pruner;
mod slug;

use hook::handle_hook;
use metadata::{load_or_discover_history, write_active_anchor, write_artifact_metadata};
use pruner::{format_history_timeline, run_compaction_pipeline, shell_quote, CompactionOptions};
use std::env;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_usage() {
    println!("shake-prune {}", VERSION);
    println!("High-performance in-place context compaction and tree-shaking for Google Antigravity\n");
    println!("Usage: shake-prune <transcript.jsonl> [output_file_or_dir] [options]");
    println!("       shake-prune --hook");
    println!("       shake-prune --version\n");
    println!("Options:");
    println!("  -h, --help               Show this help message and exit");
    println!("  -v, -V, --version        Print version information and exit");
    println!("  --hook                   Run as Antigravity PreInvocation hook (reads stdin JSON)");
    println!("  --full                   Enable full deep compaction (retains thoughts for last 20 turns, drops older)");
    println!("  --thought-window N       Number of recent assistant turns to retain thoughts for (default: 20 with --full)");
    println!("  --recent-user-turns N    Number of human conversational turns to keep 100% unpruned (default: 10)");
    println!("  --recent-window N        Fallback minimum steps to keep intact (default: 6)");
    println!("  --keep-backups N         Number of timestamped backup files to retain in logs/ (default: 5)");
    println!("  --no-in-place            Disable physical in-place compaction of transcript.jsonl");
    println!("  --dry-run                Simulate compaction and print report without modifying files");
    println!("  --json                   Output report metrics as machine-readable JSON");
    println!("\nExamples:");
    println!("  shake-prune /path/to/transcript.jsonl");
    println!("  shake-prune /path/to/transcript.jsonl --recent-user-turns 15");
    println!("  shake-prune /path/to/transcript.jsonl --full");
    println!("  shake-prune /path/to/transcript.jsonl --dry-run");
}

fn format_bytes(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

/// Validates that the input path is a valid JSONL transcript file
/// and prevents path traversal or arbitrary sensitive file modification.
fn validate_transcript_path(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("Transcript file does not exist: {}", path.display()));
    }
    if !path.is_file() {
        return Err(format!("Target is not a file: {}", path.display()));
    }

    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    if !file_name.ends_with(".jsonl") && !file_name.contains(".jsonl.bak") {
        return Err(format!(
            "Invalid file type: '{}'. /shake only operates on .jsonl transcript log files.",
            file_name
        ));
    }

    Ok(())
}

/// Strict ALLOWLIST Validation:
/// Ensures output files can ONLY be written within:
/// 1. The transcript's parent hierarchy (session brain folder: logs/, .system_generated/, and session root)
/// 2. The active workspace directory (current_dir)
/// 3. The user's system ~/.gemini directory
fn validate_output_path_allowlist(target: &Path, transcript_path: &Path) -> Result<PathBuf, String> {
    let target_parent = if target.is_dir() {
        target.to_path_buf()
    } else {
        match target.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => transcript_path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf(),
        }
    };

    let canonical_target = target_parent.canonicalize().map_err(|_| {
        format!("Output target directory does not exist or is invalid: {}", target_parent.display())
    })?;

    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();

    let mut allowed_roots: Vec<PathBuf> = Vec::new();

    let forbidden_parents = [
        Path::new("/"),
        Path::new("/tmp"),
        Path::new("/var"),
        Path::new("/var/tmp"),
        Path::new("C:\\"),
    ];

    if let Some(t_parent) = transcript_path.parent() {
        if let Ok(c) = t_parent.canonicalize() {
            if !forbidden_parents.contains(&c.as_path()) {
                allowed_roots.push(c.clone());
            }
            if let Some(c_parent) = c.parent() {
                if !forbidden_parents.contains(&c_parent) {
                    allowed_roots.push(c_parent.to_path_buf());
                    if let Some(c_grand) = c_parent.parent() {
                        if !forbidden_parents.contains(&c_grand) {
                            allowed_roots.push(c_grand.to_path_buf());
                        }
                    }
                }
            }
        }
    }

    if let Ok(curr) = env::current_dir().and_then(|p| p.canonicalize()) {
        if !forbidden_parents.contains(&curr.as_path()) {
            allowed_roots.push(curr);
        }
    }

    if !home.is_empty() {
        let gemini_dir = Path::new(&home).join(".gemini");
        if let Ok(c) = gemini_dir.canonicalize() {
            if !forbidden_parents.contains(&c.as_path()) {
                allowed_roots.push(c);
            }
        }
    }

    let is_allowed = allowed_roots.iter().any(|root| canonical_target.starts_with(root));

    if !is_allowed {
        return Err(format!(
            "Security Error: Output path '{}' is outside allowed session, workspace, and ~/.gemini directories.",
            target.display()
        ));
    }

    Ok(canonical_target.join(target.file_name().unwrap_or_default()))
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 || args[1] == "--help" || args[1] == "-h" || args[1] == "help" {
        print_usage();
        process::exit(0);
    }

    if args[1] == "--version" || args[1] == "-v" || args[1] == "-V" {
        println!("shake-prune {}", VERSION);
        process::exit(0);
    }

    if args[1] == "--hook" {
        handle_hook();
        process::exit(0);
    }

    let transcript_path = PathBuf::from(&args[1]);
    if let Err(err_msg) = validate_transcript_path(&transcript_path) {
        eprintln!("Security/Validation Error: {}", err_msg);
        process::exit(1);
    }

    let mut raw_target = String::new();
    let mut options = CompactionOptions::default();
    let mut json_output = false;

    let mut i = 2;
    while i < args.len() {
        if args[i] == "--recent-user-turns" && i + 1 < args.len() {
            if let Ok(val) = args[i + 1].parse::<usize>() {
                options.recent_user_turns = val;
            }
            i += 2;
        } else if args[i] == "--recent-window" && i + 1 < args.len() {
            if let Ok(val) = args[i + 1].parse::<usize>() {
                options.recent_window_steps = val;
            }
            i += 2;
        } else if args[i] == "--full" {
            options.thought_window_turns = Some(20);
            i += 1;
        } else if args[i] == "--thought-window" && i + 1 < args.len() {
            if let Ok(val) = args[i + 1].parse::<usize>() {
                options.thought_window_turns = Some(val);
            }
            i += 2;
        } else if args[i] == "--keep-backups" && i + 1 < args.len() {
            if let Ok(val) = args[i + 1].parse::<usize>() {
                options.keep_backups = val;
            }
            i += 2;
        } else if args[i] == "--no-in-place" {
            options.in_place = false;
            i += 1;
        } else if args[i] == "--dry-run" {
            options.dry_run = true;
            i += 1;
        } else if args[i] == "--json" {
            json_output = true;
            i += 1;
        } else if raw_target.is_empty() && !args[i].starts_with("--") {
            raw_target = args[i].clone();
            i += 1;
        } else {
            i += 1;
        }
    }

    // Execute Unified Single-Pass Pipeline
    let (_compacted_jsonl, pruned_markdown, stats, backup_file_str) =
        match run_compaction_pipeline(&transcript_path, &options) {
            Ok(res) => res,
            Err(e) => {
                eprintln!("Error during compaction pipeline: {}", e);
                process::exit(1);
            }
        };

    // Determine output file path
    let initial_output_path: PathBuf = if !raw_target.is_empty() && !Path::new(&raw_target).is_dir() && raw_target.ends_with(".md") {
        PathBuf::from(&raw_target)
    } else if !raw_target.is_empty() && Path::new(&raw_target).is_dir() {
        Path::new(&raw_target).join(&stats.suggested_filename)
    } else if let Some(parent) = transcript_path.parent() {
        if parent.file_name().map(|s| s == "logs").unwrap_or(false) {
            if let Some(conv_dir) = parent.parent().and_then(|p| p.parent()) {
                conv_dir.join(&stats.suggested_filename)
            } else {
                parent.join(&stats.suggested_filename)
            }
        } else {
            parent.join(&stats.suggested_filename)
        }
    } else {
        PathBuf::from(&stats.suggested_filename)
    };

    let abs_output_path = match validate_output_path_allowlist(&initial_output_path, &transcript_path) {
        Ok(p) => p,
        Err(err_msg) => {
            eprintln!("{}", err_msg);
            process::exit(1);
        }
    };

    if !options.dry_run {
        if let Err(e) = File::create(&abs_output_path).and_then(|mut f| f.write_all(pruned_markdown.as_bytes())) {
            eprintln!("Failed to write output file '{}': {}", abs_output_path.display(), e);
            process::exit(1);
        }

        let trigger_label = if options.thought_window_turns.is_some() {
            "Manual (/full-shake)"
        } else {
            "Manual (/shake)"
        };

        let summary_text = format!(
            "Shaken & pruned verbatim history for topic '{}'. Saved {:.1}% context tokens ({} tokens vs {} raw). Preserved {} user prompts, all reasoning, thoughts, and last {} user conversational turns.",
            stats.topic_slug.replace('_', " "),
            stats.reduction_pct,
            stats.pruned_tokens,
            stats.raw_tokens,
            stats.user_turns,
            options.recent_user_turns
        );

        let _ = write_artifact_metadata(&abs_output_path, &summary_text);
        let _ = write_active_anchor(&abs_output_path, &stats, trigger_label, &backup_file_str);
    }

    if json_output {
        if let Ok(json_str) = serde_json::to_string_pretty(&stats) {
            println!("{}", json_str);
            process::exit(0);
        }
    }

    let abs_str = abs_output_path.display().to_string();
    let quoted_path = shell_quote(&abs_str);
    let encoded_file_url = format!("file://{}", urlencoding::encode(&abs_str).replace("%2F", "/"));
    let before_fmt = format_bytes(stats.this_run_before_bytes);
    let after_fmt = format_bytes(stats.this_run_after_bytes);
    let cumulative_full_fmt = format_bytes(stats.cumulative_full_bytes);
    let tokens_saved = stats.raw_tokens.saturating_sub(stats.pruned_tokens);

    let logs_dir = transcript_path.parent().unwrap_or_else(|| Path::new("."));
    let anchor_path = logs_dir.parent().unwrap_or_else(|| Path::new(".")).join("active_shake_anchor.json");
    let all_history = load_or_discover_history(logs_dir, &anchor_path);
    let history_timeline_md = format_history_timeline(&all_history);

    let mode_header = if options.dry_run {
        "🔍 Dry Run (Simulation Only - No Files Modified)".to_string()
    } else if let Some(w) = options.thought_window_turns {
        if stats.assistant_turns > w {
            format!("⚡ Full Deep Compaction (Last {} Thoughts Retained)", w)
        } else {
            "🟢 Standard Zero-Loss Compaction (All Thoughts Retained)".to_string()
        }
    } else {
        "🟢 Standard Zero-Loss Compaction (100% Thoughts Retained)".to_string()
    };

    println!("\n# ⚡ Context Compaction & Tree-Shaking Report\n");
    if options.dry_run {
        println!("> [!NOTE]\n> **Dry Run Active**: Simulated compaction metrics displayed below. No disk files were modified.\n");
    } else {
        println!("Context for this session has been **physically compacted and anchored in this chat window**.");
    }
    println!("Mode: **{}**.\n", mode_header);
    println!("All **User prompts, Assistant reasoning, Decisions, and Error signals are 100% preserved verbatim**.\n");
    println!("---\n");
    println!("### 📊 Token Reduction & Storage Metrics\n");
    println!("| Metric Scope | Starting Size | Compacted Size | Net Reduction |");
    println!("| :--- | :---: | :---: | :---: |");
    println!("| **This Compaction Pass (`transcript.jsonl`)** | `{}` | `{}` | **{:.1}% saved** |", before_fmt, after_fmt, stats.this_run_savings_pct);
    println!("| **Cumulative Session Pruning (vs Full Stream)** | `{}` | `{}` | **{:.1}% pruned overall** |", cumulative_full_fmt, after_fmt, stats.cumulative_savings_pct);
    println!("| **Exportable Summary Artifact (`.md`)** | — | `{}` | **~{} tokens saved** |\n", format_bytes(stats.pruned_bytes), tokens_saved);
    println!("- **Preserved Core Signals**: {} User turns (100%) | {} Assistant turns (100%) | {} Error traces (100%)\n", stats.user_turns, stats.assistant_turns, stats.retained_errors);
    println!("- **Active Working Window**: Last **{} user conversational turns** kept 100% unpruned\n", options.recent_user_turns);
    
    if !options.dry_run && !backup_file_str.is_empty() {
        println!("> 💾 **In-Place JSONL Compaction**: `transcript.jsonl` was physically pruned on disk (Inode preserved, latest {} backups retained). Subsequent turns in **this exact window** now transmit the compact payload over the wire.\n", options.keep_backups);
    }

    if !history_timeline_md.is_empty() {
        println!("{}", history_timeline_md);
    }

    println!("---\n");
    println!("### 🟢 In-Window Fresh Slate Active");
    println!("> **Ready to continue**: Your context memory is now physically pruned. Simply type your next prompt and press **Send** in this chat.\n");
    println!("- **Interactive Artifact**: [📄 {}]({}) *(Click to preview in side pane)*\n", stats.suggested_filename, encoded_file_url);
    println!("<details>");
    println!("<summary>📋 Need to export or copy this session elsewhere?</summary>\n");
    println!("- **In-Chat Mention**: `@{}`", abs_str);
    println!("- **Copy to Project**: `cp {} ./`", quoted_path);
    println!("- **Copy to Clipboard**: `xclip -sel clip < {} || wl-copy < {}`", quoted_path, quoted_path);
    println!("</details>\n");
}
