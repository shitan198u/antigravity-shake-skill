---
name: shake
description: Deterministically compacts and tree-shakes conversation context by stripping verbose tool outputs and file dumps while preserving 100% of User prompts, Assistant reasoning, and Thoughts. Supports seamless in-window continuation.
---

# `/shake` — Context Compaction & Verbatim Pruning

Compacts, prunes, and tree-shakes conversation context to eliminate token bloat while preserving 100% of task state, user instructions, assistant reasoning, and execution errors.

## Execution Workflow (Zero Guesswork)

When `/shake` is invoked:

1. **Resolve Paths Directly (Do Not Search/Probe)**:
   - **Transcript Path**: `<appDataDir>/brain/<conversation-id>/.system_generated/logs/transcript.jsonl`
   - **Output Directory**: `<appDataDir>/brain/<conversation-id>/`
   *(Example: `/home/shsrra/.gemini/antigravity-ide/brain/d41b14b6-48ee-4154-8fbc-3b852c330e5f/`)*

2. **Execute the Native Pruner Binary**:
   Run the native binary directly at its installed location:
   ```bash
   ~/.gemini/bin/shake-prune "<appDataDir>/brain/<conversation-id>/.system_generated/logs/transcript.jsonl" "<appDataDir>/brain/<conversation-id>/"
   ```
   *(Alternative binary path: `~/.gemini/config/skills/shake/bin/shake-prune`)*
   *(Do NOT run `which shake-prune` or search with `find`)*

   *(Only if the binary file does not exist on disk, fall back to:)*
   ```bash
   python3 ~/.gemini/config/skills/shake/scripts/shake_prune.py "<appDataDir>/brain/<conversation-id>/.system_generated/logs/transcript.jsonl" "<appDataDir>/brain/<conversation-id>/"
   ```

3. **Present Output Directly**:
   Output the exact stdout returned by `shake-prune` to the user. It already contains:
   - 📊 Token Reduction Metrics Table
   - 🟢 In-Window Continuity Confirmation
   - 📄 Clickable Interactive Artifact Link
   - 📋 Collapsible External Export Drawer

4. **Continue in the Same Chat**:
   Do NOT ask the user to open a new tab. The native `PreInvocation` hook (`shake-prune --hook`) is already active and will automatically anchor future turns to the clean shaken artifact.
