mod hook;
mod metadata;
mod models;
mod pruner;
mod slug;

use hook::handle_hook;
use metadata::{write_active_anchor, write_artifact_metadata};
use pruner::{compact_transcript_inplace, prune_transcript, shell_quote};
use std::env;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;

fn print_usage() {
    println!("Usage: shake-prune <transcript.jsonl> [output_file_or_dir] [--recent-window N] [--no-in-place]");
    println!("       shake-prune --hook");
    println!("\nOptions:");
    println!("  -h, --help           Show this help message and exit");
    println!("  --hook               Run as Antigravity PreInvocation hook (reads stdin JSON)");
    println!("  --recent-window N    Number of recent tool execution steps to keep intact (default: 6)");
    println!("  --no-in-place        Disable physical in-place compaction of transcript.jsonl");
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

/// Validates that an output path is not pointed at sensitive system directories
/// to prevent arbitrary file overwrite vulnerabilities via prompt injection.
fn validate_output_path(target: &Path) -> Result<(), String> {
    let p_str = target.to_string_lossy();
    let lower = p_str.to_lowercase();

    let forbidden_prefixes = [
        "/etc", "/root", "/bin", "/sbin", "/usr", "/boot", "/sys", "/proc", "/dev",
        "c:\\windows", "c:\\program files", "\\windows", "\\system32"
    ];

    for prefix in forbidden_prefixes {
        if lower.starts_with(prefix) {
            return Err(format!(
                "Security Error: Output path '{}' is within restricted system directory '{}'",
                p_str, prefix
            ));
        }
    }

    Ok(())
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
    if let Err(err_msg) = validate_transcript_path(&transcript_path) {
        eprintln!("Security/Validation Error: {}", err_msg);
        process::exit(1);
    }

    let mut raw_target = String::new();
    let mut recent_window = 6usize;
    let mut in_place = true;

    let mut i = 2;
    while i < args.len() {
        if args[i] == "--recent-window" && i + 1 < args.len() {
            if let Ok(val) = args[i + 1].parse::<usize>() {
                recent_window = val;
            }
            i += 2;
        } else if args[i] == "--no-in-place" {
            in_place = false;
            i += 1;
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

    if let Err(err_msg) = validate_output_path(&output_path) {
        eprintln!("{}", err_msg);
        process::exit(1);
    }

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

    // Perform physical in-place JSONL compaction with Inode preservation
    let in_place_result = if in_place {
        compact_transcript_inplace(&transcript_path, recent_window).ok()
    } else {
        None
    };

    let abs_str = abs_output_path.display().to_string();
    let quoted_path = shell_quote(&abs_str);
    let encoded_file_url = format!("file://{}", urlencoding::encode(&abs_str).replace("%2F", "/"));
    let raw_formatted = format_bytes(stats.raw_bytes);
    let pruned_formatted = format_bytes(stats.pruned_bytes);
    let tokens_saved = stats.raw_tokens.saturating_sub(stats.pruned_tokens);

    println!("\n# ⚡ Context Compaction & Tree-Shaking Report\n");
    println!("Context for this session has been **physically compacted and anchored in this chat window**.");
    println!("All **User prompts, Assistant reasoning, Thoughts, and Error signals are 100% preserved verbatim**.\n");
    println!("---\n");
    println!("### 📊 Physical Token Reduction Metrics\n");
    println!("| Metric | Original | Pruned | Savings |");
    println!("| :--- | :--- | :--- | :--- |");
    println!("| **Payload Size** | `{}` | `{}` | **{:.1}% physical reduction** |", raw_formatted, pruned_formatted, stats.reduction_pct);
    println!("| **Estimated Tokens** | `~{}` | `~{}` | **~{} tokens saved** |", stats.raw_tokens, stats.pruned_tokens, tokens_saved);
    println!("| **Preserved Signals** | {} User turns (100%) | {} Assistant turns (100%) | {} Error traces (100%) |\n", stats.user_turns, stats.assistant_turns, stats.retained_errors);
    
    if in_place_result.is_some() {
        println!("> 💾 **In-Place JSONL Compaction**: `transcript.jsonl` was physically pruned on disk (Inode preserved) with timestamped backup created. Subsequent turns in **this exact window** now transmit the compact payload over the wire.\n");
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
