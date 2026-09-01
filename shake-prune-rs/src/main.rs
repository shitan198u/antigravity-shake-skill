mod hook;
mod metadata;
mod models;
mod pruner;
mod slug;

use hook::handle_hook;
use metadata::{write_active_anchor, write_artifact_metadata};
use pruner::prune_transcript;
use std::env;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;

fn print_usage() {
    println!("Usage: shake-prune <transcript.jsonl> [output_file_or_dir] [--recent-window N]");
    println!("       shake-prune --hook");
    println!("\nOptions:");
    println!("  -h, --help           Show this help message and exit");
    println!("  --hook               Run as Antigravity PreInvocation hook (reads stdin JSON)");
    println!("  --recent-window N    Number of recent tool execution steps to keep intact (default: 6)");
    println!("\nExamples:");
    println!("  shake-prune /path/to/transcript.jsonl");
    println!("  shake-prune /path/to/transcript.jsonl /path/to/output_dir/");
    println!("  shake-prune /path/to/transcript.jsonl /path/to/custom_name.md --recent-window 8");
    println!("  shake-prune --hook");
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

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 || args[1] == "--help" || args[1] == "-h" || args[1] == "help" {
        print_usage();
        process::exit(0);
    }

    if args[1] == "--hook" {
        handle_hook();
        process::exit(0);
    }

    let transcript_path = PathBuf::from(&args[1]);
    let mut raw_target = String::new();
    let mut recent_window = 6usize;

    let mut i = 2;
    while i < args.len() {
        if args[i] == "--recent-window" && i + 1 < args.len() {
            if let Ok(val) = args[i + 1].parse::<usize>() {
                recent_window = val;
            }
            i += 2;
        } else if raw_target.is_empty() && !args[i].starts_with("--") {
            raw_target = args[i].clone();
            i += 1;
        } else {
            i += 1;
        }
    }

    let (pruned_markdown, stats) = match prune_transcript(&transcript_path, recent_window) {
        Ok(res) => res,
        Err(e) => {
            eprintln!("Error pruning transcript: {}", e);
            process::exit(1);
        }
    };

    let output_path: PathBuf = if !raw_target.is_empty() && !Path::new(&raw_target).is_dir() && raw_target.ends_with(".md") {
        PathBuf::from(&raw_target)
    } else if !raw_target.is_empty() && Path::new(&raw_target).is_dir() {
        Path::new(&raw_target).join(&stats.suggested_filename)
    } else {
        PathBuf::from(&stats.suggested_filename)
    };

    let abs_output_path = match std::fs::canonicalize(&output_path) {
        Ok(p) => p,
        Err(_) => {
            if let Ok(curr) = env::current_dir() {
                curr.join(&output_path)
            } else {
                output_path.clone()
            }
        }
    };

    if let Err(e) = File::create(&abs_output_path).and_then(|mut f| f.write_all(pruned_markdown.as_bytes())) {
        eprintln!("Failed to write output file '{}': {}", abs_output_path.display(), e);
        process::exit(1);
    }

    let summary_text = format!(
        "Shaken & pruned verbatim history for topic '{}'. Saved {:.1}% context tokens ({} tokens vs {} raw). Preserved {} user prompts, all reasoning, and thoughts.",
        stats.topic_slug.replace('_', " "),
        stats.reduction_pct,
        stats.pruned_tokens,
        stats.raw_tokens,
        stats.user_turns
    );

    let _ = write_artifact_metadata(&abs_output_path, &summary_text);
    let _ = write_active_anchor(&abs_output_path, &stats);

    let abs_str = abs_output_path.display().to_string();
    let raw_formatted = format_bytes(stats.raw_bytes);
    let pruned_formatted = format_bytes(stats.pruned_bytes);
    let tokens_saved = stats.raw_tokens.saturating_sub(stats.pruned_tokens);

    println!("\n# ⚡ Context Compaction & Tree-Shaking Report\n");
    println!("Context for this session has been compacted and anchored in this chat window.");
    println!("All **User prompts, Assistant reasoning, Thoughts, and Error signals are 100% preserved verbatim**.\n");
    println!("---\n");
    println!("### 📊 Token Reduction Metrics\n");
    println!("| Metric | Original | Pruned | Savings |");
    println!("| :--- | :--- | :--- | :--- |");
    println!("| **Payload Size** | `{}` | `{}` | **{:.1}% reduction** |", raw_formatted, pruned_formatted, stats.reduction_pct);
    println!("| **Estimated Tokens** | `~{}` | `~{}` | **~{} tokens saved** |", stats.raw_tokens, stats.pruned_tokens, tokens_saved);
    println!("| **Preserved Signals** | {} User turns (100%) | {} Assistant turns (100%) | {} Error traces (100%) |\n", stats.user_turns, stats.assistant_turns, stats.retained_errors);
    println!("---\n");
    println!("### 🟢 In-Window Continuity Active");
    println!("> **Ready to continue**: Your context memory is now pinned to the clean state. Simply type your next prompt and press **Send** in this chat.\n");
    println!("- **Interactive Artifact**: [📄 {}](file://{}) *(Click to preview in side pane)*\n", stats.suggested_filename, abs_str);
    println!("<details>");
    println!("<summary>📋 Need to export or copy this session elsewhere?</summary>\n");
    println!("- **In-Chat Mention**: `@{}`", abs_str);
    println!("- **Copy to Project**: `cp \"{}\" ./`", abs_str);
    println!("- **Copy to Clipboard**: `xclip -sel clip < \"{}\" || wl-copy < \"{}\"`", abs_str, abs_str);
    println!("</details>\n");
}
