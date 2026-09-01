# 🛠️ How `/shake` & `/full-shake` Work: Technical Reference Manual

`shake` is a deterministic, multi-platform context tree-shaker and compaction engine purpose-built for Google Antigravity. It physically prunes historical tool execution bloat and verbose terminal noise from active conversation logs on disk while preserving 100% of User prompts, Assistant reasoning, Thoughts, and Error stack traces.

---

## 🏗️ Core Architecture & Pipeline

```mermaid
graph TD
    A[Raw transcript.jsonl & transcript_full.jsonl] --> B[Acquire Exclusive File Lock (fs2)]
    B --> C[Create Non-Destructive Timestamped Backup under Lock]
    C --> D[Pass 1: Line, Step & Assistant Turn Indexing]
    D --> E[Identify Active Working Window (Last 6 Steps)]
    E --> F[Pass 2: In-Memory JSONL Compaction]
    
    subgraph Signal Preservation
        F --> G1[100% User Prompts Verbatim]
        F --> G2[100% Assistant Final Responses & Explanations]
        F --> G3[100% Error Stack Traces & Non-Zero Exits]
        F --> G4[100% Recent Tool Outputs in Active Window]
    end
    
    subgraph Pruning & Structured Receipts
        F --> H1[RUN_COMMAND >250 chars -> Action Receipt]
        F --> H2[VIEW_FILE >500 lines -> Action Receipt]
        F --> H3[write_to_file / replace_file -> Progressive Backlink]
        F --> H4[/full-shake: Drop thoughts older than 20 turns]
    end
    
    F --> I[In-Place Truncate(0) & Rewrite (Inode Preserved)]
    I --> J[Physical fsync Commitment (sync_all)]
    J --> K[Unlock File Handle (fs2)]
    K --> L[Generate Interactive Markdown Artifact with Timeline]
    L --> M[Update active_shake_anchor.json with NamedTempFile]
```

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

### 2. Standard `/shake` vs. Deep `/full-shake`

| Feature | Standard `/shake` | Deep `/full-shake` |
| :--- | :--- | :--- |
| **Tool Execution Bloat** | Pruned to structured receipts | Pruned to structured receipts |
| **User Prompts** | 100% Verbatim | 100% Verbatim |
| **Assistant Final Answers** | 100% Verbatim | 100% Verbatim |
| **Errors & Traces** | 100% Verbatim | 100% Verbatim |
| **Scratchpad Thoughts (`thinking`)** | **100% Verbatim (All turns)** | **Retains last 20 turns**; drops older thoughts |
| **Automatic Fallback** | — | Automatically acts as natural shake if session has $\le 20$ turns |
| **Additional Savings** | — | Extra **~400 KB – 500 KB (~120k – 150k tokens)** |

### 3. The Active Working Window (Last 6 Steps)
To ensure the agent never loses context on immediate tasks:
- The **last 6 steps** of tool outputs (commands, file inspections, search queries) are **never pruned**.
- The LLM has full verbatim visibility into recent files and build results.
- Compaction only affects historical turns older than 6 steps where decisions have already been finalized.

### 4. Two-Tier Metric Scopes & Interactive Timeline
Reports clearly separate:
* **This Compaction Pass (`transcript.jsonl`)**: Exact bytes reduced on disk for this turn.
* **Cumulative Session Pruning (vs Full Stream)**: Total bloat pruned across the lifetime of the session compared to `transcript_full.jsonl`.
* **Exportable Summary Artifact (`.md`)**: Compressed artifact file size.
* **Session Compaction Timeline (`<details>`)**: Interactive dropdown tracking all prior compactions with timestamps, trigger types, and archive links.

### 5. Proactive 200k Token Auto-Compaction Hook
The native `PreInvocation` hook (`shake-prune --hook`) automatically protects conversations from token rot:
* **Calibrated Density**: Calibrated to **3.3 bytes/token** for Code/JSON transcripts.
* **Threshold Trigger**: Triggers auto-compaction when `transcript.jsonl` exceeds **660 KB (~200,000 tokens)**.
* **Growth Delta Guard (50 KB)**: Compares against `last_compacted_bytes`. If less than 50 KB of new logs have accumulated, the hook executes in `<0.2ms` without running disk compaction.
* **180s Cooldown**: Prevents CPU tight-loops in edge-case or failure scenarios.

### 6. Security & Safety Hardening
* **Canonical Storage Allowlist**: Validates that paths are within the user's canonical `~/.gemini` storage to prevent Context Poisoning from untrusted repositories.
* **Strict Output Path Allowlist**: Enforces that Markdown artifacts can only be written to session, workspace, or `~/.gemini` directories.
* **Lock-Before-Backup Concurrency**: Acquires `fs2` exclusive lock before creating backups, eliminating torn-write corruptions.
* **Markdown & XSS Sanitization**: Escapes HTML entities (`<` ➔ `&lt;`, `>` ➔ `&gt;`) and backticks (`` ``` `` ➔ `` ` ` ` ``).
* **URL Encoding**: URL-encodes `file://` links to prevent broken links with spaces.
* **Atomic Tempfiles**: Uses `tempfile::Builder` for exclusive 0600 permissions and cryptographically random filenames.
* **ReDoS Immunity**: Linear $O(N)$ tag scanning instead of recursive regex matching.
* **True Fail-Open Guarantee**: `panic::catch_unwind` with unwind panic strategy ensures `{}` fallback output on any internal error.

---

## 📊 Token Calibration Benchmarks

| Metric | Naive Prose Assumption | Empirical Calibrated Syntax Ratio |
| :--- | :--- | :--- |
| **Chars / Token** | `4.0 chars/token` | **`3.3 chars/token`** |
| **Punctuation & Symbols** | ~2% | **20.7%** (braces, quotes, colons, slashes) |
| **Whitespace & Formatting** | ~15% | **9.0%** |
| **Words & Identifiers** | ~83% | **70.4%** |
| **200k Token Threshold** | 800 KB | **660 KB (645 KB)** |
