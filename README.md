# ⚡ Antigravity `/shake`

> **Deterministic, In-Place Context Tree-Shaking, Token Compaction & Utility Suite for Google Antigravity (Gemini)**  
> *Physically compact active conversation logs on disk, eliminate context degradation, and continue chatting seamlessly in the **exact same tab**.*

---

## 🌟 Why `/shake`?

During long development sessions, AI coding agents accumulate tens of thousands of lines of terminal output, compiler logs, and file inspections. This leads to **"context rot"**:
1. **Severe Latency**: Millions of raw characters are re-serialized and sent across the wire on every turn.
2. **Attention Decay**: The LLM loses track of earlier instructions under a mountain of terminal noise.
3. **Lost Momentum**: Developers are forced to open new chat tabs, re-explaining the project context and losing state.

`/shake` solves this by **physically pruning active session logs directly on disk in-place** while preserving 100% of your dialogue, reasoning, thoughts, error traces, and active working state.

---

## 🚀 Key Features in v0.2.0

* 🟢 **Same-Tab In-Place Compaction**: Modifies active session logs directly on disk without breaking open file descriptors or requiring new chat tabs.
* 🧠 **Adaptive Standard & Deep Compaction**: One safe command (`/shake`). Standard zero-loss compaction operates automatically for normal sessions; long marathon threads (>30 user turns) automatically switch to deep compaction with Milestone Horizon.
* 📋 **Clean Default Artifact (`@shake_latest.md`)**: Compactions update a single stable artifact `@shake_latest.md` by default, eliminating multi-megabyte file accumulation across repetitive shakes.
* 🛡️ **Inode Preservation & File Locking**: Uses POSIX/Windows truncate-and-rewrite with `fs2` exclusive locking, atomic crash fallback (`.bak`), and `fsync` durability.
* ⚡ **Dual-Trigger Proactive Auto-Hook**: Background `PreInvocation` and `Stop` hooks automatically detect and compact conversations whenever file size crosses **80,000 tokens** (`264 KB`) **OR** accumulates **$\ge 20$ unpruned tool runs**, completely preventing lossy server-side `{{ CHECKPOINT }}` truncation and autonomous burst bloat!
* 🏛️ **Permanent Master Archive (`transcript_full.jsonl`)**: Receipts point directly to `transcript_full.jsonl` with exact 1-indexed line numbers (`line=N`). No broken links, ever.
* 🎯 **Dual-Boundary Working Memory Engine**: Working memory retains human conversational context (last **10 user turns**) and autonomous tool volume (capped at last **20 tool outputs**).
* 🛡️ **30-Call Un-Clamped Error Window**: Preserves all failures (`exit != 0` or status `failed`) occurring in the last **30 tool calls** 100% raw, full, and un-clamped.
* 🛠️ **Unified CLI Utility Suite**: Five high-utility subcommands (`run`, `preview`, `status`, `undo`, `show`) for daily workflow management.
* 🦀 **Pure Native Rust**: High-performance, memory-safe, multi-platform binaries for Linux (x86_64, aarch64), macOS (Apple Silicon & Intel), and Windows (x86_64).

---

## 📊 Physical Token Reduction Metrics (example run, not a guarantee)

| Metric | Original Master Stream (`transcript_full.jsonl`) | Standard Mode (`<= 30 turns`) | Deep Mode (`> 30 turns`) |
| :--- | :---: | :---: | :---: |
| **Payload Size on Disk** | `3.35 MB` | `750 KB` | **`455 KB`** |
| **Estimated Token Load** | `~990,000 tokens` | `~220,000 tokens` | **`~138,000 tokens`** |
| **Cumulative Savings** | *Baseline* | **`77.4% pruned`** | **`86.0% pruned`** |
| **Active Working Window**| Unpruned | **Last 10 User Turns ∩ Last 20 Tools** | **Last 10 User Turns ∩ Last 20 Tools** |
| **Error Retention** | All | **Last 30 Calls Verbatim (Un-clamped)** | **Last 30 Calls Verbatim (Un-clamped)** |
| **Scratchpad Thoughts** | All | **100% Preserved** | **Last 20 Assistant Turns Only** |
| **Milestone Horizon** | None | Verbatim Dialogue | **Turn 1 Genesis + Last 25 Turns** |

Figures above are from a single representative fixture; your savings vary by tool-output ratio.

---

## ⚡ Quick Start & Installation

Install `/shake` into your global Antigravity environment with a single self-contained command:

### Linux / macOS:
```bash
curl -fsSL https://raw.githubusercontent.com/shitan198u/antigravity-shake-skill/main/install.sh | bash
```

### Windows (PowerShell):
```powershell
irm https://raw.githubusercontent.com/shitan198u/antigravity-shake-skill/main/install.ps1 | iex
```

---

## 🚀 How to Use

### 1. In Antigravity Chat (Slash Command)
Simply type `/shake` whenever context fills up, responses become laggy, or before starting a complex task.

---

### 2. Standalone CLI Utility Suite

```bash
# 1. Run adaptive compaction (updates shake_latest.md by default)
shake-prune run /path/to/transcript.jsonl

# 2. Preview compaction impact safely without modifying disk
shake-prune preview /path/to/transcript.jsonl

# 3. Inspect context health, token size, and recommendations
shake-prune status /path/to/transcript.jsonl

# 4. Safely undo / rollback from atomic backup
shake-prune undo /path/to/transcript.jsonl

# 5. Inspect archived tool execution from permanent master log (supports --redact)
shake-prune show /path/to/transcript.jsonl --step 42 --pretty --redact
shake-prune show /path/to/transcript.jsonl --line 128

# 6. Verify environment, hooks, and permissions
shake-prune doctor
```

---

## ⚙️ Configuration (`shake.toml`)

Configure system-wide settings in `~/.gemini/config/shake.toml`:

```toml
[shake]
keep_recent_turns = 10          # Keep last N human user turns verbatim
keep_recent_tools = 20          # Keep last N tool runs raw
keep_recent_errors = 30         # Keep un-clamped error traces for last N tools
deep_after_user_turns = 30      # Automatically switch to deep compaction past 30 turns
redact_secrets = false          # Redact API keys, tokens, and bearer secrets

[auto]
enabled = true                  # Set to false to disable auto-shake hook completely
size_threshold_bytes = 264000   # ~80k tokens
tool_burst_threshold = 20       # Autonomous tool burst trigger
cooldown_seconds = 180          # 3-minute cooldown between compactions
growth_delta_bytes = 25600      # 25 KB transcript growth required

[retention]
artifact_retention_count = 20   # Maximum historical artifacts retained

[diagnostics]
log_level = "info"
```

All settings can also be set via environment variables:
`SHAKE_KEEP_RECENT_TURNS`, `SHAKE_KEEP_RECENT_TOOLS`, `SHAKE_KEEP_RECENT_ERRORS`,
`SHAKE_DEEP_AFTER_TURNS`, `SHAKE_SECRET_REDACTION=1`, `SHAKE_AUTO_DISABLE=1`.

---

## 🗑️ Uninstallation

To completely remove the binary, skill definitions, and lifecycle hook:

### Linux / macOS:
```bash
./install.sh --uninstall
```

### Windows:
```powershell
powershell -File .\install.ps1 -Uninstall
```

---

## 📚 Technical Documentation & Deep Dives

* 🧠 **[Antigravity Lifecycle & Backend vs. UI Cache](references/antigravity_lifecycle.md)**: Deep dive on model prompt streams vs. webview DOM caches, Inode preservation, and server-side checkpointing prevention.
* 🛠️ **[How `/shake` Works Technical Reference](references/how_it_works.md)**: Breakdown of the Single Master Archive, line-indexed receipts, token calibration, and security hardening.
* ⚖️ **[Comparison with Other Compaction Tools](references/omp_comparison.md)**: Detailed comparison between `/shake`, OMP, and traditional summarization techniques.

---

## 🛡️ Security & Reliability Architecture

* **Strict Input/Output Validation**: Rejects access to sensitive system paths (`/etc`, `/root`, `C:\Windows`).
* **Context Poisoning Prevention**: Restricts anchor discovery strictly to canonical system directories (`~/.gemini`).
* **Exclusive Concurrency (`fs2`)**: Holds exclusive file locks during in-place truncation and issues `fsync` (`sync_all()`) before unlocking.
* **Pre-Commit Fingerprint & Intent Journaling**: Ensures zero risk of data loss from concurrent writes or mid-rewrite interruptions.
* **Fail-Open Hook Guarantee**: The `PreInvocation` and `Stop` hooks run with `panic::catch_unwind` protection—they will always emit `{}` and exit cleanly on any error or panic.

---

## 📄 License

MIT License. Copyright (c) 2026 shitan198u.
