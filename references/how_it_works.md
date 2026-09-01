# 🧠 Deep Dive: How `/shake` Works, Inode-Preserving Compaction & Context Mechanics

This document provides a comprehensive, technical explanation of what happens under the hood when `/shake` is executed, how `transcript.jsonl` is physically compacted on disk without breaking open file handles, and how the AI interacts with the compacted history.

---

## ⚡ Physical In-Place Compaction in the Same Chat Window

When `/shake` runs, it executes **Safe Inode-Preserving In-Place JSONL Compaction** directly on the active session's `transcript.jsonl`:

```
                                  BEFORE /shake
         ┌─────────────────────────────────────────────────────────────┐
         │ transcript.jsonl (2.7 MB on disk, Inode: 4698)              │
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
         │ transcript.jsonl.bak_20260901_235144 (Timestamped backup)   │
         │ transcript.jsonl (380 KB on disk, Inode: 4698 [SAME INODE]) │
         │  • Turn 1: User prompt (100% verbatim)                      │
         │  • Turn 2: RUN_COMMAND (exit 0: "Command completed...")     │ ◄── COMPACT
         │  • Turn 3: VIEW_FILE ("File inspected...")                  │ ◄── COMPACT
         │  • Turn 4: Assistant thought & decision (100% verbatim)     │
         │  • Turn N-5..N: Active Working Window (100% full logs)      │
         └─────────────────────────────────────────────────────────────┘
                                        │
                                        ▼
                       Next Turn in the EXACT SAME TAB
         Antigravity's active file descriptor continues writing seamlessly.
         Model API payload physically drops from 2.7 MB ➔ 380 KB (85%+ saved)!
```

---

## 🛡️ The "Inode Swap" Solution & File Descriptor Safety

### Why Atomic Rename Fails on Active Logs (POSIX Inode Traps)
In Unix-like systems (Linux & macOS), when a process like Antigravity opens a file for appending (`O_WRONLY | O_APPEND`), the operating system binds the file descriptor to the **filesystem Inode**, not the file path string.
* If a tool uses `fs::rename` or `os.replace` to swap a `.tmp` file over the original, the directory entry points to a new Inode, but the IDE’s open file descriptor remains attached to the *unlinked old Inode*.
* **The Result**: Subsequent turns written by the IDE would be appended to the unlinked file, causing silent data loss on disk.

### How `/shake` Preserves the Inode (Truncate-and-Rewrite)
1. **Timestamped Backup**: Copies the file to `transcript.jsonl.bak_YYYYMMDD_HHMMSS` before any modification.
2. **Open Existing Inode in Read+Write Mode**: Opens the active file path with read and write permissions.
3. **In-Memory Transformation**: Compiles the noise-reduced stream.
4. **In-Place Truncation (`file.set_len(0)` / `truncate(0)`)**: Truncates the file to 0 bytes and rewinds to offset 0 on the **exact same open file descriptor**.
5. **Flush & Sync**: Writes the compacted JSONL lines and flushes to disk.

👉 **The Inode number never changes**. Antigravity IDE continues appending subsequent turns to the compacted file with zero desync or data loss.

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

* **Direct In-Place Ingestion**: The compacted `transcript.jsonl` contains the full conversation history with noise removed.
* **Direct Markdown Injection**: If referencing `@shake_topic.md` in a fresh window, the entire file is injected verbatim.
* **Guaranteed Context Fit**: A pruned session is **15,000 – 40,000 tokens**, fitting easily into Gemini’s **1M+ token window** with 100% full-text visibility.
