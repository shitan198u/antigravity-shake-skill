---
name: shake
description: >-
  Compacts, prunes, and tree-shakes conversation context to reduce token bloat while
  preserving essential task state, decisions, modified files, and next actions.
  Use this skill whenever the user invokes `/shake` or asks to prune, compact,
  or refresh the agent's context and working memory.
---

# `/shake` Context Compaction & Verbatim Pruning Skill

The `/shake` skill eliminates context bloat caused by verbose command outputs, file views, and tool payloads while preserving **100% of the user and assistant dialogue verbatim** without lossy summarization.

---

## Execution Workflow

When `/shake` is triggered:

### Step 1: Run Native Pruner
Run the native Rust binary (or Python fallback) on the current session:

```bash
~/.gemini/bin/shake-prune \
  <appDataDir>/brain/<conversation-id>/.system_generated/logs/transcript.jsonl \
  <appDataDir>/brain/<conversation-id>/
```

*(If the native binary is ever absent, the agent automatically falls back to `python3 ~/.gemini/config/skills/shake/scripts/shake_prune.py`).*

This tool:
1. Derives a **topic-specific filename** (e.g., `shake_<topic_slug>_<timestamp>.md`).
2. Preserves **100% of User requests and Assistant responses verbatim**.
3. Retains action receipts (`[Command completed successfully]` / `[File inspected]`).
4. Preserves all **execution errors, stack traces, and non-zero exit codes**.
5. Preserves the active working window (recent tool outputs).
6. Completely strips bulky `RUN_COMMAND` stdout, `VIEW_FILE` contents, and large search payloads.
7. Automatically creates the `.metadata.json` for **Interactive IDE Artifact** UI integration.
8. Computes and outputs exact token reduction metrics and full copyable absolute paths.

### Step 2: Check Repository Status
```bash
git status --short
```

### Step 3: Present Token Reduction Report & Quick-Copy Block
Present the user with a clean summary table and the dedicated **Quick-Copy Block**:
- **Topic & Session ID**
- **Token Reduction Stats** (Original vs Pruned, % saved)
- **Interactive Artifact Link**: Clickable link to open the artifact directly in the IDE
- **In-Chat Mention Syntax**: `@/absolute/path/to/shake_...md`
- **Copy Commands**: One-liner CLI commands to copy to project or clipboard
