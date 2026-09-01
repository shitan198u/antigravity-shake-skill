---
name: shake
description: Compacts and prunes conversation context by deterministically stripping verbose tool outputs, file views, and command stdout while preserving 100% of User prompts, Assistant reasoning, and Thoughts verbatim. Supports seamless in-window continuation via PreInvocation hook.
---

# `/shake` — Context Compaction & Verbatim Pruning

Compacts, prunes, and tree-shakes conversation context to reduce token bloat while preserving essential task state, decisions, modified files, and next actions. Use this skill whenever the user invokes `/shake` or asks to prune, compact, or refresh the agent's context and working memory.

## Instructions

When the user types `/shake` or asks to prune/compact context:

1. **Locate Current Transcript**:
   Find the active transcript file at `<appDataDir>/brain/<conversation-id>/.system_generated/logs/transcript.jsonl`.

2. **Execute High-Speed Pruning**:
   Execute the native Rust pruner (or Python fallback) to prune the transcript:
   ```bash
   shake-prune "<appDataDir>/brain/<conversation-id>/.system_generated/logs/transcript.jsonl" "<appDataDir>/brain/<conversation-id>/"
   ```
   *(Fallback if native binary is not present: `python3 ~/.gemini/config/skills/shake/scripts/shake_prune.py ...`)*

3. **Present Summary & In-Window Continuity**:
   Display the pruning stats report to the user:
   - Topic detected and timestamped filename generated.
   - Original bytes/tokens vs pruned bytes/tokens and % reduction.
   - Preserved signals count (100% user prompts, assistant turns, thoughts, error traces).
   - In-Window Continuity indicator (`🟢 ACTIVE` via `PreInvocation` hook).
   - Clickable file links and quick-copy command blocks.

4. **Seamless In-Window Continuation**:
   The user can immediately continue typing their next prompt in the **exact same chat window**! The `PreInvocation` hook automatically pins future reasoning to the clean shaken artifact without requiring a tab switch.
