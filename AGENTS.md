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

### 5. Configuration Subsystem (`shake.toml` & Env Overrides)
Optionally configure system-wide compaction preferences in `<Global Config Directory>/shake.toml`:

```toml
[auto]
enabled = true                  # Set to false to disable auto-shake hook completely
token_threshold_bytes = 264000  # ~80k tokens
tool_burst_threshold = 20       # Autonomous tool burst trigger
cooldown_seconds = 180          # 3-minute cooldown between compactions
growth_delta_bytes = 25600      # 25 KB transcript growth required

[retention]
recent_user_turns = 10          # Keep last N human user turns verbatim
recent_tools_cap = 20           # Keep last N tool runs raw
recent_errors_cap = 30          # Keep un-clamped error traces for last N tools

[privacy]
redact_secrets = false          # Redact API keys, tokens, and bearer secrets

[diagnostics]
log_level = "info"
```

All settings can be overridden via environment variables:
`SHAKE_AUTO_DISABLE=1`, `SHAKE_RECENT_USER_TURNS`, `SHAKE_TOOLS_CAP`, `SHAKE_ERRORS_CAP`,
`SHAKE_TOKEN_THRESHOLD_BYTES`, `SHAKE_TOOL_BURST_THRESHOLD`, `SHAKE_COOLDOWN_SECONDS`,
`SHAKE_GROWTH_DELTA_BYTES`, `SHAKE_SECRET_REDACTION=1`.

---

## 🔒 Invariant Requirements for Context Pruning

Any modifications or custom builds of the pruner engine **MUST adhere to these non-negotiable rules**:
1. **100% Verbatim Prompts & Dialogue**: Never use an LLM to summarize conversation turns. User requests and assistant replies must be preserved character-for-character.
2. **Signal Preservation**: Retain all assistant explanations, code diffs, decisions, and un-clamped error traces within the recent 30-call tool error window.
3. **Master Archive Completeness**: Ensure `transcript_full.jsonl` contains all active transcript steps *before* pruning so every `archive=... line=N` receipt resolves to an existing line.
4. **Crash Recovery & Pre-Commit Protection**: Use intent journaling (`.shake_in_progress`) to enable automatic recovery if interrupted, and verify snapshot fingerprints before truncation to detect uncooperative concurrent writes.
5. **Thought Windowing (`/full-shake`)**: When `--full` is specified, retain scratchpad thoughts for the latest 20 turns while dropping older thoughts (Turn 1 Genesis thoughts are always preserved verbatim).
6. **Active Working Window**: Retain the outputs of the last 10 user turns verbatim (capped at the last 20 tool executions, retaining raw traces for any errors in the last 30 tools, with a fallback 6-step window when user-turns=0) to prevent broken agent momentum.
7. **Selective Ephemeral Deduplication**: Only prune `/shake` and `HOOK_NOTICE`/`ANCHOR_NOTICE` ephemeral messages; third-party ephemeral notifications must remain intact.
8. **Permission Hardening**: Sensitive files (`.shake_in_progress`, `transcript_full.jsonl`, `shake_hook.log`, `shake_metadata.json`, backups) must be created with restricted user-only permissions (`0600` on Unix).
9. **Fail-Open Hook**: The `--hook` command must exit with code `0` and output `{}` upon any unexpected error or lock contention (non-blocking lock + 2.5s watchdog) so it never blocks the user's chat.
10. **Untouched Full Stream**: Never prune `transcript_full.jsonl` on disk. It serves as the developer's permanent unpruned audit log.

---

## ⚡ Self-Verification & Uninstallation
After installing, run:
- `<Global Binary Directory>/shake-prune --version` (Prints `shake-prune 0.1.10`).
- `<Global Binary Directory>/shake-prune --help` (Exit code must be `0`).
- `<Global Binary Directory>/shake-prune doctor` (Verifies environment and config health).
- Confirm `<Global Skill Directory>/SKILL.md` exists and is non-empty.

To completely uninstall:
- Linux/macOS: `./install.sh --uninstall`
- Windows: `powershell -File .\install.ps1 -Uninstall`
