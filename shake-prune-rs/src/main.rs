use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;

use shake_prune::hook::handle_hook;
use shake_prune::metadata::{write_active_anchor, write_artifact_metadata};

use shake_prune::pruner::{
    estimate_tokens, run_compaction_pipeline, shell_quote, CompactionOptions,
};
use shake_prune::{
    format_bytes, validate_output_path_allowlist, validate_transcript_path, VERSION,
};

fn handle_restore(target: &Path) {
    let abs_target = match target.canonicalize() {
        Ok(p) => p,
        Err(_) => target.to_path_buf(),
    };
    let bak_path = abs_target.with_extension("jsonl.bak");
    if !bak_path.exists() {
        eprintln!(
            "Error: Backup file does not exist at '{}'. Cannot restore.",
            bak_path.display()
        );
        process::exit(1);
    }
    match fs::copy(&bak_path, &abs_target) {
        Ok(bytes) => {
            println!(
                "✅ Successfully restored '{}' from atomic backup '{}' ({} bytes restored).",
                abs_target.display(),
                bak_path.display(),
                bytes
            );
        }
        Err(e) => {
            eprintln!("Error: Failed to restore backup: {}", e);
            process::exit(1);
        }
    }
}

fn handle_doctor() {
    println!("🩺 Antigravity /shake Diagnostic Doctor");
    println!("--------------------------------------------------");
    println!("Version: shake-prune {}", VERSION);
    if let Ok(exe_path) = env::current_exe() {
        println!("Binary Path: {}", exe_path.display());
    }

    let home = env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .unwrap_or_default();
    if home.is_empty() {
        println!("❌ HOME / USERPROFILE environment variable is NOT set.");
    } else {
        let gemini_dir = Path::new(&home).join(".gemini");
        if gemini_dir.exists() {
            println!("✅ Storage Root: {} (Accessible)", gemini_dir.display());
            let hooks_file = gemini_dir.join("config/hooks.json");
            if hooks_file.exists() {
                if let Ok(content) = fs::read_to_string(&hooks_file) {
                    if content.contains("shake-prune") {
                        println!("✅ Auto-Hook Registration: Active in hooks.json");
                    } else {
                        println!("⚠️ Auto-Hook Registration: hooks.json exists, but shake-prune hook is not registered.");
                    }
                }
            } else {
                println!(
                    "⚠️ Auto-Hook Registration: hooks.json not found at {}",
                    hooks_file.display()
                );
            }
        } else {
            println!(
                "⚠️ Storage Root: {} does not exist yet.",
                gemini_dir.display()
            );
        }
    }
    println!("--------------------------------------------------");
    println!("Diagnostic check completed.");
}

fn print_usage() {
    println!(
        r#"shake-prune {} - Deterministic In-Place Context Compactor for Antigravity

USAGE:
    shake-prune <transcript_path.jsonl> [output_path.md] [OPTIONS]
    shake-prune --hook
    shake-prune doctor
    shake-prune restore <transcript_path.jsonl>

ARGUMENTS:
    <transcript_path.jsonl>  Path to active transcript.jsonl
    [output_path.md]         Optional path for markdown summary artifact

OPTIONS:
    --recent-user-turns <N>  Number of recent user turns to retain unpruned (default: 10)
    --tools-cap <N>          Maximum recent tool outputs to retain unpruned (default: 20)
    --errors-cap <N>         Maximum recent tool calls to preserve raw errors (default: 30)
    --recent-window <N>      Fallback raw tool step window if user-turns=0 (default: 6)
    --full                   Full deep compaction (prunes thoughts older than window)
    --thought-window <N>     Number of recent turns to retain thoughts in full mode (default: 20)
    --dry-run                Simulate compaction without modifying transcript.jsonl
    --no-in-place            Generate markdown summary artifact without truncating JSONL
    --json                   Emit machine-readable JSON metrics on stdout
    --help, -h               Show this help message
    --version, -v            Show version
"#,
        VERSION
    );
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

    if args[1] == "doctor" || args[1] == "--doctor" {
        handle_doctor();
        process::exit(0);
    }

    if args[1] == "restore" {
        if args.len() < 3 {
            eprintln!("Usage: shake-prune restore <path/to/transcript.jsonl>");
            process::exit(1);
        }
        handle_restore(&PathBuf::from(&args[2]));
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
        } else if args[i] == "--tools-cap" && i + 1 < args.len() {
            if let Ok(val) = args[i + 1].parse::<usize>() {
                options.recent_tools_cap = val;
            }
            i += 2;
        } else if args[i] == "--errors-cap" && i + 1 < args.len() {
            if let Ok(val) = args[i + 1].parse::<usize>() {
                options.recent_errors_cap = val;
            }
            i += 2;
        } else if args[i] == "--thought-window" && i + 1 < args.len() {
            if let Ok(val) = args[i + 1].parse::<usize>() {
                options.thought_window_turns = Some(val);
            }
            i += 2;
        } else if args[i] == "--full" {
            options.marathon_horizon = true;
            if options.thought_window_turns.is_none() {
                options.thought_window_turns = Some(20);
            }
            i += 1;
        } else if args[i] == "--dry-run" {
            options.dry_run = true;
            i += 1;
        } else if args[i] == "--no-in-place" {
            options.in_place = false;
            i += 1;
        } else if args[i] == "--json" {
            json_output = true;
            i += 1;
        } else if !args[i].starts_with('-') && raw_target.is_empty() {
            raw_target = args[i].clone();
            i += 1;
        } else {
            i += 1;
        }
    }

    let (_compacted_jsonl, pruned_markdown, stats, master_archive_abs_str) =
        match run_compaction_pipeline(&transcript_path, &options) {
            Ok(res) => res,
            Err(e) => {
                eprintln!("Error during compaction: {}", e);
                process::exit(1);
            }
        };

    let initial_output_path = if !raw_target.is_empty() {
        let p = PathBuf::from(&raw_target);
        if p.is_dir()
            || raw_target.ends_with('/')
            || raw_target.ends_with('\\')
            || (!raw_target.ends_with(".md") && !raw_target.contains('.'))
        {
            p.join(&stats.suggested_filename)
        } else {
            p
        }
    } else if let Some(parent) = transcript_path.parent() {
        if parent.ends_with("logs") {
            if let Some(grandparent) = parent.parent().and_then(|p| p.parent()) {
                grandparent.join(&stats.suggested_filename)
            } else {
                parent.join(&stats.suggested_filename)
            }
        } else {
            parent.join(&stats.suggested_filename)
        }
    } else {
        PathBuf::from(&stats.suggested_filename)
    };

    let abs_output_path =
        match validate_output_path_allowlist(&initial_output_path, &transcript_path) {
            Ok(p) => p,
            Err(err_msg) => {
                eprintln!("{}", err_msg);
                process::exit(1);
            }
        };

    if !options.dry_run {
        if let Err(e) =
            File::create(&abs_output_path).and_then(|mut f| f.write_all(pruned_markdown.as_bytes()))
        {
            eprintln!(
                "Failed to write output file '{}': {}",
                abs_output_path.display(),
                e
            );
            process::exit(1);
        }

        let trigger_label = if options.thought_window_turns.is_some() {
            "Manual (/full-shake)"
        } else {
            "Manual (/shake)"
        };
        let _ = write_artifact_metadata(&abs_output_path, &stats.topic_slug);
        let _ = write_active_anchor(
            &abs_output_path,
            &stats,
            trigger_label,
            &master_archive_abs_str,
        );
    }

    if json_output {
        let json_val = serde_json::json!({
            "raw_bytes": stats.raw_bytes,
            "this_run_before_bytes": stats.this_run_before_bytes,
            "this_run_after_bytes": stats.this_run_after_bytes,
            "this_run_savings_pct": stats.this_run_savings_pct,
            "cumulative_full_bytes": stats.cumulative_full_bytes,
            "cumulative_savings_pct": stats.cumulative_savings_pct,
            "user_turns": stats.user_turns,
            "assistant_turns": stats.assistant_turns,
            "pruned_tools": stats.pruned_tools,
            "retained_errors": stats.retained_errors,
            "retained_short_cmds": stats.retained_short_cmds,
            "retained_recent_steps": stats.retained_recent_steps,
            "topic_slug": stats.topic_slug,
            "suggested_filename": stats.suggested_filename,
            "master_archive": master_archive_abs_str,
            "output_path": abs_output_path.display().to_string(),
        });
        println!("{}", json_val);
        return;
    }

    let abs_str = abs_output_path.to_string_lossy().to_string();
    let quoted_path = shell_quote(&abs_str);

    let est_prompt_tokens_before = estimate_tokens(stats.this_run_before_bytes);
    let est_prompt_tokens_after = estimate_tokens(stats.this_run_after_bytes);

    println!("\n# ⚡ Context Compaction Completed");
    println!("> - **Session Topic**: `{}`", stats.topic_slug);
    println!("> - **Working Window**: Preserved last {} user turns verbatim (capped at {} tool outputs, {} error retention).", options.recent_user_turns, options.recent_tools_cap, options.recent_errors_cap);
    println!("> - **Master Archive**: `{}`", master_archive_abs_str);
    println!("> - **Executive Summary**: `{}`\n", abs_str);

    println!("| Metric | Pre-Shake | Shaken (Active Memory) | Savings |");
    println!("| :--- | :--- | :--- | :--- |");
    println!(
        "| **Live Prompt Payload** | `{}` | **`{}`** | **`{:.1}%`** |",
        format_bytes(stats.this_run_before_bytes),
        format_bytes(stats.this_run_after_bytes),
        stats.this_run_savings_pct
    );
    println!(
        "| **Estimated Prompt Tokens** | `~{}` | **`~{}`** | **`{:.1}%`** |",
        est_prompt_tokens_before, est_prompt_tokens_after, stats.this_run_savings_pct
    );
    println!(
        "| **Tool Outputs Compacted** | `{}` | `0` (compacted to receipts) | `-` |",
        stats.pruned_tools
    );
    println!(
        "| **Retained Errors (Un-Clamped)** | - | `{}` (full traces kept verbatim) | `-` |",
        stats.retained_errors
    );
    println!(
        "| **Active Working Tools** | - | `{}` (unpruned output kept verbatim) | `-` |\n",
        stats.retained_recent_steps
    );

    if options.dry_run {
        println!(
            "> ⚠️ **[Dry Run Active]**: No changes were written to `{}`.",
            transcript_path.display()
        );
    } else {
        println!("> 🔒 **Inode Preserved**: File rewritten in place. Open file descriptors remain valid.\n");
    }

    println!("<details>");
    println!("<summary>📋 Need to export or copy this session elsewhere?</summary>\n");
    println!("- **In-Chat Mention**: `@{}`", abs_str);
    println!("- **Copy to Project**: `cp {} ./`", quoted_path);
    if cfg!(target_os = "windows") {
        println!(
            "- **Copy to Clipboard**: `powershell -c \"Get-Content {} | Set-Clipboard\"`",
            quoted_path
        );
    } else if cfg!(target_os = "macos") {
        println!("- **Copy to Clipboard**: `pbcopy < {}`", quoted_path);
    } else {
        println!(
            "- **Copy to Clipboard**: `xclip -sel clip < {} || wl-copy < {}`",
            quoted_path, quoted_path
        );
    }
    println!("</details>\n");
}
