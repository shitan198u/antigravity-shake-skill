---
name: shake
description: Deterministically compacts and tree-shakes conversation context by stripping verbose tool outputs, cargo/compiler dumps, and file reads while preserving 100% of User prompts, Assistant reasoning, and Thoughts. Trigger when context is full, prompt is laggy, before complex tasks, or to prune bloat.
---

# Antigravity Context Tree-Shaking Skill (`/shake`)

Compact conversation context in-place, freeing **50%–80% of transmitted tokens** while preserving 100% of user prompts, assistant reasoning, thoughts, error tracebacks, and the active working state.

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
# Direct in-place compaction and interactive artifact generation
~/.gemini/bin/shake-prune "<appDataDir>/brain/<conversation-id>/.system_generated/logs/transcript.jsonl" "<appDataDir>/brain/<conversation-id>/"
```

---

### 3. Display the Report & Continue
The binary outputs a formatted markdown summary. Present it directly to the user:
- Report the **tokens saved** and **physical reduction percentage**.
- The `PreInvocation` hook pins the assistant's working focus to the clean state.
- **The user can immediately continue typing prompts in the exact same chat window.**
