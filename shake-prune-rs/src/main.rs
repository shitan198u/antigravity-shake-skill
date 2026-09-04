use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;

use shake_prune::analysis::{analyze_transcript, AnalysisOptions};
use shake_prune::config::ShakeConfig;
use shake_prune::continuity::ContinuityCard;
use shake_prune::hook::handle_hook;
use shake_prune::metadata::{
    is_circuit_open, write_active_anchor, write_artifact_metadata, AnchorFilePayload,
};
use shake_prune::mode::{resolve_mode, CompactionMode, ResolvedMode};
use shake_prune::pruner::{estimate_tokens, run_compaction_pipeline, CompactionOptions};
use shake_prune::{
    format_bytes, validate_output_path_allowlist, validate_transcript_path, VERSION,
};

fn handle_restore(target: &Path, force: bool) {
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

    // 1. Validate backup is non-empty, readable, and contains valid JSON lines before touching transcript (P2-9, 5.2)
    let bak_len = match fs::metadata(&bak_path) {
        Ok(m) => m.len(),
        Err(e) => {
            eprintln!("Error stating backup file '{}': {}", bak_path.display(), e);
            process::exit(1);
        }
    };
    if bak_len == 0 {
        eprintln!(
            "Error: Backup file at '{}' is empty (0 bytes). Refusing to restore empty backup.",
            bak_path.display()
        );
        process::exit(1);
    }

    let bak_file = match File::open(&bak_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "Error: Cannot read backup file '{}': {}",
                bak_path.display(),
                e
            );
            process::exit(1);
        }
    };
    let reader = std::io::BufReader::new(bak_file);
    use std::io::BufRead;
    let mut line_count = 0;
    for (idx, line_res) in reader.lines().enumerate() {
        match line_res {
            Ok(line_str) => {
                if !line_str.trim().is_empty() {
                    if !force && serde_json::from_str::<serde_json::Value>(&line_str).is_err() {
                        eprintln!(
                            "Error: Backup file at '{}' contains invalid JSON on line {}. Refusing to restore corrupt backup (use --force to override).",
                            bak_path.display(),
                            idx + 1
                        );
                        process::exit(1);
                    }
                    line_count += 1;
                }
            }
            Err(e) => {
                eprintln!("Error reading backup file '{}': {}", bak_path.display(), e);
                process::exit(1);
            }
        }
    }
    if line_count == 0 {
        eprintln!(
            "Error: Backup file at '{}' contains no content lines. Refusing to restore.",
            bak_path.display()
        );
        process::exit(1);
    }

    // 2. Lock target transcript exclusively (P2-9)
    let mut target_file = match File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&abs_target)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Error opening transcript '{}': {}", abs_target.display(), e);
            process::exit(1);
        }
    };
    use fs2::FileExt;
    if let Err(e) = target_file.lock_exclusive() {
        eprintln!("Error locking transcript '{}': {}", abs_target.display(), e);
        process::exit(1);
    }

    // 3. Create .pre_restore snapshot of current transcript if it has data (P2-9)
    if let Ok(meta) = target_file.metadata() {
        if meta.len() > 0 {
            use std::io::{Seek, SeekFrom};
            let pre_restore_path = abs_target.with_extension("jsonl.pre_restore");
            if let Ok(mut pre_file) = File::create(&pre_restore_path) {
                let _ = target_file.seek(SeekFrom::Start(0));
                let _ = std::io::copy(&mut target_file, &mut pre_file);
                let _ = pre_file.flush();
                let _ = target_file.seek(SeekFrom::Start(0));
                shake_prune::atomic::set_user_only_permissions(&pre_restore_path);
            }
        }
    }

    // 4. Restore from backup
    match shake_prune::atomic::restore_from_backup(&mut target_file, &bak_path) {
        Ok(bytes) => {
            shake_prune::atomic::set_user_only_permissions(&abs_target);
            shake_prune::atomic::remove_intent_marker(&abs_target);
            let _ = fs2::FileExt::unlock(&target_file);
            println!(
                "✅ Successfully restored '{}' from atomic backup '{}' ({} bytes, {} lines restored).",
                abs_target.display(),
                bak_path.display(),
                bytes,
                line_count
            );
        }
        Err(e) => {
            let _ = fs2::FileExt::unlock(&target_file);
            eprintln!("Error: Failed to restore backup: {}", e);
            process::exit(1);
        }
    }
}

fn handle_doctor(json_output: bool) {
    let home = env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .unwrap_or_default();
    let exe_path = env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    let config_path = ShakeConfig::global_config_path();
    let config_exists = config_path.as_ref().map(|p| p.exists()).unwrap_or(false);
    let loaded_config = ShakeConfig::load();
    let (config_valid, config_err) = if let Some(cp) = &config_path {
        if cp.exists() {
            match ShakeConfig::load_from_file(cp) {
                Ok(_) => (true, None),
                Err(e) => (false, Some(e)),
            }
        } else {
            (true, None)
        }
    } else {
        (true, None)
    };

    let (gemini_exists, hook_active, hooks_path, logs_writable) = if home.is_empty() {
        (false, false, String::new(), false)
    } else {
        let gemini_dir = Path::new(&home).join(".gemini");
        let hooks_file = gemini_dir.join("config/hooks.json");
        let logs_dir = gemini_dir.join("logs");
        let mut active = false;
        if hooks_file.exists() {
            if let Ok(content) = fs::read_to_string(&hooks_file) {
                active = content.contains("shake-prune");
            }
        }
        let writable = fs::create_dir_all(&logs_dir).is_ok();
        (
            gemini_dir.exists(),
            active,
            hooks_file.display().to_string(),
            writable,
        )
    };

    let stale_markers = if gemini_exists {
        let brain_dir = Path::new(&home).join(".gemini/antigravity-ide/brain");
        let mut markers = Vec::new();
        if brain_dir.exists() {
            if let Ok(entries) = fs::read_dir(&brain_dir) {
                for entry in entries.flatten() {
                    let marker = entry
                        .path()
                        .join(".system_generated/logs/.shake_in_progress");
                    if marker.exists() {
                        markers.push(marker);
                    }
                }
            }
        }
        markers
    } else {
        Vec::new()
    };

    if json_output {
        let val = serde_json::json!({
            "version": VERSION,
            "binary_path": exe_path,
            "home_set": !home.is_empty(),
            "storage_root_exists": gemini_exists,
            "hook_registered": hook_active,
            "hooks_config": hooks_path,
            "config_path": config_path.map(|p| p.display().to_string()),
            "config_found": config_exists,
            "config_valid": config_valid,
            "config_error": config_err,
            "auto_shake_enabled": loaded_config.auto.enabled,
            "logs_writable": logs_writable,
            "stale_intent_markers": stale_markers.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        });
        println!("{}", val);
        return;
    }

    println!("🩺 Antigravity /shake Diagnostic Doctor");
    println!("--------------------------------------------------");
    println!("Version: shake-prune {}", VERSION);
    println!("Binary Path: {}", exe_path);

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

            if let Some(cp) = &config_path {
                if cp.exists() {
                    if let Some(err) = &config_err {
                        println!("❌ Config File: {} (SYNTAX ERROR: {})", cp.display(), err);
                    } else {
                        println!(
                            "✅ Config File: {} (auto.enabled = {})",
                            cp.display(),
                            loaded_config.auto.enabled
                        );
                    }
                } else {
                    println!(
                        "ℹ️ Config File: Not found at {} (using defaults; auto.enabled = {})",
                        cp.display(),
                        loaded_config.auto.enabled
                    );
                }
            }

            if !stale_markers.is_empty() {
                println!(
                    "⚠️ Stale Intent Markers: Found {} unrecovered marker(s) from interrupted compactions.",
                    stale_markers.len()
                );
            }

            if logs_writable {
                println!("✅ Diagnostic Logs: ~/.gemini/logs directory is writable");
            } else {
                println!("⚠️ Diagnostic Logs: ~/.gemini/logs directory is NOT writable");
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

fn handle_preview(transcript_path: &Path, requested_mode: CompactionMode, json_output: bool) {
    if let Err(err_msg) = validate_transcript_path(transcript_path) {
        eprintln!("Security/Validation Error: {}", err_msg);
        process::exit(1);
    }

    let analysis = match analyze_transcript(transcript_path, &AnalysisOptions::default()) {
        Ok(a) => a,
        Err(e) => {
            eprintln!(
                "Failed to analyze transcript '{}': {}",
                transcript_path.display(),
                e
            );
            process::exit(1);
        }
    };

    let config = ShakeConfig::load();
    let resolved_mode = resolve_mode(requested_mode, &analysis, config.deep_after_user_turns);

    let mut options = CompactionOptions {
        recent_user_turns: config.retention.recent_user_turns,
        recent_tools_cap: config.retention.recent_tools_cap,
        recent_errors_cap: config.retention.recent_errors_cap,
        recent_window_steps: config.retention.recent_window_steps,
        thought_window_turns: None,
        marathon_horizon: false,
        in_place: false,
        dry_run: true,
        redact_secrets: config.privacy.redact_secrets,
        non_blocking_lock: false,
    };

    if resolved_mode == ResolvedMode::Deep {
        options.marathon_horizon = true;
        options.thought_window_turns = Some(20);
    }

    let (_compacted_jsonl, _pruned_md, stats, master_archive_abs_str) =
        match run_compaction_pipeline(transcript_path, &options) {
            Ok(res) => res,
            Err(e) => {
                eprintln!("Preview error during compaction dry run: {}", e);
                process::exit(1);
            }
        };

    let continuity_card = ContinuityCard::build(
        &analysis,
        &master_archive_abs_str,
        transcript_path,
        options.redact_secrets,
    );

    if json_output {
        let json_val = serde_json::json!({
            "transcript_path": transcript_path.display().to_string(),
            "mode_requested": requested_mode.to_string(),
            "mode_resolved": resolved_mode.to_string(),
            "before_bytes": stats.this_run_before_bytes,
            "before_tokens": stats.raw_tokens,
            "estimated_after_bytes": stats.this_run_after_bytes,
            "estimated_after_tokens": stats.pruned_tokens,
            "estimated_savings_pct": stats.this_run_savings_pct,
            "user_turns": stats.user_turns,
            "assistant_turns": stats.assistant_turns,
            "pruned_tools": stats.pruned_tools,
            "newly_pruned_tools": stats.newly_pruned_tools,
            "already_pruned_tools": stats.already_pruned_tools,
            "retained_errors": stats.retained_errors,
            "continuity": continuity_card,
        });
        println!("{}", json_val);
        return;
    }

    let format_prompt_tokens = |tokens: usize| -> String {
        if tokens >= 1_000_000 {
            format!("~{:.1}M", tokens as f64 / 1_000_000.0)
        } else if tokens >= 1_000 {
            format!("~{}k", (tokens + 500) / 1000)
        } else {
            format!("~{}", tokens)
        }
    };

    let display_topic: String = stats
        .topic_slug
        .split('_')
        .filter(|s| !s.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    println!("\n# 🔍 Context Compaction Preview");
    println!(
        "> **Session**: `{}` • **Mode**: `{}` (Adaptive) • **Target**: `{}`\n",
        display_topic,
        resolved_mode,
        transcript_path.display()
    );

    let savings_label = if stats.this_run_savings_pct <= 0.0 {
        "0.0% (Already compact)".to_string()
    } else {
        format!("{:.1}% estimated", stats.this_run_savings_pct)
    };
    let pruned_label = if stats.newly_pruned_tools == 0 && stats.already_pruned_tools > 0 {
        format!("Already compact ({} archived)", stats.already_pruned_tools)
    } else if stats.already_pruned_tools > 0 {
        format!(
            "{} to prune ({} total archived)",
            stats.newly_pruned_tools, stats.pruned_tools
        )
    } else {
        format!("{} to prune", stats.newly_pruned_tools)
    };

    println!("| Metric | Current | Estimated After | Savings |");
    println!("| :--- | :--- | :--- | :--- |");
    println!(
        "| **Context Payload** | `{}` ({}) | **`{}`** ({}) | **`{}`** |",
        format_bytes(stats.this_run_before_bytes),
        format_prompt_tokens(stats.raw_tokens),
        format_bytes(stats.this_run_after_bytes),
        format_prompt_tokens(stats.pruned_tokens),
        savings_label
    );
    println!(
        "| **Pruned Tool Bloat** | {} tool executions | **Clean receipts** (`archive=...`) | **{}** |",
        stats.pruned_tools,
        pruned_label
    );
    println!(
        "| **Working Memory** | Last {} user turns | **100% dialogue preserved** | Active |\n",
        options.recent_user_turns
    );

    println!("### 📌 Continuity Anchor Preview");
    if let Some(task) = &continuity_card.task {
        println!("- **Task**: {}", task);
    }
    if !continuity_card.recent_files.is_empty() {
        println!(
            "- **Recent Files**: {}",
            continuity_card.recent_files.join(", ")
        );
    }
    if !continuity_card.recent_failures.is_empty() {
        let fails: Vec<String> = continuity_card
            .recent_failures
            .iter()
            .map(|f| {
                if let Some(code) = f.exit {
                    format!("{} (step {} exit {})", f.tool, f.step, code)
                } else {
                    format!("{} (step {})", f.tool, f.step)
                }
            })
            .collect();
        println!("- **Recent Failures**: {}", fails.join("; "));
    }
    println!("- **Undo Command**: `{}`\n", continuity_card.undo_command);
    println!(
        "*Run `shake-prune run {}` to execute compaction.*",
        transcript_path.display()
    );
}

fn handle_status(transcript_path: &Path, json_output: bool) {
    if let Err(err_msg) = validate_transcript_path(transcript_path) {
        eprintln!("Security/Validation Error: {}", err_msg);
        process::exit(1);
    }

    let analysis = match analyze_transcript(transcript_path, &AnalysisOptions::default()) {
        Ok(a) => a,
        Err(e) => {
            eprintln!(
                "Failed to analyze transcript '{}': {}",
                transcript_path.display(),
                e
            );
            process::exit(1);
        }
    };

    let config = ShakeConfig::load();
    let logs_dir = transcript_path.parent().unwrap_or_else(|| Path::new("."));
    let master_archive = logs_dir.join("transcript_full.jsonl");
    let backup_file = transcript_path.with_extension("jsonl.bak");

    let anchor_path = analysis.artifact_dir.join("active_shake_anchor.json");
    let anchor_data: Option<AnchorFilePayload> = if anchor_path.exists() {
        File::open(&anchor_path)
            .ok()
            .and_then(|f| serde_json::from_reader(f).ok())
    } else {
        None
    };

    let is_size_exceeded = analysis.bytes >= config.auto.size_threshold_bytes;
    let is_burst_exceeded = analysis.unpruned_tool_count >= config.auto.tool_burst_threshold;

    let (recommended, reason) = if is_size_exceeded {
        (
            true,
            format!(
                "Context payload exceeded {} threshold (~80k tokens)",
                format_bytes(config.auto.size_threshold_bytes as usize)
            ),
        )
    } else if is_burst_exceeded {
        (
            true,
            format!(
                "Unpruned tool burst ({} tools >= threshold {})",
                analysis.unpruned_tool_count, config.auto.tool_burst_threshold
            ),
        )
    } else {
        (
            false,
            "Context payload and tool counts are healthy".to_string(),
        )
    };

    let resolved_mode = resolve_mode(
        CompactionMode::Auto,
        &analysis,
        config.deep_after_user_turns,
    );

    let master_archive_bytes = fs::metadata(&master_archive).map(|m| m.len()).unwrap_or(0);
    let backup_bytes = fs::metadata(&backup_file).map(|m| m.len()).unwrap_or(0);

    if json_output {
        let json_val = serde_json::json!({
            "transcript_path": transcript_path.display().to_string(),
            "bytes": analysis.bytes,
            "estimated_tokens": analysis.estimated_tokens,
            "user_turns": analysis.total_user_turns,
            "assistant_turns": analysis.total_assistant_turns,
            "tool_steps": analysis.total_tool_steps,
            "unpruned_tools": analysis.unpruned_tool_count,
            "failed_tools": analysis.failed_tool_count,
            "recommendation": {
                "compact_recommended": recommended,
                "reason": reason,
                "suggested_mode": resolved_mode.to_string(),
            },
            "master_archive": {
                "path": master_archive.display().to_string(),
                "exists": master_archive.exists(),
                "bytes": master_archive_bytes,
            },
            "backup": {
                "path": backup_file.display().to_string(),
                "exists": backup_file.exists(),
                "bytes": backup_bytes,
            },
            "anchor": {
                "exists": anchor_path.exists(),
                "topic": anchor_data.as_ref().and_then(|a| a.topic.clone()),
                "last_compacted_bytes": anchor_data.as_ref().and_then(|a| a.last_compacted_bytes),
                "token_savings_pct": anchor_data.as_ref().and_then(|a| a.token_savings_pct),
                "timestamp": anchor_data.as_ref().and_then(|a| a.timestamp.clone()),
                "consecutive_failures": anchor_data.as_ref().and_then(|a| a.consecutive_failures).unwrap_or(0),
                "circuit_open": anchor_data.as_ref().map(|a| is_circuit_open(a, chrono::Utc::now().timestamp())).unwrap_or(false),
            }
        });
        println!("{}", json_val);
        return;
    }

    println!("\n# 📊 Transcript Status");
    println!("> **Target**: `{}`\n", transcript_path.display());

    println!("| Metric | Value | Status |");
    println!("| :--- | :--- | :--- |");
    let size_status = if is_size_exceeded {
        "⚠️ Exceeds 80k threshold"
    } else {
        "✅ Healthy"
    };
    println!(
        "| **Context Size** | `{}` (~{}k tokens) | {} |",
        format_bytes(analysis.bytes as usize),
        (analysis.estimated_tokens + 500) / 1000,
        size_status
    );
    println!(
        "| **Conversational Turns** | {} user / {} assistant | Active |",
        analysis.total_user_turns, analysis.total_assistant_turns
    );
    let tools_status = if is_burst_exceeded {
        "⚠️ Exceeds burst threshold"
    } else {
        "✅ Within bounds"
    };
    println!(
        "| **Unpruned Tools** | {} executions | {} |",
        analysis.unpruned_tool_count, tools_status
    );
    let rec_str = if recommended {
        "**Compaction Recommended**"
    } else {
        "Healthy (Not Urgent)"
    };
    println!("| **Recommendation** | {} | {} |", rec_str, reason);
    println!(
        "| **Suggested Mode** | `{}` | Adaptive recommendation |\n",
        resolved_mode
    );

    println!("### Subsystem Health");
    if master_archive.exists() {
        println!(
            "- **Master Archive (`transcript_full.jsonl`)**: Present ({})",
            format_bytes(master_archive_bytes as usize)
        );
    } else {
        println!("- **Master Archive (`transcript_full.jsonl`)**: Not initialized yet");
    }

    if backup_file.exists() {
        println!(
            "- **Atomic Backup (`transcript.jsonl.bak`)**: Present ({})",
            format_bytes(backup_bytes as usize)
        );
    } else {
        println!("- **Atomic Backup (`transcript.jsonl.bak`)**: None");
    }

    if let Some(anchor) = anchor_data {
        let topic_name = anchor.topic.as_deref().unwrap_or("Unknown");
        let savings = anchor.token_savings_pct.unwrap_or(0.0);
        let step_str = anchor
            .anchored_at_step
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "?".to_string());
        println!(
            "- **Last Anchor**: `{}` at Step {} (Saved {:.1}%)",
            topic_name, step_str, savings
        );
        let failures = anchor.consecutive_failures.unwrap_or(0);
        let circuit_status = if is_circuit_open(&anchor, chrono::Utc::now().timestamp()) {
            "🔴 TRIPPED (backing off)"
        } else {
            "🟢 Normal"
        };
        println!(
            "- **Auto-Hook Circuit Breaker**: {} ({} consecutive failures)",
            circuit_status, failures
        );
    } else {
        println!("- **Last Anchor**: No active anchor file found");
    }
    println!();
}

fn handle_show(
    transcript_path: &Path,
    step_opt: Option<u64>,
    line_opt: Option<usize>,
    pretty: bool,
    json_output: bool,
    full: bool,
) {
    let logs_dir = transcript_path.parent().unwrap_or_else(|| Path::new("."));
    let full_archive = logs_dir.join("transcript_full.jsonl");

    let target_file = if full_archive.exists() {
        full_archive
    } else if transcript_path.exists() {
        transcript_path.to_path_buf()
    } else {
        eprintln!(
            "Error: Master archive not found at '{}' and transcript not found at '{}'.",
            full_archive.display(),
            transcript_path.display()
        );
        process::exit(1);
    };

    let file = match File::open(&target_file) {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "Error opening archive file '{}': {}",
                target_file.display(),
                e
            );
            process::exit(1);
        }
    };
    let reader = std::io::BufReader::new(file);
    use std::io::BufRead;

    let mut matched_line: Option<(usize, String)> = None;

    if let Some(target_line) = line_opt {
        for (idx, line_res) in reader.lines().enumerate() {
            let line_no = idx + 1;
            if line_no == target_line {
                if let Ok(l) = line_res {
                    matched_line = Some((line_no, l));
                }
                break;
            }
        }
    } else if let Some(target_step) = step_opt {
        for (idx, line_res) in reader.lines().enumerate() {
            let line_no = idx + 1;
            if let Ok(l) = line_res {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&l) {
                    if val.get("step_index").and_then(|v| v.as_u64()) == Some(target_step) {
                        matched_line = Some((line_no, l));
                        break;
                    }
                }
            }
        }
    } else {
        eprintln!("Error: You must specify --step <N> or --line <N> to show.");
        process::exit(1);
    }

    let (line_no, line_content) = match matched_line {
        Some(m) => m,
        None => {
            if let Some(l) = line_opt {
                eprintln!(
                    "Error: Line {} not found in '{}'.",
                    l,
                    target_file.display()
                );
            } else if let Some(s) = step_opt {
                eprintln!(
                    "Error: Step {} not found in '{}'.",
                    s,
                    target_file.display()
                );
            }
            process::exit(1);
        }
    };

    if json_output {
        println!("{}", line_content);
        return;
    }

    let val: serde_json::Value = match serde_json::from_str(&line_content) {
        Ok(v) => v,
        Err(_) => {
            println!("{}", line_content);
            return;
        }
    };

    let step_idx = val.get("step_index").and_then(|v| v.as_u64()).unwrap_or(0);
    let stype = val
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("UNKNOWN");
    let status = val.get("status").and_then(|v| v.as_str()).unwrap_or("");
    let source = val.get("source").and_then(|v| v.as_str()).unwrap_or("");
    let content = val.get("content").and_then(|v| v.as_str()).unwrap_or("");

    if pretty {
        println!("\n# 🔍 Master Archive Record");
        println!(
            "> **Source**: `{}` • **Line**: `{}` • **Step**: `{}`\n",
            target_file.display(),
            line_no,
            step_idx
        );
        println!("- **Type**: `{}`", stype);
        if !status.is_empty() {
            println!("- **Status**: `{}`", status);
        }
        if !source.is_empty() {
            println!("- **Source**: `{}`", source);
        }

        if let Some(tool_calls) = val.get("tool_calls").and_then(|v| v.as_array()) {
            println!("- **Tool Calls**: {}", tool_calls.len());
            for tc in tool_calls {
                let name = tc.get("name").and_then(|v| v.as_str()).unwrap_or("tool");
                let args = tc.get("args").map(|a| a.to_string()).unwrap_or_default();
                println!("  - `{}`: `{}`", name, args);
            }
        }

        println!("\n### Content Output");
        if content.is_empty() {
            println!("*(Empty content)*");
        } else if full || content.chars().count() <= 1500 {
            println!("```\n{}\n```", content);
        } else {
            let cutoff = content
                .char_indices()
                .nth(1500)
                .map(|(i, _)| i)
                .unwrap_or(content.len());
            println!(
                "```\n{}\n... [truncated, use --full to view entire output]\n```",
                &content[..cutoff]
            );
        }
    } else {
        println!(
            "# Record: step={} line={} type={}",
            step_idx, line_no, stype
        );
        if full || content.chars().count() <= 2000 {
            println!("{}", content);
        } else {
            let cutoff = content
                .char_indices()
                .nth(2000)
                .map(|(i, _)| i)
                .unwrap_or(content.len());
            println!(
                "{}\n... [truncated, use --full to view entire output]",
                &content[..cutoff]
            );
        }
    }
}

fn handle_run(args: &[String]) {
    if args.is_empty() {
        eprintln!("Usage: shake-prune run <transcript_path.jsonl> [output_path.md] [OPTIONS]");
        process::exit(1);
    }

    let transcript_path = PathBuf::from(&args[0]);
    if let Err(err_msg) = validate_transcript_path(&transcript_path) {
        eprintln!("Security/Validation Error: {}", err_msg);
        process::exit(1);
    }

    let config = ShakeConfig::load();
    let mut raw_target = String::new();
    let mut requested_mode = CompactionMode::Auto;
    let mut timestamped_artifact = false;

    let mut options = CompactionOptions {
        recent_user_turns: config.retention.recent_user_turns,
        recent_tools_cap: config.retention.recent_tools_cap,
        recent_errors_cap: config.retention.recent_errors_cap,
        recent_window_steps: config.retention.recent_window_steps,
        thought_window_turns: None,
        marathon_horizon: false,
        in_place: true,
        dry_run: false,
        redact_secrets: config.privacy.redact_secrets,
        non_blocking_lock: false,
    };
    let mut json_output = false;
    let mut force = false;

    let mut i = 1;
    while i < args.len() {
        if args[i] == "--mode" && i + 1 < args.len() {
            if let Ok(m) = args[i + 1].parse::<CompactionMode>() {
                requested_mode = m;
            }
            i += 2;
        } else if args[i] == "--recent-user-turns" && i + 1 < args.len() {
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
            requested_mode = CompactionMode::Deep;
            i += 1;
        } else if args[i] == "--timestamped-artifact" {
            timestamped_artifact = true;
            i += 1;
        } else if args[i] == "--redact-secrets" {
            options.redact_secrets = true;
            i += 1;
        } else if args[i] == "--force" {
            force = true;
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

    let analysis_res = analyze_transcript(&transcript_path, &AnalysisOptions::default());
    let resolved_mode = if let Ok(ref analysis) = analysis_res {
        resolve_mode(requested_mode, analysis, config.deep_after_user_turns)
    } else {
        match requested_mode {
            CompactionMode::Deep => ResolvedMode::Deep,
            _ => ResolvedMode::Standard,
        }
    };

    if resolved_mode == ResolvedMode::Deep {
        options.marathon_horizon = true;
        if options.thought_window_turns.is_none() {
            options.thought_window_turns = Some(20);
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

    let default_artifact_name = if timestamped_artifact {
        stats.suggested_filename.clone()
    } else {
        "shake_latest.md".to_string()
    };

    let initial_output_path = if !raw_target.is_empty() {
        let p = PathBuf::from(&raw_target);
        if p.is_dir()
            || raw_target.ends_with('/')
            || raw_target.ends_with('\\')
            || (!raw_target.ends_with(".md") && !raw_target.contains('.'))
        {
            p.join(&default_artifact_name)
        } else {
            p
        }
    } else if let Some(parent) = transcript_path.parent() {
        if parent.ends_with("logs") {
            if let Some(grandparent) = parent.parent().and_then(|p| p.parent()) {
                grandparent.join(&default_artifact_name)
            } else {
                parent.join(&default_artifact_name)
            }
        } else {
            parent.join(&default_artifact_name)
        }
    } else {
        PathBuf::from(&default_artifact_name)
    };

    let abs_output_path =
        match validate_output_path_allowlist(&initial_output_path, &transcript_path, force) {
            Ok(p) => p,
            Err(err_msg) => {
                eprintln!("{}", err_msg);
                process::exit(1);
            }
        };

    let continuity_card = if let Ok(ref analysis) = analysis_res {
        Some(ContinuityCard::build(
            analysis,
            &master_archive_abs_str,
            &transcript_path,
            options.redact_secrets,
        ))
    } else {
        None
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
        shake_prune::atomic::set_user_only_permissions(&abs_output_path);

        let trigger_label = match resolved_mode {
            ResolvedMode::Deep => "Manual (/shake deep)",
            ResolvedMode::Standard => "Manual (/shake)",
        };
        let _ = write_artifact_metadata(&abs_output_path, &stats.topic_slug);
        let _ = write_active_anchor(
            &abs_output_path,
            &stats,
            trigger_label,
            &master_archive_abs_str,
            continuity_card.as_ref(),
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
            "newly_pruned_tools": stats.newly_pruned_tools,
            "already_pruned_tools": stats.already_pruned_tools,
            "retained_errors": stats.retained_errors,
            "retained_short_cmds": stats.retained_short_cmds,
            "retained_recent_steps": stats.retained_recent_steps,
            "topic_slug": stats.topic_slug,
            "suggested_filename": stats.suggested_filename,
            "master_archive": master_archive_abs_str,
            "output_path": abs_output_path.display().to_string(),
            "duration_ms": stats.duration_ms,
            "trigger_detail": stats.trigger_detail,
            "mode_requested": requested_mode.to_string(),
            "mode_resolved": resolved_mode.to_string(),
            "continuity": continuity_card,
        });
        println!("{}", json_val);
        return;
    }

    let abs_str = abs_output_path.to_string_lossy().to_string();

    let est_prompt_tokens_before = estimate_tokens(stats.this_run_before_bytes);
    let est_prompt_tokens_after = estimate_tokens(stats.this_run_after_bytes);

    let format_prompt_tokens = |tokens: usize| -> String {
        if tokens >= 1_000_000 {
            format!("~{:.1}M", tokens as f64 / 1_000_000.0)
        } else if tokens >= 1_000 {
            format!("~{}k", (tokens + 500) / 1000)
        } else {
            format!("~{}", tokens)
        }
    };

    let display_topic: String = stats
        .topic_slug
        .split('_')
        .filter(|s| !s.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    println!("\n# ⚡ Context Compacted");
    println!(
        "> **Session**: `{}` • **Status**: Ready to continue\n",
        display_topic
    );

    if options.dry_run {
        println!(
            "> ⚠️ **[Dry Run Active]**: No changes were written to `{}`.\n",
            transcript_path.display()
        );
    }

    let savings_label = if stats.this_run_savings_pct <= 0.0 {
        "0.0% (Already compact)".to_string()
    } else {
        format!("{:.1}% saved", stats.this_run_savings_pct)
    };
    let pruned_label = if stats.newly_pruned_tools == 0 && stats.already_pruned_tools > 0 {
        format!("Already compact ({} archived)", stats.already_pruned_tools)
    } else if stats.already_pruned_tools > 0 {
        format!(
            "{} newly pruned ({} total archived)",
            stats.newly_pruned_tools, stats.pruned_tools
        )
    } else {
        format!("{} pruned", stats.newly_pruned_tools)
    };

    println!("| Metric | Before | After | Reduction |");
    println!("| :--- | :--- | :--- | :--- |");
    println!(
        "| **Context Payload** | `{}` ({}) | **`{}`** ({}) | **`{}`** |",
        format_bytes(stats.this_run_before_bytes),
        format_prompt_tokens(est_prompt_tokens_before),
        format_bytes(stats.this_run_after_bytes),
        format_prompt_tokens(est_prompt_tokens_after),
        savings_label
    );
    println!(
        "| **Pruned Tool Bloat** | {} tool executions | **Clean receipts** (`archive=...`) | **{}** |",
        stats.pruned_tools,
        pruned_label
    );
    println!(
        "| **Working Memory** | Last {} user turns | **100% dialogue preserved** | Active |\n",
        options.recent_user_turns
    );

    println!("### [Open Summary Artifact](file://{})", abs_str);
    println!(
        "*Click above to open the executive summary in the artifact viewer or copy milestones.*\n"
    );

    println!("<details>");
    println!("<summary>⚙️ Archive & Working Tools</summary>\n");
    println!(
        "- **Master Archive**: [transcript_full.jsonl](file://{})",
        master_archive_abs_str
    );
    println!(
        "- **Active Working Tools**: Kept last {} tool outputs and {} un-clamped error traces in active memory.",
        stats.retained_recent_steps,
        stats.retained_errors
    );
    println!("</details>\n");
}

fn print_usage() {
    println!(
        r#"shake-prune {} - Context compaction and utility suite for Google Antigravity

USAGE:
    shake-prune <SUBCOMMAND> [OPTIONS]
    shake-prune <transcript_path.jsonl> [output_path.md] [OPTIONS] (alias for 'run')

SUBCOMMANDS:
    run       Execute context compaction (adaptive standard/deep mode)
    preview   Read-only preview of compaction impact and continuity anchor
    status    Inspect transcript size, archive health, and compaction recommendation
    undo      Restore previous transcript state from atomic backup (alias: restore)
    show      Inspect archived tool outputs from transcript_full.jsonl
    doctor    Verify installation, config, permissions, and hook registration

OPTIONS (for 'run'):
    --mode <auto|standard|deep>  Compaction mode (default: auto)
    --recent-user-turns <N>      Recent user turns to keep unpruned (default: 10)
    --tools-cap <N>              Maximum recent tool outputs to keep (default: 20)
    --errors-cap <N>             Maximum recent tool calls to preserve raw errors (default: 30)
    --recent-window <N>          Fallback raw tool step window if user-turns=0 (default: 6)
    --timestamped-artifact       Generate timestamped artifact instead of shake_latest.md
    --redact-secrets             Redact credentials in active JSONL and artifacts
    --force                      Allow overwriting existing output files
    --dry-run                    Simulate compaction without modifying transcript.jsonl
    --no-in-place                Generate artifact without truncating JSONL
    --json                       Emit machine-readable JSON metrics
    --help, -h                   Show this help message
    --version, -v                Show version
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

    match args[1].as_str() {
        "doctor" | "--doctor" => {
            let json_flag = args.iter().any(|a| a == "--json");
            handle_doctor(json_flag);
            process::exit(0);
        }
        "undo" | "restore" => {
            if args.len() < 3 {
                eprintln!("Usage: shake-prune undo <path/to/transcript.jsonl> [--force]");
                process::exit(1);
            }
            let force = args.iter().any(|a| a == "--force");
            let target = args.iter().skip(2).find(|a| *a != "--force");
            if let Some(target_str) = target {
                handle_restore(&PathBuf::from(target_str), force);
            } else {
                eprintln!("Usage: shake-prune undo <path/to/transcript.jsonl> [--force]");
                process::exit(1);
            }
            process::exit(0);
        }
        "status" => {
            if args.len() < 3 {
                eprintln!("Usage: shake-prune status <path/to/transcript.jsonl> [--json]");
                process::exit(1);
            }
            let json_flag = args.iter().any(|a| a == "--json");
            let target = args.iter().skip(2).find(|a| *a != "--json");
            if let Some(target_str) = target {
                handle_status(&PathBuf::from(target_str), json_flag);
            } else {
                eprintln!("Usage: shake-prune status <path/to/transcript.jsonl> [--json]");
                process::exit(1);
            }
            process::exit(0);
        }
        "preview" => {
            if args.len() < 3 {
                eprintln!("Usage: shake-prune preview <path/to/transcript.jsonl> [--mode auto|standard|deep] [--json]");
                process::exit(1);
            }
            let json_flag = args.iter().any(|a| a == "--json");
            let mut requested_mode = CompactionMode::Auto;
            let mut target_str = None;
            let mut i = 2;
            while i < args.len() {
                if args[i] == "--mode" && i + 1 < args.len() {
                    if let Ok(m) = args[i + 1].parse::<CompactionMode>() {
                        requested_mode = m;
                    }
                    i += 2;
                } else if args[i] == "--json" {
                    i += 1;
                } else if !args[i].starts_with('-') && target_str.is_none() {
                    target_str = Some(&args[i]);
                    i += 1;
                } else {
                    i += 1;
                }
            }
            if let Some(t) = target_str {
                handle_preview(&PathBuf::from(t), requested_mode, json_flag);
            } else {
                eprintln!("Usage: shake-prune preview <path/to/transcript.jsonl> [--mode auto|standard|deep] [--json]");
                process::exit(1);
            }
            process::exit(0);
        }
        "show" => {
            if args.len() < 3 {
                eprintln!("Usage: shake-prune show <path/to/transcript.jsonl> (--step <N> | --line <N> | step=N | line=N) [--pretty] [--json] [--full]");
                process::exit(1);
            }
            let json_flag = args.iter().any(|a| a == "--json");
            let pretty_flag = args.iter().any(|a| a == "--pretty");
            let full_flag = args.iter().any(|a| a == "--full");
            let mut step_opt = None;
            let mut line_opt = None;
            let mut target_str = None;
            let mut i = 2;
            while i < args.len() {
                if (args[i] == "--step" || args[i] == "-s") && i + 1 < args.len() {
                    step_opt = args[i + 1].parse::<u64>().ok();
                    i += 2;
                } else if (args[i] == "--line" || args[i] == "-l") && i + 1 < args.len() {
                    line_opt = args[i + 1].parse::<usize>().ok();
                    i += 2;
                } else if args[i].starts_with("step=") {
                    step_opt = args[i].trim_start_matches("step=").parse::<u64>().ok();
                    i += 1;
                } else if args[i].starts_with("line=") {
                    line_opt = args[i].trim_start_matches("line=").parse::<usize>().ok();
                    i += 1;
                } else if args[i] == "--json" || args[i] == "--pretty" || args[i] == "--full" {
                    i += 1;
                } else if !args[i].starts_with('-') && target_str.is_none() {
                    target_str = Some(&args[i]);
                    i += 1;
                } else {
                    i += 1;
                }
            }
            if let Some(t) = target_str {
                handle_show(
                    &PathBuf::from(t),
                    step_opt,
                    line_opt,
                    pretty_flag,
                    json_flag,
                    full_flag,
                );
            } else {
                eprintln!("Usage: shake-prune show <path/to/transcript.jsonl> (--step <N> | --line <N>) [--pretty] [--json] [--full]");
                process::exit(1);
            }
            process::exit(0);
        }
        "run" => {
            let run_args: Vec<String> = args.iter().skip(2).cloned().collect();
            if run_args.is_empty() {
                eprintln!(
                    "Usage: shake-prune run <transcript_path.jsonl> [output_path.md] [OPTIONS]"
                );
                process::exit(1);
            }
            handle_run(&run_args);
            process::exit(0);
        }
        _ => {
            let run_args: Vec<String> = args.iter().skip(1).cloned().collect();
            handle_run(&run_args);
            process::exit(0);
        }
    }
}
