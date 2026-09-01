# 🧠 Deep Dive: How `/shake` Works, In-Place Compaction & Context Mechanics

This document provides a comprehensive, technical explanation of what happens under the hood when `/shake` is executed, how `transcript.jsonl` is physically compacted on disk, and how the AI interacts with the compacted history.

---

## ⚡ Physical In-Place Compaction in the Same Chat Window

When `/shake` runs, it executes **Safe Physical In-Place JSONL Compaction** directly on the active session's `transcript.jsonl`:

```
                                  BEFORE /shake
         ┌─────────────────────────────────────────────────────────────┐
         │ transcript.jsonl (2.7 MB on disk)                           │
         │  • Turn 1: User prompt                                      │
         │  • Turn 2: RUN_COMMAND (5,000 lines npm/cmake stdout)       │ ◄── BLOAT
         │  • Turn 3: VIEW_FILE (2,000 lines source code)             │ ◄── BLOAT
         │  • Turn 4: Assistant thought & decision                     │
         └─────────────────────────────────────────────────────────────┘
                                        │
                                        ▼
                                  Run "/shake"
                                        │
                                        ▼
                                  AFTER /shake
         ┌─────────────────────────────────────────────────────────────┐
         │ transcript.jsonl.bak (Full raw backup created)              │
         │ transcript.jsonl (380 KB on disk — JSONL structure valid)   │
         │  • Turn 1: User prompt (100% verbatim)                      │
         │  • Turn 2: RUN_COMMAND (exit 0: "Command completed...")     │ ◄── COMPACT
         │  • Turn 3: VIEW_FILE ("File inspected...")                  │ ◄── COMPACT
         │  • Turn 4: Assistant thought & decision (100% verbatim)     │
         │  • Turn N-5..N: Active Working Window (100% full logs)      │
         └─────────────────────────────────────────────────────────────┘
                                        │
                                        ▼
                       Next Turn in the EXACT SAME TAB
         Antigravity reads the compacted transcript.jsonl from disk.
         Model API payload physically drops from 2.7 MB ➔ 380 KB (85%+ saved)!
```

---

## 🛡️ Why In-Place Compaction Is 100% Safe

1. **Strict Schema Preservation**:
   - Every line in `transcript.jsonl` remains a valid JSON object matching Antigravity's exact schema (`step_index`, `type`, `content`, `status`, `thinking`, `tool_calls`).
   - The sequence of steps and line indices remain completely intact.
2. **Automatic Raw Backup (`transcript.jsonl.bak`)**:
   - Before any bytes are modified, the full, unpruned original is copied to `transcript.jsonl.bak`.
3. **Atomic File Swapping**:
   - Compaction writes to `transcript.jsonl.tmp` and uses atomic filesystem rename (`os.replace` / `fs::rename`), eliminating any risk of partial writes or corruption.
4. **Zero Tab Switching Required**:
   - You stay in the same window. Because the IDE reads `transcript.jsonl` from disk on each invocation, your next prompt transmits **80%–90% fewer tokens over the wire**.

---

## 📊 What Is Preserved vs. What Is Compacted

| Conversation Element | Treatment by `/shake` | AI Visibility & Precision |
| :--- | :--- | :--- |
| **User Prompts** | Retained 100% verbatim | Complete, character-for-character precision across all turns |
| **Assistant Explanations** | Retained 100% verbatim | Architectural decisions, bug analyses, and notes are preserved word-for-word |
| **Model Thoughts** | Retained in `<details>` drawers & JSON | Deep reasoning chains (`thinking`) are preserved without loss |
| **Execution Errors & Stack Traces** | Retained 100% with full traceback | Any failed command (`exit_code != 0`), build error, or exception is preserved for debugging |
| **Active Working Window** | Last 6 tool steps retained in full | Immediate momentum and active command outputs remain intact |
| **Old Successful Tool Dumps** | Replaced with compact action receipts | `npm run build` (1,000 lines) becomes `ℹ️ [Command completed successfully (exit 0)]` |

---

## 🔍 Is RAG (Retrieval-Augmented Generation) Happening?

> **No. There is zero vector chunking, lossy embedding search, or top-k retrieval.**

### Why Developers Worry About "Passing `.md` Files"
In generic LLM tools, attaching large documents triggers vector chunking where 90% of the document is left out.

### How Antigravity Handles `/shake` Transcripts
1. **Direct In-Place Ingestion**: The compacted `transcript.jsonl` contains the full conversation history with noise removed.
2. **Direct Markdown Injection**: If referencing `@shake_topic.md` in a fresh window, the entire file is injected verbatim.
3. **Guaranteed Context Fit**: A pruned session is **15,000 – 40,000 tokens**, fitting easily into Gemini’s **1M+ token window** with 100% full-text visibility.
