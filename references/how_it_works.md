# 🛠️ How `/shake` & `/full-shake` Work: Technical Reference Manual

`shake` is a deterministic, multi-platform context tree-shaker and compaction engine purpose-built for Google Antigravity. It physically prunes historical tool execution bloat and verbose terminal noise from active conversation logs on disk while preserving 100% of User prompts, Assistant reasoning, Thoughts, and Error stack traces.

---

## 🏗️ Core Architecture & Pipeline

```mermaid
graph TD
    A[transcript.jsonl (Active Model Context)] --> B[Acquire Exclusive File Lock (fs2)]
    B --> C[Create Non-Destructive Timestamped Backup under Lock]
    C --> D[Prune Older Timestamped Backups (keep_backups=5)]
    D --> E[Single-Pass Pipeline: Step & Assistant Indexing]
    
    subgraph Unified In-Memory Generation
        E --> F1[In-Place Compacted JSONL Stream]
        E --> F2[Exportable Markdown Report & Stats]
    end
    
    subgraph Signal Preservation
        F1 --> G1[100% User Prompts Verbatim]
        F1 --> G2[100% Assistant Final Responses & Explanations]
        F1 --> G3[100% Error Stack Traces & Non-Zero Exits]
        F1 --> G4[100% Recent Tool Outputs in Active Window]
    end
    
    subgraph Structured Receipt Tags
        F1 --> H1[RUN_COMMAND -> [PRUNED tool=RUN_COMMAND step=N exit=0 lines=M archive=...]]
        F1 --> H2[VIEW_FILE -> [PRUNED tool=VIEW_FILE step=N lines=M archive=...]]
        F1 --> H3[write_to_file -> [PRUNED tool=write_to_file step=N lines=M archive=...]]
        F1 --> H4[/full-shake: Drop thoughts older than 20 turns]
    end
    
    F1 --> I[In-Place Truncate(0) & Rewrite (Inode Preserved)]
    I --> J[Physical fsync Commitment (sync_all)]
    J --> K[Unlock File Handle (fs2)]
    K --> L[Generate Interactive Markdown Artifact with Timeline]
    L --> M[Update active_shake_anchor.json with NamedTempFile]
```

> [!NOTE]
> **Permanent History Isolation**: `transcript_full.jsonl` is deliberately left **100% untouched** on disk. It remains the developer's permanent unpruned debug log, allowing raw historical inspection while `transcript.jsonl` stays lean.

---

## ⚡ Key Features & Mechanical Details

### 1. Inode-Preserving Truncate-and-Rewrite
Standard atomic rename (`fs::rename`) swaps out the underlying filesystem Inode. Because the Antigravity IDE holds open file descriptors to `transcript.jsonl`, renaming the file orphans the IDE handle, causing subsequent turns to write to a deleted file.
- `/shake` opens the file with `File::options().read(true).write(true)`.
- Acquires an exclusive cross-platform file lock using `fs2::FileExt::lock_exclusive()`.
- **Creates the backup while holding the lock** to guarantee zero torn writes.
- Truncates to 0 bytes (`file.set_len(0)`), seeks to start (`file.seek(SeekFrom::Start(0))`), and writes the compacted stream.
- Calls `file.sync_all()` (`fsync`) before releasing the lock.
- **The IDE's active file descriptor continues writing to the exact same file without interruption.**

---

### 2. Unified Single-Pass Pipeline
Earlier prototypes double-walked `transcript.jsonl` (once for markdown report generation, once for in-place compaction).
- The unified pipeline buffers JSON lines once and generates both the in-place compacted JSONL buffer and the full Markdown summary in a single execution pass.
- Eliminates redundant I/O, halving CPU and memory consumption on large multi-megabyte sessions.

---

### 3. Rolling Backup Retention (`--keep-backups N`)
To prevent disk exhaustion across dozens of compactions:
- Every shake creates a timestamped archive: `transcript.jsonl.bak_<YYYYMMDD_HHMMSS>`.
- The retention engine (`prune_old_backups`) sorts existing archives chronologically, keeps the newest $N$ backups (default: 5), and removes older snapshots.
- The latest unversioned `transcript.jsonl.bak` is always preserved.

---

### 4. Structured Receipt Schema
Pruned tool execution blocks are replaced with a stable, machine-parseable receipt schema:

```text
[PRUNED tool=RUN_COMMAND step=42 exit=0 lines=120 archive=/path/to/transcript.jsonl.bak_20260902_204559]
[PRUNED tool=VIEW_FILE step=45 lines=520 archive=/path/to/transcript.jsonl.bak_20260902_204559]
[PRUNED tool=write_to_file step=50 lines=140 archive=/path/to/transcript.jsonl.bak_20260902_204559]
```

- **tool**: The exact tool name that was executed.
- **step**: The step index in the conversation stream.
- **exit**: Exit status code for command executions (`0` = success).
- **lines**: Number of lines that were pruned from context.
- **archive**: Full canonical path to the timestamped backup containing the raw output.

---

### 5. Thought Windowing (`/full-shake`)
While `/shake` retains 100% of thoughts across all turns, `/full-shake` keeps thoughts for the **last 20 assistant turns** only:
- Thoughts older than 20 turns are stripped from `PLANNER_RESPONSE` nodes.
- Dialogue, explanations, and decisions remain 100% verbatim.
- Saves an extra **~400 KB – 500 KB (~120k – 150k tokens)** on mega-threads.
- If the conversation has $\le 20$ turns, it automatically falls back to standard zero-loss retention.

---

### 6. Active Working Window (Immunity)
The last 6 tool execution steps are **100% immune** to compaction:
- Recent file reads, active terminal diffs, and immediate compiler warnings remain in active working memory so ongoing tasks are never disrupted.

---

### 7. Fail-Open PreInvocation Hook
The background lifecycle hook runs before every model dispatch:
- Monitors `transcript.jsonl` size. If $\ge 660\text{ KB}$ (~200k tokens), it triggers auto-compaction.
- Protected by a **50 KB Growth Delta Guard** and **180s Cooldown**.
- Wrapped in `panic::catch_unwind`: on any error, it safely outputs `{}` and exits 0, ensuring the IDE never stalls.
