# Agent Instructions: Cross-Platform Adaptation & Installation Guide

This document is a direct guide for **AI Coding Agents** (Google Antigravity, Claude Code, Cursor, Aider, etc.) running on a user's machine to automatically configure, adapt, or compile the `/shake` skill on **any operating system** (Linux, macOS, Windows).

---

## 🎯 Primary Goal
Install the `/shake` context compaction skill globally into the user's Antigravity environment so that typing `/shake` in any chat session instantly prunes token bloat and sets an in-window continuity anchor.

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
3. **Python Fallback Engine**:
   - If neither prebuilt binary nor Rust compiler is available, copy `scripts/shake_prune.py` to `<Global Skill Directory>/scripts/shake_prune.py`.

### 3. Deploy Skill Definition
Copy `SKILL.md` to `<Global Skill Directory>/SKILL.md`.

### 4. Configure PreInvocation Hook (`hooks.json`)
Ensure the `PreInvocation` hook is configured in `<Global Config Directory>/hooks.json`:

#### On Linux / macOS:
```json
{
  "shake-anchor": {
    "enabled": true,
    "PreInvocation": [
      {
        "type": "command",
        "command": "~/.gemini/bin/shake-prune --hook"
      }
    ]
  }
}
```

#### On Windows:
```json
{
  "shake-anchor": {
    "enabled": true,
    "PreInvocation": [
      {
        "type": "command",
        "command": "%USERPROFILE%\\.gemini\\bin\\shake-prune.exe --hook"
      }
    ]
  }
}
```

---

## 🔒 Invariant Requirements for Context Pruning

Any modifications or custom builds of the pruner engine **MUST adhere to these non-negotiable rules**:
1. **100% Verbatim Prompts & Dialogue**: Never use an LLM to summarize conversation turns. User requests and assistant replies must be preserved character-for-character.
2. **100% Thought Preservation**: Retain all `<details><summary>💭 Thought Process</summary>...</details>` blocks.
3. **100% Error Preservation**: All tool calls with `exit_code != 0` or failure statuses must retain their full stack traces/logs.
4. **Active Working Window**: Retain the outputs of the last 6 tool steps to prevent broken momentum.
5. **Fail-Open Hook**: The `--hook` command must exit with code `0` and output `{}` upon any unexpected error so it never blocks the user's chat.

---

## ⚡ Self-Verification
After installing, run:
- `<Global Binary Directory>/shake-prune --help` (Exit code must be `0`).
- Confirm `<Global Skill Directory>/SKILL.md` exists and is non-empty.
