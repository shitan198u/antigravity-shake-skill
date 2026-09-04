---
name: shake
description: Deterministically compacts and tree-shakes conversation context by stripping verbose tool outputs, cargo/compiler dumps, and file reads while preserving 100% of User prompts, Assistant reasoning, Thoughts, active working memory (last 10 user turns and 20 tool runs), and the last 30 tool error traces. Supports adaptive deep compaction on marathon threads (>30 user turns). Trigger when context is full, prompt is laggy, before complex tasks, or to prune bloat.
---

# Antigravity Context Tree-Shaking Skill (`/shake`)

Compact conversation context in-place, freeing **50%–80% of transmitted tokens** while preserving 100% of user prompts, assistant reasoning, thoughts, error tracebacks, and the **last 10 user conversational turns** in active working memory.

---

## ⚡ Execution Workflow

### 1. Identify Conversation Context Paths
From your runtime context (`ANTIGRAVITY_APP_DATA_DIR` when set, else the app data dir):
- **Conversation ID**: `<conversation-id>`
- **Transcript Path**:
  ```text
  <appDataDir>/brain/<conversation-id>/.system_generated/logs/transcript.jsonl
  ```
- **Artifact Directory**:
  ```text
  <appDataDir>/brain/<conversation-id>/
  ```

---

### 2. Execute High-Performance Pruner

Run the native binary using `run_command`. Quote paths with spaces:

```bash
# Linux/macOS — direct in-place compaction, adaptive mode, default shake_latest.md artifact
~/.gemini/bin/shake-prune run "<appDataDir>/brain/<conversation-id>/.system_generated/logs/transcript.jsonl"
```

```powershell
# Windows
%USERPROFILE%\.gemini\bin\shake-prune.exe run "<appDataDir>\brain\<conversation-id>\.system_generated\logs\transcript.jsonl"
```

*Pruned outputs include exact `line=N` pointers to `transcript_full.jsonl`. If you ever need to inspect historical commands or file contents from earlier turns, invoke `shake-prune show` or `view_file`.*

---

### 3. Display the Report & Continue
The binary outputs a formatted markdown summary. Present it directly to the user:
- Report the **tokens saved** and **physical reduction percentage**.
- Click the summary artifact link to inspect details.
- The `PreInvocation` hook pins the assistant's working focus with the continuity card.
- **The user can immediately continue typing prompts in the exact same chat window.**

---

### 🛠️ Daily Utility Commands

| Command | Purpose |
| :--- | :--- |
| `shake-prune run <transcript> [--mode auto\|standard\|deep] [--redact-secrets]` | Execute adaptive context compaction (default artifact: `shake_latest.md`) |
| `shake-prune preview <transcript> [--json]` | Read-only impact simulation (estimated tokens, reduction %, continuity anchor) |
| `shake-prune status <transcript> [--json]` | Inspect size, token estimate, turn counts, health recommendation, and archives |
| `shake-prune undo <transcript> [--force]` | Restore previous state from verified atomic backup (`transcript.jsonl.bak`) |
| `shake-prune show <transcript> --step N \| --line N [--pretty] [--json]` | Inspect archived tool execution from permanent `transcript_full.jsonl` |
| `shake-prune doctor [--json]` | Inspect environment health, config file, and hook registration |

Behavior notes: active memory is last 10 user turns ∩ last 20 tools with the last
30 tool errors kept raw; only `/shake` + `HOOK_NOTICE`/`ANCHOR_NOTICE` ephemeral
messages are deduplicated (third-party notices stay). Optional `~/.gemini/config/shake.toml`
plus `SHAKE_*` env overrides tune thresholds (see `references/how_it_works.md` §11).
