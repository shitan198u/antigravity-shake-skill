# 🧠 Deep Dive: How `/shake` Works, Context Mechanics & AI Visibility

This document provides a comprehensive, technical explanation of what happens under the hood when `/shake` is executed, how Antigravity manages context across turns, and how the AI interacts with the compacted history.

---

## ❓ The Core Question: Does `transcript.jsonl` Become "New"?

In simple terms: **Logically yes for the AI, but safely non-destructive on disk.**

Here is the exact distinction:

1. **On Disk (`transcript.jsonl` is Append-Only)**:
   - Antigravity maintains `transcript.jsonl` as an append-only system journal.
   - `shake-prune` **never modifies or overwrites the active `transcript.jsonl` file in-place**. Mutating an active file descriptor while the IDE process is actively writing to it would cause race conditions, corrupt telemetry, or break IDE file watchers.
2. **Logically in the AI's Context Window**:
   - `shake-prune` extracts the entire history from `transcript.jsonl`, eliminates the low-signal tool payload noise (e.g. 500-line `npm` build streams, 1,000-line `view_file` dumps), and produces a clean, structured **Verbatim Artifact** (`shake_<topic>_<timestamp>.md`).
   - Through the **PreInvocation Hook** (`shake-prune --hook`), the AI receives an active boundary directive on subsequent turns, instructing it to treat the historical raw tool outputs as archived and anchor its working context in the clean state.
3. **In a Fresh Window (New Tab)**:
   - When you start a new conversation and reference `@shake_<topic>.md`, Antigravity initializes a **brand-new, completely fresh `transcript.jsonl`** where Turn 1 is the 100% complete, unbloated historical baseline.

---

## 🔍 Is RAG (Retrieval-Augmented Generation) Happening?

> **No. There is zero vector chunking, lossy embedding search, or top-k retrieval.**

### Why Developers Worry About "Passing `.md` Files"
In many generic LLM systems, attaching large documents triggers a background RAG pipeline:
- The document is sliced into arbitrary 500-token chunks.
- Vector embeddings are calculated.
- When you ask a question, only the "top 3 most similar chunks" are pulled into the prompt.
- **The Result**: The AI loses the global architecture, misses edge cases, and forgets previous agreements because 90% of the document is left out.

### How Antigravity Handles `/shake` Markdown Artifacts
Antigravity does **NOT** use lossy vector RAG for `@file` or artifact inclusions:
1. **Direct Full-Text Verbatim Injection**: The entire Markdown file is injected directly into the active prompt payload.
2. **Guaranteed Fit within Context Limit**:
   - A bloated session might reach **600,000+ tokens (2.5+ MB)**, which degrades attention.
   - The pruned `/shake` file is typically **15,000 – 40,000 tokens (30 KB – 100 KB)**.
   - Because the underlying Gemini model features a massive **1M+ token context window**, the entire pruned history fits in a small fraction of available memory with **100% full-text visibility**.

---

## 🔄 Turn-by-Turn Execution Walkthrough

```
                     TURNS 1..N: Long Coding Session
                     (2.5 MB raw logs, file dumps, status chatter)
                                    │
                                    ▼
                          User runs: "/shake"
                                    │
    ┌───────────────────────────────┴───────────────────────────────┐
    │ 1. Native Pruner (`shake-prune`) reads transcript.jsonl       │
    │ 2. Strips verbose tool stdout; preserves 100% dialogue        │
    │ 3. Writes `shake_<topic>_<timestamp>.md` in artifact dir     │
    │ 4. Writes `active_shake_anchor.json` (active: true)           │
    │ 5. Prints formatted token reduction table (~80-90% savings)  │
    └───────────────────────────────┬───────────────────────────────┘
                                    │
                                    ▼
                 TURN N+1: User types next prompt in SAME TAB
                                    │
    ┌───────────────────────────────┴───────────────────────────────┐
    │ 1. IDE triggers PreInvocation Hook (`shake-prune --hook`)     │
    │ 2. Hook reads `active_shake_anchor.json` (<0.2ms latency)     │
    │ 3. Hook injects ephemeral system instruction:                 │
    │    "[Context compacted via /shake. Active state anchored in   │
    │     @shake_topic.md (Step 220+). Treat raw stdout as archived]"│
    │ 4. LLM receives prompt with focused attention                 │
    │ 5. Agent continues working seamlessly with zero bloat         │
    └───────────────────────────────────────────────────────────────┘
```

---

## 📊 What Is Preserved vs. What Is Pruned

| Conversation Element | Treatment by `/shake` | AI Visibility & Precision |
| :--- | :--- | :--- |
| **User Prompts** | Retained 100% verbatim | Complete, character-for-character precision across all turns |
| **Assistant Explanations** | Retained 100% verbatim | Architectural decisions, bug analyses, and notes are preserved word-for-word |
| **Model Thoughts** | Retained in `<details>` drawers | Deep reasoning chains (`thinking`) are preserved without cluttering the UI |
| **Execution Errors & Stack Traces** | Retained 100% with full traceback | Any failed command (`exit_code != 0`), build error, or exception is preserved for debugging |
| **Active Working Window** | Last 6 tool steps retained in full | Immediate momentum and active command outputs remain intact |
| **Old Successful Tool Dumps** | Replaced with compact action receipts | `npm run build` (1,000 lines) becomes `ℹ️ [Command completed successfully (exit 0)]` |

---

## 🚀 Two Ways to Continue After `/shake`

### Option 1: In-Window Continuation (Default & Recommended)
- **Action**: Simply type your next message and press **Send** in the same chat tab.
- **Under the Hood**: The native `PreInvocation` hook injects the session anchor ephemerally. The model shifts its focus away from stale raw tool stdout to the active working state.

### Option 2: Fresh Tab Continuation (For Complete Reset)
- **Action**: Open a new chat tab and type:
  ```text
  @/path/to/shake_topic_YYYYMMDD_HHMM.md Continue with the next task.
  ```
- **Under the Hood**: A brand-new `transcript.jsonl` is created. Antigravity injects the full verbatim pruned transcript as the initial context, completely freeing up the context window.
