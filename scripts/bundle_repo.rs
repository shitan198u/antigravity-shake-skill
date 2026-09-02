use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

fn is_text_file(path: &Path) -> bool {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");

    if file_name == ".gitignore" || file_name == "hooks.json" {
        return true;
    }

    matches!(
        ext,
        "rs" | "toml" | "md" | "sh" | "ps1" | "json" | "yml" | "yaml"
    )
}

fn should_skip_dir(entry_name: &str) -> bool {
    matches!(
        entry_name,
        ".git" | "target" | "bin" | "assets" | ".system_generated" | "scratch"
    )
}

fn collect_files(dir: &Path, base: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    if dir.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .collect();
        entries.sort_by_key(|e| e.path());

        for entry in entries {
            let path = entry.path();
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();

            if path.is_dir() {
                if !should_skip_dir(&name_str) {
                    collect_files(&path, base, files)?;
                }
            } else if path.is_file() {
                // Skip output file itself or binary artifacts
                if name_str.ends_with(".txt") || name_str == "bundle_repo" {
                    continue;
                }
                if is_text_file(&path) {
                    files.push(path);
                }
            }
        }
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let root_path = if let Some(arg) = std::env::args().nth(1) {
        PathBuf::from(arg)
    } else {
        std::env::current_dir().unwrap()
    };

    let mut files = Vec::new();
    collect_files(&root_path, &root_path, &mut files)?;
    files.sort();

    let output_path = root_path.join("scripts").join("repo_bundle.txt");
    let mut out = File::create(&output_path)?;

    writeln!(out, "================================================================================")?;
    writeln!(out, "  ANTIGRAVITY SHAKE COMPLETE REPOSITORY & CODEBASE DUMP")?;
    writeln!(out, "  Total Files Bundled: {}", files.len())?;
    writeln!(out, "================================================================================\n")?;

    writeln!(out, "TABLE OF CONTENTS")?;
    writeln!(out, "--------------------------------------------------------------------------------")?;
    let mut total_lines = 0usize;
    let mut total_bytes = 0usize;

    for (idx, path) in files.iter().enumerate() {
        let rel_path = path.strip_prefix(&root_path).unwrap_or(path);
        let content = fs::read_to_string(path).unwrap_or_default();
        let lines = content.lines().count();
        let bytes = content.len();
        total_lines += lines;
        total_bytes += bytes;
        writeln!(out, "{:2}. {:<55} | {:4} lines | {:6} bytes", idx + 1, rel_path.display(), lines, bytes)?;
    }

    writeln!(out, "--------------------------------------------------------------------------------")?;
    writeln!(out, "TOTAL: {} files | {} lines | {} bytes\n", files.len(), total_lines, total_bytes)?;
    writeln!(out, "================================================================================\n")?;

    for (idx, path) in files.iter().enumerate() {
        let rel_path = path.strip_prefix(&root_path).unwrap_or(path);
        let content = fs::read_to_string(path).unwrap_or_default();
        let lines = content.lines().count();
        let bytes = content.len();

        writeln!(out, "================================================================================")?;
        writeln!(out, "FILE #{}: {}", idx + 1, rel_path.display())?;
        writeln!(out, "METRICS: {} lines | {} bytes", lines, bytes)?;
        writeln!(out, "================================================================================\n")?;
        writeln!(out, "{}\n", content)?;
    }

    println!("Successfully generated codebase bundle at: {}", output_path.display());
    println!("Total Files: {} | Total Lines: {} | Total Size: {} bytes", files.len(), total_lines, total_bytes);

    Ok(())
}
