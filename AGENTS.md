# Agent Instructions: Cross-Platform Adaptation & Installation Guide

This document is a direct guide for **AI Coding Agents** (Google Antigravity, Claude Code, Cursor, Aider, etc.) running on a user's machine to automatically configure, adapt, or compile the `/shake` and `/full-shake` skills on **any operating system** (Linux, macOS, Windows).

---

## 🎯 Primary Goal
Install the `/shake` and `/full-shake` context compaction skills globally into the user's Antigravity environment so that typing `/shake` or `/full-shake` in any chat session instantly prunes token bloat and sets an in-window continuity anchor.

---

## 🗺️ Platform Path Resolution Matrix

| Platform | Global Skill Directory | Global Binary Directory | Lifecycle Hooks Config |
| :--- | :--- | :--- | :--- |
| **Linux** | `~/.gemini/config/skills/shake` | `~/.gemini/bin/shake-prune` | `~/.gemini/config/hooks.json` |
| **macOS** | `~/.gemini/config/skills/shake` | `~/.gemini/bin/shake-prune` | `~/.gemini/config/hooks.json` |
| **Windows** | `%USERPROFILE%\.gemini\config\skills\shake` | `%USERPROFILE%\.gemini\bin\shake-prune.exe` | `%USERPROFILE%\.gemini\config\hooks.json` |

---

## 🛠️ Step-by-Step Installation Protocol for Agents

When a user asks you to install or enable `/shake` on their machine:

### 1. Identify Host Environment & Architecture
Check the current OS and CPU architecture:
- **Linux**: `uname -s` (Linux), `uname -m` (x86_64 / aarch64)
- **macOS**: `uname -s` (Darwin), `uname -m` (arm64 / x86_64)
- **Windows**: `$env:OS`, `$env:PROCESSOR_ARCHITECTURE` (AMD64 / ARM64)

### 2. Binary Installation Strategy (Cascade)
1. **Prebuilt Binary Check**:
   - On **Linux x86_64**: Use `bin/shake-prune` directly from this repo.
   - On other platforms: If a prebuilt binary matching the target OS/Arch is available (from GitHub Releases or `bin/`), copy it directly to `<Global Binary Directory>`.
2. **Local Compilation (If `cargo` is installed)**:
   - Run:
     ```bash
     cargo build --release --manifest-path shake-prune-rs/Cargo.toml
     ```
   - Copy the compiled binary:
     - Linux/macOS: `shake-prune-rs/target/release/shake-prune` ➔ `<Global Binary Directory>/shake-prune`
     - Windows: `shake-prune-rs\target\release\shake-prune.exe` ➔ `%USERPROFILE%\.gemini\bin\shake-prune.exe`
3. **Automated Shell Installer**:
   - Alternatively, execute the self-contained installer script:
     - Linux/macOS: `./install.sh`
     - Windows: `powershell -ExecutionPolicy Bypass -File .\install.ps1`

### 3. Deploy Skill Definitions
- Copy `SKILL.md` to `<Global Skill Directory>/SKILL.md`.
- Copy `skills/full-shake/SKILL.md` to `<Global Config Directory>/skills/full-shake/SKILL.md`.
- Copy `references/` into `<Global Skill Directory>/references/`.

### 4. Configure PreInvocation Hook (`hooks.json`)
Ensure the `PreInvocation` hook is configured in `<Global Config Directory>/hooks.json`:

```json
{
  "hooks": {
    "PreInvocation": [
      {
        "command": "shake-prune --hook"
      }
    ]
  }
}
```

---

## 🔒 Invariant Requirements for Context Pruning

Any modifications or custom builds of the pruner engine **MUST adhere to these non-negotiable rules**:
1. **100% Verbatim Prompts & Dialogue**: Never use an LLM to summarize conversation turns. User requests and assistant replies must be preserved character-for-character.
2. **Signal Preservation**: Retain all assistant explanations, code diffs, decisions, and non-zero exit error traces (`exit_code != 0`).
3. **Thought Windowing (`/full-shake`)**: When `--full` is specified, retain scratchpad thoughts for the latest 20 turns while dropping older thoughts.
4. **Active Working Window**: Retain the outputs of the last 6 tool execution steps to prevent broken momentum.
5. **Fail-Open Hook**: The `--hook` command must exit with code `0` and output `{}` upon any unexpected error so it never blocks the user's chat.
6. **Untouched Full Stream**: Never prune `transcript_full.jsonl` on disk. It serves as the developer's permanent unpruned audit log.

---

## ⚡ Self-Verification & Uninstallation
After installing, run:
- `<Global Binary Directory>/shake-prune --version` (Prints `shake-prune 0.1.8`).
- `<Global Binary Directory>/shake-prune --help` (Exit code must be `0`).
- Confirm `<Global Skill Directory>/SKILL.md` exists and is non-empty.

To completely uninstall:
- Linux/macOS: `./install.sh --uninstall`
- Windows: `powershell -File .\install.ps1 -Uninstall`
