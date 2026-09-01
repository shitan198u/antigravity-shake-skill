# 🛠️ How `/shake` Works: Technical Reference Manual

`shake` is a deterministic, multi-platform context tree-shaker and compaction engine purpose-built for Google Antigravity. It physically prunes historical tool execution bloat from active conversation logs while preserving 100% of User prompts, Assistant reasoning, Thoughts, and Error stack traces.

---

## 🏗️ Core Architecture & Pipeline

```mermaid
graph TD
    A[Raw transcript.jsonl & transcript_full.jsonl] --> B[Exclusive File Lock (fs2)]
    B --> C[Pass 1: Line & Step Indexing]
    C --> D[Identify Active Working Window (Last 6 Steps)]
    D --> E[Pass 2: In-Memory JSONL Compaction]
    
    subgraph Signal Preservation
        E --> F1[100% User Prompts Verbatim]
        E --> F2[100% Thoughts & Reasoning Verbatim]
        E --> F3[100% Error Stack Traces & Non-Zero Exits]
        E --> F4[100% Recent Tool Outputs (Last 6 Steps)]
    end
    
    subgraph Pruning & Structured Receipts
        E --> G1[RUN_COMMAND >250 chars -> Action Receipt]
        E --> G2[VIEW_FILE >500 lines -> Action Receipt]
        E --> G3[write_to_file / replace_file -> Progressive Backlink]
    end
    
    E --> H[Create Timestamped Non-Destructive Backup]
    H --> I[In-Place Truncate(0) & Rewrite (Inode Preserved)]
    I --> J[Physical fsync commitment (sync_all)]
    J --> K[Unlock File Handle (fs2)]
    K --> L[Generate Interactive Markdown Artifact]
    L --> M[Update active_shake_anchor.json with NamedTempFile]
```

---

## ⚡ Key Features & Mechanical Details

### 1. Inode-Preserving Truncate-and-Rewrite
Standard atomic rename (`fs::rename`) swaps out the underlying filesystem Inode. Because the Antigravity IDE holds open file descriptors to `transcript.jsonl`, renaming the file orphans the IDE handle, causing subsequent turns to write to a deleted file.
- `/shake` opens the file with `File::options().read(true).write(true)`.
- Acquires an exclusive cross-platform file lock using `fs2::FileExt::lock_exclusive()`.
- Truncates to 0 bytes (`file.set_len(0)`), seeks to start (`file.seek(SeekFrom::Start(0))`), and writes the compacted stream.
- Calls `file.sync_all()` (`fsync`) before releasing the lock.
- **The IDE's active file descriptor continues writing to the exact same file without interruption.**

### 2. The Active Working Window (Last 6 Steps)
To ensure the agent never loses context on immediate tasks:
- The **last 6 steps** of tool outputs (commands, file inspections, search queries) are **never pruned**.
- The LLM has full verbatim visibility into recent files and build results.
- Compaction only affects historical turns older than 6 steps where decisions have already been finalized.

### 3. Progressive Disclosure & Canonical Backlinks
For code write actions (`write_to_file`, `replace_file_content`, `multi_replace_file_content`):
- Rather than dumping 2,000 lines of code into the prompt history, `/shake` compacts older write actions into receipts:
  ```text
  [File written to disk (140 lines). Step 42 full payload archived in /home/.../transcript.jsonl.bak_20260902_005415. Inspect via view_file if needed]
  ```
- The LLM retains knowledge of what file changed, the purpose of the change, and the exact absolute filesystem path to restore the code on-demand.

### 4. Proactive 200k Token Auto-Compaction Hook
The native `PreInvocation` hook (`shake-prune --hook`) automatically protects conversations from token rot:
- **Calibrated Density**: Calibrated to **3.3 bytes/token** for Code/JSON transcripts.
- **Threshold Trigger**: Triggers auto-compaction when `transcript.jsonl` exceeds **660 KB (~200,000 tokens)**.
- **Growth Delta Guard (50 KB)**: Compares against `last_compacted_bytes`. If less than 50 KB of new logs have accumulated, the hook executes in `<0.2ms` without running disk compaction.
- **180s Cooldown**: Prevents CPU tight-loops in edge-case or failure scenarios.

### 5. Security & Safety Hardening
- **Path Validation**: Rejects arbitrary system files (`/etc/passwd`, `/root/.ssh/`) for input and output.
- **Context Poisoning Defense**: Restricts hook discovery strictly to system-managed storage (`~/.gemini` or `/brain/`).
- **Markdown & XSS Sanitization**: Escapes HTML entities (`<` ➔ `&lt;`, `>` ➔ `&gt;`) and backticks (`` ``` `` ➔ `` ` ` ` ``).
- **URL Encoding**: URL-encodes `file://` links to prevent broken links with spaces.
- **Atomic Tempfiles**: Uses `tempfile::Builder` for exclusive 0600 permissions and cryptographically random filenames.
- **ReDoS Immunity**: Linear $O(N)$ tag scanning instead of recursive regex matching.
- **Panic Safety**: `panic::catch_unwind` ensures a rock-solid fail-open guarantee (`{}` output on any internal panic).

---

## 📊 Token Calibration Benchmarks

| Metric | Naive Prose Assumption | Empirical Calibrated Syntax Ratio |
| :--- | :--- | :--- |
| **Chars / Token** | `4.0 chars/token` | **`3.3 chars/token`** |
| **Punctuation & Symbols** | ~2% | **20.7%** (braces, quotes, colons, slashes) |
| **Whitespace & Formatting** | ~15% | **9.0%** |
| **Words & Identifiers** | ~83% | **70.4%** |
| **200k Token Threshold** | 800 KB | **660 KB (645 KB)** |
