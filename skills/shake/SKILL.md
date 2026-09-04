---
name: shake
description: Deterministically compacts and tree-shakes conversation context by stripping verbose tool outputs, cargo/compiler dumps, and file reads while preserving 100% of User prompts, Assistant reasoning, Thoughts, active working memory (last 10 user turns and 20 tool runs), and the last 30 tool error traces. Supports adaptive deep compaction on marathon threads (>30 user turns). Trigger when context is full, prompt is laggy, before complex tasks, or to prune bloat.
---

# Antigravity Context Tree-Shaking Skill (`/shake`)

Compact conversation context in-place, freeing **50%–80% of transmitted tokens** while preserving 100% of user prompts, assistant reasoning, thoughts, error tracebacks, and the **last 10 user conversational turns** in active working memory.

---

## ⚡ Execution Workflow

### 1. Identify Conversation Context Paths
From your runtime context:
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

Run the native binary using `run_command`:

```bash
# Direct in-place compaction with adaptive mode and default shake_latest.md artifact
~/.gemini/bin/shake-prune run "<appDataDir>/brain/<conversation-id>/.system_generated/logs/transcript.jsonl"
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
| `shake-prune run <transcript>` | Execute adaptive context compaction (default artifact: `shake_latest.md`) |
| `shake-prune preview <transcript>` | Read-only impact simulation (estimated tokens, reduction %, continuity anchor) |
| `shake-prune status <transcript>` | Inspect size, token estimate, turn counts, health recommendation, and archives |
| `shake-prune undo <transcript>` | Restore previous state from verified atomic backup (`transcript.jsonl.bak`) |
| `shake-prune show <transcript> --step N` | Inspect archived tool execution from permanent `transcript_full.jsonl` |
| `shake-prune doctor` | Inspect environment health, config file, and hook registration |
