mod metadata;
mod models;
mod pruner;
mod slug;

use metadata::write_artifact_metadata;
use pruner::prune_transcript;
use std::env;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;

fn print_usage() {
    eprintln!("Usage: shake-prune <transcript.jsonl> [output_file_or_dir] [--recent-window N]");
    eprintln!("\nExamples:");
    eprintln!("  shake-prune /path/to/transcript.jsonl");
    eprintln!("  shake-prune /path/to/transcript.jsonl /path/to/output_dir/");
    eprintln!("  shake-prune /path/to/transcript.jsonl /path/to/custom_name.md --recent-window 8");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        process::exit(1);
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
        "Shaken & pruned verbatim history for topic '{}'. Saved {:.1}% context tokens ({} tokens vs {} raw). Preserved {} user prompts and all reasoning.",
        stats.topic_slug.replace('_', " "),
        stats.reduction_pct,
        stats.pruned_tokens,
        stats.raw_tokens,
        stats.user_turns
    );

    let _ = write_artifact_metadata(&abs_output_path, &summary_text);

    let abs_str = abs_output_path.display().to_string();

    println!("\n================================================================================");
    println!("               ⚡ SHAKE CONTEXT PRUNING REPORT (RUST NATIVE) ⚡");
    println!("================================================================================");
    println!("• Session ID:       {}", stats.conv_id);
    println!("• Topic:            {}", stats.topic_slug.replace('_', " ").to_uppercase());
    println!("• Original Payload: {} bytes (~{} tokens)", stats.raw_bytes, stats.raw_tokens);
    println!("• Pruned Payload:   {} bytes (~{} tokens)", stats.pruned_bytes, stats.pruned_tokens);
    println!("• Token Savings:    {:.1}% reduction", stats.reduction_pct);
    println!(
        "• Preserved Signals: {} user turns (100%), {} assistant turns (100%), {} errors",
        stats.user_turns, stats.assistant_turns, stats.retained_errors
    );
    println!("--------------------------------------------------------------------------------");
    println!("📋 RESUMPTION PATHS & QUICK-COPY");
    println!("--------------------------------------------------------------------------------");
    println!("• Absolute File Path: {}", abs_str);
    println!("• In-Chat Mention:    @{}", abs_str);
    println!("• Copy to Project:    cp \"{}\" ./", abs_str);
    println!("• Copy to Clipboard:  xclip -sel clip < \"{}\" || wl-copy < \"{}\"", abs_str, abs_str);
    println!("================================================================================\n");
}
