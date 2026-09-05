# Agent Instructions: Cross-Platform Adaptation & Installation Guide

This document is a direct guide for **AI Coding Agents** (Google Antigravity, Claude Code, Cursor, Aider, etc.) running on a user's machine to automatically configure, adapt, or compile the `/shake` skill and utility suite on **any operating system** (Linux, macOS, Windows).

---

## 🎯 Primary Goal
Install the `/shake` context compaction skill globally into the user's Antigravity environment so that typing `/shake` in any chat session instantly prunes token bloat, adapts standard or deep compaction automatically, and sets an in-window continuity anchor.

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
- Copy `skills/shake/SKILL.md` (or repo root `SKILL.md`) to `<Global Skill Directory>/SKILL.md`.
- Copy `references/` into `<Global Skill Directory>/references/`.

### 4. Configure PreInvocation Hook (`hooks.json`)
Ensure the `PreInvocation` and `Stop` hooks are configured in `<Global Config Directory>/hooks.json`:

```json
{
  "hooks": {
    "PreInvocation": [
      {
        "command": "shake-prune --hook"
      }
    ],
    "Stop": [
      {
        "command": "shake-prune --hook"
      }
    ]
  }
}
```

> [!NOTE]
> **PATH Resolution**: If `<Global Binary Directory>` (`~/.gemini/bin`) is not in your system `PATH`, configure the absolute path to the binary (e.g. `~/.gemini/bin/shake-prune --hook` or `%USERPROFILE%\.gemini\bin\shake-prune.exe --hook`). The self-contained `./install.sh` and `install.ps1` installers automatically configure the absolute path.

---

### 5. Configuration Subsystem (`shake.toml` & Env Overrides)
Optionally configure compaction preferences in `<Global Config Directory>/shake.toml`:

```toml
[shake]
keep_recent_turns = 10          # Keep last N human user turns verbatim
keep_recent_tools = 20          # Keep last N tool runs raw
keep_recent_errors = 30         # Keep un-clamped error traces for last N tools
deep_after_user_turns = 30      # Automatically switch to deep compaction past 30 turns
redact_secrets = false          # Redact API keys, tokens, and bearer secrets

[advanced]
auto_enabled = true             # Set to false to disable auto-shake hook completely
token_threshold_bytes = 264000  # ~80k tokens
tool_burst_threshold = 20       # Autonomous tool burst trigger
cooldown_seconds = 180          # 3-minute cooldown between compactions
growth_delta_bytes = 25600      # 25 KB transcript growth required

[retention]
artifact_retention_count = 20   # Maximum historical artifacts retained

[diagnostics]
log_level = "info"
```

Legacy `[shake]` keys only fill modern counterparts left at defaults; explicit
`[retention]` / `[diagnostics]` / top-level values always win. Sensitive artifacts
are always created `0600` on Unix; Windows ACLs are out of scope. The `.bak`
crash-recovery copy is verbatim (unredacted) by design.

All settings can be overridden via environment variables:
`SHAKE_KEEP_RECENT_TURNS`, `SHAKE_KEEP_RECENT_TOOLS`, `SHAKE_KEEP_RECENT_ERRORS`,
`SHAKE_DEEP_AFTER_TURNS`, `SHAKE_SECRET_REDACTION=1`, `SHAKE_AUTO_DISABLE=1`,
`SHAKE_RECENT_WINDOW`, `SHAKE_TOKEN_THRESHOLD_BYTES`, `SHAKE_TOOL_BURST_THRESHOLD`,
`SHAKE_COOLDOWN_SECONDS`, `SHAKE_GROWTH_DELTA_BYTES`, `SHAKE_ARTIFACT_RETENTION`,
`SHAKE_LOG_LEVEL`.

---

## 🛠️ Unified CLI Utility Suite (v0.2.0)

| Subcommand | Syntax | Description |
| :--- | :--- | :--- |
| `run` | `shake-prune run <transcript> [output] [--mode auto\|standard\|deep]` | Run adaptive compaction (default output: `shake_latest.md`) |
| `preview` | `shake-prune preview <transcript> [--json]` | Read-only simulation of reduction metrics and continuity anchor |
| `status` | `shake-prune status <transcript> [--json]` | Inspect token count, archive health, and compaction recommendation |
| `undo` | `shake-prune undo <transcript> [--force]` | Rollback from `.jsonl.bak` with `.pre_restore` snapshot safety |
| `show` | `shake-prune show <transcript> --step N \| --line N [--pretty]` | Inspect archived tool execution from permanent `transcript_full.jsonl` |
| `doctor` | `shake-prune doctor [--json]` | Diagnostics: verify hook, config, storage root, and permissions |

---

## 🔒 Invariant Requirements for Context Pruning

Any modifications or custom builds of the pruner engine **MUST adhere to these non-negotiable rules**:
1. **100% Verbatim Prompts & Dialogue**: Never use an LLM to summarize conversation turns. User requests and assistant replies must be preserved character-for-character.
2. **Signal Preservation**: Retain all assistant explanations, code diffs, decisions, and un-clamped error traces within the recent 30-call tool error window.
3. **Master Archive Completeness**: Ensure `transcript_full.jsonl` contains all active transcript steps *before* pruning so every `archive=... line=N` receipt resolves to an existing line.
4. **Crash Recovery & Pre-Commit Protection**: Use intent journaling (`.shake_in_progress`) to enable automatic recovery if interrupted, and verify snapshot fingerprints before truncation to detect uncooperative concurrent writes.
5. **Adaptive Deep Compaction**: When user turns > 30, automatically retain scratchpad thoughts for the latest 20 turns and apply Milestone Horizon (Turn 1 Genesis thoughts are always preserved verbatim).
6. **Active Working Window**: Retain the outputs of the last 10 user turns verbatim (capped at the last 20 tool executions, retaining raw traces for any errors in the last 30 tools, with a fallback 6-step window when user-turns=0).
7. **Selective Ephemeral Deduplication**: Only prune `/shake` and `HOOK_NOTICE`/`ANCHOR_NOTICE` ephemeral messages; third-party ephemeral notifications must remain intact.
8. **Permission Hardening**: Sensitive files (`.shake_in_progress`, `transcript_full.jsonl`, `shake_hook.log`, `shake_metadata.json`, backups) must be created with restricted user-only permissions (`0600` on Unix).
9. **Fail-Open Hook**: The `--hook` command must exit with code `0` and output `{}` upon any unexpected error or lock contention (non-blocking lock + 2.5s watchdog) so it never blocks the user's chat.
10. **Untouched Full Stream**: Never prune `transcript_full.jsonl` on disk. It serves as the developer's permanent unpruned audit log.

---

## ⚡ Self-Verification & Uninstallation
After installing, run:
- `<Global Binary Directory>/shake-prune --version` (Prints `shake-prune 0.2.0`).
- `<Global Binary Directory>/shake-prune --help` (Exit code must be `0`).
- `<Global Binary Directory>/shake-prune doctor` (Verifies environment and config health).
- Confirm `<Global Skill Directory>/SKILL.md` exists and is non-empty.

To completely uninstall:
- Linux/macOS: `./install.sh --uninstall`
- Windows: `powershell -File .\install.ps1 -Uninstall`
