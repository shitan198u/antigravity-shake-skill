# 🛠️ How `/shake` & `/full-shake` Work: Technical Reference Manual

`shake` is a deterministic, multi-platform context tree-shaker and compaction engine purpose-built for Google Antigravity. It physically prunes historical tool execution bloat and verbose terminal noise from active conversation logs on disk while preserving 100% of User prompts, Assistant reasoning, Thoughts, and Error stack traces.

---

## 🏗️ Core Architecture & Pipeline

```mermaid
graph TD
    A[transcript.jsonl (Active Model Context)] --> B[Acquire Exclusive File Lock (fs2)]
    B --> C[Create Single Atomic Fallback: transcript.jsonl.bak]
    C --> D[Purge Legacy Timestamped Backups (Zero Disk Bloat)]
    D --> E[Index Master transcript_full.jsonl -> Step-to-Line Map]
    
    subgraph Stream Processing
        E --> F1[10-Turn Human Conversational Working Window: 100% Unpruned]
        E --> F2[Ephemeral Notice Deduplication: Keep Latest Only]
        E --> F3[Historical RUN_COMMAND/VIEW_FILE: Convert to line=N Receipts]
        E --> F4[Historical Heredocs: cat EOF > 250 chars -> Line Receipt]
        E --> F5[/full-shake: Window Thoughts to 20 Turns + Milestone Horizon]
    end
    
    F1 --> G[In-Place Truncate(0) & Rewrite (Inode Preserved)]
    F2 --> G
    F3 --> G
    F4 --> G
    F5 --> G
    
    G --> H[Physical fsync Commitment (sync_all)]
    H --> I[Unlock File Handle (fs2)]
    I --> J[Generate Interactive Markdown Artifact with Timeline]
    J --> K[Update active_shake_anchor.json with NamedTempFile]
```

---

## ⚡ Key Features & Mechanical Details

### 1. The Single Master Archive Architecture (Zero Dangling Pointers)
Instead of creating dozens of multi-megabyte `.bak_*` files on every run (which risks broken links when older backups are rotated):
* All structured receipts point permanently to **`transcript_full.jsonl`** with exact 1-indexed line numbers:
  ```text
  [PRUNED tool=RUN_COMMAND step=42 exit=0 lines=120 archive=/path/to/transcript_full.jsonl line=184]
  ```
* Because `transcript_full.jsonl` is Antigravity's master unpruned log that is **never deleted and never truncated**, every receipt link remains **100% permanent and clickable forever**.

---

### 2. Zero Disk Bloat Policy
* We maintain only **one atomic crash-recovery file**: `transcript.jsonl.bak` (overwritten under lock right before `truncate(0)`).
* All legacy timestamped `.bak_*` files are automatically purged, saving **15 MB – 50 MB of duplicate disk bloat per conversation**.

---

### 3. 10-Turn Human Conversational Working Window (`recent_user_turns: 10`)
* Replaces fragile step-count heuristics.
* Retains all tool executions, diffs, compiler checks, and thoughts from the **last 10 back-and-forth user exchanges** completely unpruned in active working memory.
* Completely eliminates agent amnesia and redundant command re-runs across all workflows.

---

### 4. Proactive 80k Token Auto-Compaction Hook
* `AUTO_SHAKE_TOKEN_THRESHOLD_BYTES` is calibrated to **`264,000` bytes (~80,000 tokens)**.
* Proactively compacts the prompt payload before it ever approaches Antigravity's platform-level checkpoint ceiling (~150k–200k tokens), **completely preventing lossy server-side `{{ CHECKPOINT }}` truncation and Turn 1 amnesia**.
* Protected by a **50 KB Growth Delta Guard** and **180s Cooldown**.

---

### 5. Line-Indexed Receipts ($O(1)$ Direct Lookup)
* Receipts specify the exact 1-indexed line number in `transcript_full.jsonl`:
  ```text
  [PRUNED tool=write_to_file step=45 file=src/main.rs lines=80 archive=.../transcript_full.jsonl line=210]
  [PRUNED heredoc command="cat << 'EOF' > ..." lines=45 archive=.../transcript_full.jsonl line=184]
  ```
* The agent or user can inspect any historical file or command instantly in `<5ms` via:
  ```json
  view_file(AbsolutePath: ".../transcript_full.jsonl", StartLine: 210, EndLine: 210)
  ```

---

### 6. Warning Awareness in Receipts
* If an older command emitted compiler warnings, the count is reflected directly in the receipt:
  ```text
  [PRUNED tool=RUN_COMMAND step=42 exit=0 warnings=3 archive=.../transcript_full.jsonl line=184]
  ```
* The model knows warnings were emitted without carrying the raw warning noise in prompt context.

---

### 7. Enhanced Marathon `/full-shake` (Milestone Horizon)
* On sessions with $> 30$ user turns:
  * **Turn 1 (Genesis)** is preserved 100% verbatim (original guidelines and constraints).
  * **Middle Turns (Turns 2 to N-25)** are collapsed into a structured Milestone Checkpoint block with exact line-indexed backup links.
  * **Last 25 user turns** are preserved 100% verbatim.
  * **Scratchpad thoughts** are windowed to the last 20 assistant turns.
* Restores sub-second agility on threads that have been active for days.
