---
name: full-shake
description: Deeply compacts conversation context in-place by pruning tool outputs and retaining scratchpad thoughts for the last 20 turns only.
---

# ⚡ /full-shake: Deep Context Compaction (Thought Windowing)

Run this skill when you want **Deep Context Compaction** for long-running sessions.
* **Prunes 100% of raw tool outputs and terminal dumps** (like `/shake`).
* **Preserves 100% of User prompts, Assistant final explanations, Decisions, and Error traces verbatim**.
* **Retains scratchpad thoughts for the last 20 turns** while dropping older thoughts to save an additional **~400 KB – 500 KB (~120k – 150k tokens)**.
* **Natural Shake Fallback**: If the session has $\le 20$ turns, it automatically retains all thoughts (behaving as standard `/shake`).

---

## 🚀 Execution Instructions

When the user invokes `/full-shake`:

1. Locate the active session's `transcript.jsonl` log file:
   - Primary path: `<appDataDir>/brain/<conversation-id>/.system_generated/logs/transcript.jsonl`
   - Artifact directory: `<appDataDir>/brain/<conversation-id>/`

2. Execute the native compiled binary with `--full`:
   ```bash
   ~/.gemini/bin/shake-prune "<transcript_path>" "<artifact_directory>" --full --thought-window 20
   ```

3. Present the formatted output report directly to the user.
