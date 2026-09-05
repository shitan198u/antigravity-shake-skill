#!/usr/bin/env python3
"""
bundle_repo.py
Bundles the entire Antigravity /shake codebase and documentation into a single
clean text file for external AI code review, auditing, and architectural evaluation.

Usage:
    python3 scripts/bundle_repo.py [--output scripts/repo_bundle.txt]
"""

import os
import sys
import argparse
from pathlib import Path
from datetime import datetime, timezone

SKIP_DIRS = {
    ".git",
    "target",
    "bin",
    "dist",
    "assets",
    ".system_generated",
    "scratch",
    "node_modules",
    ".idea",
    ".vscode",
}

VALID_EXTENSIONS = {
    ".rs",
    ".toml",
    ".md",
    ".sh",
    ".ps1",
    ".json",
    ".yml",
    ".yaml",
}

SKIP_FILES = {
    "repo_bundle.txt",
    "Cargo.lock",
}

# Preferred ordering for intuitive architectural review
PRIORITY_ORDER = [
    "LICENSE",
    "README.md",
    "AGENTS.md",
    "hooks.json",
    "install.sh",
    "install.ps1",
    ".github/workflows/ci.yml",
    ".github/workflows/release.yml",
    "references/antigravity_lifecycle.md",
    "references/how_it_works.md",
    "references/omp_comparison.md",
    "skills/shake/SKILL.md",
    "shake-prune-rs/Cargo.toml",
    "shake-prune-rs/src/analysis.rs",
    "shake-prune-rs/src/mode.rs",
    "shake-prune-rs/src/continuity.rs",
    "shake-prune-rs/src/models.rs",
    "shake-prune-rs/src/slug.rs",
    "shake-prune-rs/src/metadata.rs",
    "shake-prune-rs/src/pruner.rs",
    "shake-prune-rs/src/hook.rs",
    "shake-prune-rs/src/config.rs",
    "shake-prune-rs/src/main.rs",
    "shake-prune-rs/tests/integration_tests.rs",
]

def get_repo_root() -> Path:
    script_dir = Path(__file__).resolve().parent
    if (script_dir.parent / "shake-prune-rs").exists():
        return script_dir.parent
    return Path.cwd()

def collect_files(repo_root: Path):
    collected = []
    for root, dirs, files in os.walk(repo_root):
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS and (not d.startswith(".") or d == ".github")]
        for f in files:
            if f in SKIP_FILES:
                continue
            path = Path(root) / f
            rel_path = path.relative_to(repo_root)
            rel_str = str(rel_path).replace("\\", "/")

            if f in {"hooks.json", "LICENSE"} or path.suffix.lower() in VALID_EXTENSIONS:
                collected.append(rel_str)

    def sort_key(item: str):
        if item in PRIORITY_ORDER:
            return (0, PRIORITY_ORDER.index(item))
        return (1, item)

    collected.sort(key=sort_key)
    return collected

def generate_bundle(repo_root: Path, output_file: Path):
    rel_files = collect_files(repo_root)
    now_iso = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC")

    lines_out = []
    lines_out.append("=" * 80)
    lines_out.append("ANTIGRAVITY /SHAKE CONTEXT ENGINE — COMPLETE CODEBASE & DOCS BUNDLE")
    lines_out.append("=" * 80)
    lines_out.append(f"Generated At: {now_iso}")
    lines_out.append(f"Repository Root: {repo_root}")
    lines_out.append(f"Total Files Bundled: {len(rel_files)}")
    lines_out.append("")
    lines_out.append("INTENTIONALLY OMITTED FILES (PRESENT IN REPOSITORY, SKIPPED IN BUNDLE):")
    lines_out.append("  - shake-prune-rs/Cargo.lock: Committed & tracked in Git for deterministic builds.")
    lines_out.append("    Omitted from this text bundle to conserve context tokens during LLM code review.")
    lines_out.append("  - bin/shake-prune & bin/shake-prune.exe: Compiled machine binaries built via CI / cargo.")
    lines_out.append("    Omitted from text bundle as non-text binary artifacts.")
    lines_out.append("  - target/ & .git/: Standard Rust target build cache and Git VCS internal metadata.")
    lines_out.append("")
    lines_out.append("TABLE OF CONTENTS:")
    lines_out.append("-" * 80)

    # Pre-read to compute line counts for Table of Contents
    file_stats = []
    for rel in rel_files:
        full_path = repo_root / rel
        try:
            with open(full_path, "r", encoding="utf-8", errors="replace") as fh:
                content = fh.read()
                line_count = len(content.splitlines())
                char_count = len(content)
                file_stats.append((rel, line_count, char_count, content))
        except Exception as e:
            file_stats.append((rel, 0, 0, f"Error reading file: {e}"))

    for idx, (rel, line_count, char_count, _) in enumerate(file_stats, start=1):
        lines_out.append(f"{idx:2d}. {rel:<48} ({line_count:>4} lines, {char_count:>6} bytes)")

    lines_out.append("-" * 80)
    lines_out.append("\n")

    # Append file contents
    for idx, (rel, line_count, char_count, content) in enumerate(file_stats, start=1):
        lines_out.append("=" * 80)
        lines_out.append(f"[{idx}/{len(file_stats)}] FILE: {rel}")
        lines_out.append(f"Path: {rel} | Lines: {line_count} | Bytes: {char_count}")
        lines_out.append("=" * 80)
        lines_out.append(content)
        if not content.endswith("\n"):
            lines_out.append("\n")
        lines_out.append("\n")

    output_file.parent.mkdir(parents=True, exist_ok=True)
    with open(output_file, "w", encoding="utf-8") as out:
        out.write("\n".join(lines_out))

    total_size = output_file.stat().st_size
    total_lines = sum(s[1] for s in file_stats)
    print(f"✅ Successfully bundled {len(rel_files)} files into: {output_file}")
    print(f"📊 Total lines: {total_lines:,} | Size: {total_size / 1024:.1f} KB")

def main():
    parser = argparse.ArgumentParser(description="Bundle Antigravity Shake codebase and documentation into a single text file.")
    repo_root = get_repo_root()
    default_output = repo_root / "scripts" / "repo_bundle.txt"
    parser.add_argument("-o", "--output", type=Path, default=default_output, help=f"Destination bundle file (default: {default_output})")
    args = parser.parse_args()

    generate_bundle(repo_root, args.output)

if __name__ == "__main__":
    main()
