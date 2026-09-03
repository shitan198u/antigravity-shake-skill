pub mod atomic;
pub mod hook;
pub mod metadata;
pub mod models;
pub mod pruner;
pub mod receipts;
pub mod slug;

use std::env;
use std::path::{Path, PathBuf};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn format_bytes(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

pub fn is_sensitive_system_path(path: &Path) -> bool {
    let path_str = path.to_string_lossy().to_lowercase().replace('\\', "/");

    let unix_forbidden = [
        "/etc", "/root", "/boot", "/dev", "/proc", "/sys", "/usr", "/bin", "/sbin", "/var/log",
    ];

    for prefix in &unix_forbidden {
        if path_str == *prefix || path_str.starts_with(&format!("{}/", prefix)) {
            return true;
        }
    }

    let windows_forbidden = [
        "c:/windows",
        "c:/program files",
        "c:/program files (x86)",
        "c:/programdata",
        "c:/system volume information",
    ];

    for prefix in &windows_forbidden {
        if path_str == *prefix || path_str.starts_with(&format!("{}/", prefix)) {
            return true;
        }
    }

    false
}

pub fn validate_transcript_path(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!(
            "Transcript file does not exist: {}",
            path.display()
        ));
    }
    if !path.is_file() {
        return Err(format!("Target is not a file: {}", path.display()));
    }

    let canonical = path.canonicalize().map_err(|e| {
        format!(
            "Failed to canonicalize transcript path '{}': {}",
            path.display(),
            e
        )
    })?;

    if is_sensitive_system_path(&canonical) || is_sensitive_system_path(path) {
        return Err(format!(
            "Security Error: Transcript path '{}' is located within a restricted sensitive system directory.",
            path.display()
        ));
    }

    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    if !file_name.ends_with(".jsonl") && !file_name.ends_with(".jsonl.bak") {
        return Err(format!(
            "Invalid file type: '{}'. /shake only operates on .jsonl or .jsonl.bak transcript log files.",
            file_name
        ));
    }

    Ok(())
}

pub fn validate_output_path_allowlist(
    target: &Path,
    transcript_path: &Path,
) -> Result<PathBuf, String> {
    let target_parent = if target.is_dir() {
        target.to_path_buf()
    } else {
        match target.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => transcript_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf(),
        }
    };

    // Check sensitive system denylist upfront before canonicalization so
    // paths like /etc or C:\Windows are rejected on all platforms (§4.1 / cross-OS parity).
    if is_sensitive_system_path(target) || is_sensitive_system_path(&target_parent) {
        return Err(format!(
            "Security Error: Output path '{}' is located within a restricted sensitive system directory.",
            target.display()
        ));
    }

    let canonical_target = target_parent.canonicalize().map_err(|_| {
        format!(
            "Output target directory does not exist or is invalid: {}",
            target_parent.display()
        )
    })?;

    if is_sensitive_system_path(&canonical_target) {
        return Err(format!(
            "Security Error: Output path '{}' is located within a restricted sensitive system directory.",
            target.display()
        ));
    }

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

    let is_allowed = allowed_roots
        .iter()
        .any(|allowed| canonical_target.starts_with(allowed));

    if !is_allowed {
        return Err(format!(
            "Security Error: Output path '{}' is outside the permitted allowlist hierarchy.",
            target.display()
        ));
    }

    if target.is_dir() {
        Ok(canonical_target)
    } else {
        let file_name = target.file_name().unwrap_or_default();
        Ok(canonical_target.join(file_name))
    }
}
