# 🛠️ How `/shake` & `/full-shake` Work: Technical Reference Manual

`shake` is a deterministic, multi-platform context tree-shaker and compaction engine purpose-built for Google Antigravity. It physically prunes historical tool execution bloat and verbose terminal noise from active conversation logs on disk while preserving 100% of User prompts, Assistant reasoning, Thoughts, and Error stack traces.

---

## 🏗️ Core Architecture & Pipeline

```mermaid
graph TD
    A[transcript.jsonl (Active Model Context)] --> B[Acquire Exclusive File Lock (fs2)]
    B --> C[Check & Recover .shake_in_progress Intent Marker if present]
    C --> D[Record SnapshotFingerprint (size + mtime_nanos)]
    D --> E[Sync Active Steps into Master transcript_full.jsonl]
    E --> F[Index Master transcript_full.jsonl -> Step-to-Line Map]
    
    subgraph Stream Processing & Privacy
        F --> G1[10-Turn Human Conversational Working Window: 100% Unpruned]
        F --> G2[Selective Ephemeral Deduplication: Keep Latest Shake Only]
        F --> G3[Historical RUN_COMMAND/VIEW_FILE: Convert to line=N Receipts]
        F --> G4[Historical Heredocs: cat EOF > 250 chars -> Line Receipt]
        F --> G5[Secret Redaction Filter: API Keys, Bearer Tokens, RSA Keys]
        F --> G6[/full-shake: Window Thoughts to 20 Turns + Milestone Horizon]
    end
    
    G1 --> H[Stage Pruned Buffer & Validate JSON Integrity]
    G2 --> H
    G3 --> H
    G4 --> H
    G5 --> H
    G6 --> H

    H --> I[Pre-Commit Change Detection: Verify Snapshot Fingerprint]
    I --> J[Write Intent Journal: .shake_in_progress (0600 Mode)]
    J --> K[Atomic Backup: transcript.jsonl.bak (0600 Mode)]
    K --> L[In-Place Truncate(0) & Rewrite (Inode Preserved)]
    L --> M[Physical fsync Commitment (sync_all)]
    M --> N[Remove Intent Journal (.shake_in_progress)]
    N --> O[Unlock File Handle (fs2)]
    O --> P[Generate Interactive Markdown Artifact with Timeline]
    P --> Q[Update active_shake_anchor.json with Atomic Rename]
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

### 3. Dual-Boundary Working Memory Engine (`recent_user_turns: 10`, `recent_tools_cap: 20`)
* **The Autonomous Loop Nuance**: In iterative chat, bounding context by the last 10 user turns is ideal. But when a user gives a long autonomous task (e.g. debugging a suite of 20 tests), the agent can run 40+ commands in a *single* user turn, accumulating massive terminal stdout bloat before the user turn counter ever increments.
* **The Dual Boundary**: Working memory is strictly bounded by:
  $$\text{Active Memory} = \text{Last 10 User Turns} \cap \text{Last 20 Tool Runs}$$
  * If the last 10 user turns contain only 12 tools total, all 12 tools remain 100% unpruned.
  * If an autonomous loop runs 35 tools in 1 turn, the **most recent 20 tool outputs remain 100% raw and unpruned** (preventing amnesia on recent compiler outputs or test diffs), while the earlier 15 tools from that same turn are converted to $O(1)$ line-indexed receipts.

---

### 4. Dual-Trigger Proactive Auto-Hook
* Evaluates both file size and tool execution frequency on every agent turn:
  1. **Token Size Ceiling**: File size reaches **`264,000` bytes (~80,000 tokens)**, OR
  2. **Autonomous Tool Burst**: The transcript accumulates **$\ge 20$ unpruned tool execution outputs**, even within a single user prompt.
* Proactively compacts the prompt payload before it ever approaches Antigravity's platform-level checkpoint ceiling (~150k–200k tokens), **completely preventing lossy server-side `{{ CHECKPOINT }}` truncation and Turn 1 amnesia**.
* Protected by a **25 KB Growth Delta Guard** and **180s Cooldown** (3 minutes) to guarantee zero disk thrashing.

---

### 5. 30-Call Un-Clamped Error Retention Window (`recent_errors_cap: 30`)
* **Full Fidelity for Active Debugging**: Any command or tool execution that fails (`exit != 0` or status `failed`) occurring within the **last 30 tool calls** is preserved **100% full, raw, and un-clamped**. Stack traces, `journalctl` system logs, and multi-file compiler errors are never truncated, giving the model complete ground truth for active troubleshooting.
* **Ancient Error Compaction**: Once a failure is older than 30 tool calls (solved history), it is converted into a line-indexed receipt:
  ```text
  [PRUNED tool=RUN_COMMAND step=14 exit=1 lines=150 archive=.../transcript_full.jsonl line=48]
  ```
* Solves the unbounded memory leak where ancient stack traces from 80 steps ago would stay in `transcript.jsonl` forever, while preserving direct $O(1)$ retrievability from `transcript_full.jsonl`.

---

### 6. Line-Indexed Receipts ($O(1)$ Direct Lookup)
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

---

### 8. Master Archive Pre-Syncing (P0-1 Data Guarantee)
* **The Vulnerability**: If `transcript_full.jsonl` was missing active conversation turns, line receipts (`archive=... line=N`) could point to non-existent lines or trigger dangling references.
* **The Solution**: Under an exclusive file lock, `shake-prune` executes `sync_master_full_transcript`:
  * Parses existing step indices in `transcript_full.jsonl`.
  * Identifies any steps present in the active transcript that are missing from the master archive.
  * Appends missing steps to `transcript_full.jsonl` and executes physical `fsync`.
  * Builds the master step-to-line index from the complete master stream.
  * Fails closed: If any step cannot be resolved to an exact line in `transcript_full.jsonl`, pruning is aborted immediately without touching `transcript.jsonl`.

---

### 9. Crash Recovery via Intent Journaling & Pre-Commit Fingerprinting
* **Intent Marker (`.shake_in_progress`)**:
  * Before in-place truncation, an intent marker containing target path, timestamp, PID, and backup path is written with `0600` permissions.
  * If the process crashes mid-rewrite (e.g. power loss or SIGKILL), any subsequent invocation (CLI or `--hook`) detects `.shake_in_progress`, recovers from the backup, fsyncs, and removes the marker before proceeding.
* **Pre-Commit Snapshot Fingerprint**:
  * Prior to staging, a `SnapshotFingerprint` records transcript file length and nanosecond modification time (`mtime_nanos`).
  * Right before `truncate(0)`, the fingerprint is re-verified.
  * If an uncooperative external process wrote to the file concurrently, `shake-prune` aborts with an error, preserving active data without data loss.

---

### 10. Privacy, Redaction & Permission Hardening
* **Secret Redaction**:
  * When enabled via `--redact-secrets` or `[privacy] redact_secrets = true` in `shake.toml`, all prompts, tool outputs, and generated reports are sanitized against patterns including:
    * GitHub tokens (`ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_`)
    * AWS access keys (`AKIA[0-9A-Z]{16}`)
    * Bearer tokens (`Bearer [a-zA-Z0-9_\-\.]{16,}`)
    * RSA / OpenSSH private keys (`BEGIN ... PRIVATE KEY`)
    * Generic API keys (`api[_-]?key = ...`)
    * HTTP Authorization headers (`Authorization: ...`)
* **POSIX 0600 User-Only Permissions**:
  * All sensitive runtime artifacts—`.shake_in_progress`, `transcript_full.jsonl`, `shake_hook.log`, `shake_metadata.json`, and backups—are created with `0600` mode (`-rw-------`) on Unix systems to prevent disclosure in multi-user environments.

---

### 11. Configuration Subsystem (`shake.toml` & Environment Overrides)
Users and organizations can customize retention policies and auto-shake parameters via `~/.gemini/config/shake.toml`:

```toml
[auto]
enabled = true                  # Set to false to disable auto-shake hook completely
token_threshold_bytes = 264000  # Trigger auto-shake at ~80k tokens
tool_burst_threshold = 20       # Autonomous tool burst trigger
cooldown_seconds = 180          # Minimum seconds between auto-compaction
growth_delta_bytes = 25600      # Minimum transcript byte growth

[retention]
recent_user_turns = 10          # Human conversational turns retained verbatim
recent_tools_cap = 20           # Maximum active tool outputs retained raw
recent_errors_cap = 30          # Un-clamped error traces retention window

[privacy]
redact_secrets = false          # Automatic credential and secret redaction

[diagnostics]
log_level = "info"
```

All TOML keys support full 12-factor override via environment variables (`SHAKE_AUTO_DISABLE`, `SHAKE_RECENT_USER_TURNS`, `SHAKE_TOOLS_CAP`, `SHAKE_ERRORS_CAP`, `SHAKE_TOKEN_THRESHOLD_BYTES`, `SHAKE_TOOL_BURST_THRESHOLD`, `SHAKE_COOLDOWN_SECONDS`, `SHAKE_GROWTH_DELTA_BYTES`, `SHAKE_SECRET_REDACTION`).

---

### 12. Hardened Restore Subcommand with `.pre_restore` Snapshot
* Running `shake-prune restore` requires an exclusive lock on the active transcript.
* Validates that the backup is readable and non-empty.
* Automatically creates a safety snapshot `transcript.jsonl.pre_restore` before overwriting.
* Applies `0600` permissions and removes stale intent markers upon completion.

