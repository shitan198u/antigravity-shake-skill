# ⚡ Antigravity `/shake` Skill

> **Deterministic, In-Place Context Tree-Shaking and Token Compaction for Google Antigravity (Gemini)**  
> *Physically compact active conversation logs on disk, eliminate context degradation, and continue chatting seamlessly in the **exact same tab**.*

---

## 📸 Visual Gallery

<details open>
<summary>🔍 <b>Side-by-Side Comparison: Standard <code>/shake</code> vs. Deep <code>/full-shake</code></b></summary>
<br/>

| 🟢 Standard `/shake` (100% Thoughts Retained) | ⚡ Deep `/full-shake` (20-Turn Thought Window) |
| :---: | :---: |
| <a href="assets/artifact_preview.png"><img src="assets/artifact_preview.png" width="380" alt="Standard /shake Report"></a> | <a href="assets/full_shake_preview.png"><img src="assets/full_shake_preview.png" width="380" alt="Deep /full-shake Report"></a> |
| *Zero-loss baseline report* | *Includes 2-tier scopes & timeline dropdown* |

</details>

---

## 🌟 Why `/shake`?

During long development sessions, AI coding agents accumulate tens of thousands of lines of terminal output, compiler logs, and file inspections. This leads to **"context rot"**:
1. **Severe Latency**: Millions of raw characters are re-serialized and sent across the wire on every turn.
2. **Attention Decay**: The LLM loses track of earlier instructions under a mountain of terminal noise.
3. **Lost Momentum**: Developers are forced to open new chat tabs, re-explaining the project context and losing state.

`/shake` solves this by **physically pruning the active `transcript.jsonl` on disk in-place** while preserving 100% of your dialogue, reasoning, thoughts, error traces, and active working state.

---

## 🚀 Key Features

* 🟢 **Same-Tab In-Place Compaction**: Modifies active session logs directly on disk without breaking open file descriptors or requiring new chat tabs.
* 🛡️ **Inode Preservation & File Locking**: Uses POSIX/Windows truncate-and-rewrite with `fs2` exclusive locking and `fsync` durability.
* ⚡ **Proactive 200k Token Auto-Shake**: Background `PreInvocation` hook automatically detects and compacts conversations that exceed 200k tokens (`660 KB`).
* ⏱️ **50 KB Growth Delta Guard**: Prevents redundant CPU/disk cycles when conversations contain extensive clean dialogue.
* 🧠 **100% Signal Retention**: Preserves all user prompts, assistant thoughts/reasoning, and non-zero exit error traces verbatim.
* 🕒 **Active Working Window**: Retains the last 6 execution steps with 0% pruning so active momentum is never interrupted.
* 🔍 **Progressive Disclosure Backlinks**: Compacts older code writes into lightweight receipts containing canonical absolute paths to timestamped backups (`.bak_<timestamp>`).
* 🔒 **Hardened Security**: ReDoS-immune linear scanning, HTML/XSS sanitization, URL-encoded links, exclusive `0600` tempfiles, and strict canonical allowlists.
* 🦀 **Pure Native Rust**: Precompiled multi-platform binaries for Linux (x86_64), macOS (Universal Binary for Apple Silicon & Intel), and Windows (x86_64).

---

## 📊 Physical Token Reduction Metrics

| Metric | Original Active Session | Compacted via `/shake` | Total Savings |
| :--- | :--- | :--- | :--- |
| **Payload Size on Disk** | `1.4 MB` | `470 KB` | **54.5% – 78% physical reduction** |
| **Estimated Token Load** | `~420,000 tokens` | `~145,000 tokens` | **`~275,000 tokens saved`** |
| **Execution Overhead** | — | Native Rust binary | **`<10ms` in-place rewrite** |
| **Hook Latency** | — | Native PreInvocation | **`<0.2ms` per prompt** |

---

## 📦 Quick Installation

### Linux & macOS (curl / bash)
```bash
curl -fsSL https://raw.githubusercontent.com/shitan198u/antigravity-shake-skill/main/install.sh | bash
```

### Windows (PowerShell)
```powershell
irm https://raw.githubusercontent.com/shitan198u/antigravity-shake-skill/main/install.ps1 | iex
```

### Build from Local Source
```bash
cargo build --release --manifest-path shake-prune-rs/Cargo.toml
cp shake-prune-rs/target/release/shake-prune ~/.gemini/bin/shake-prune
```

---

## 💡 How to Use

### 1. In Any Antigravity Chat (Interactive Slash Commands)

#### 🟢 Standard Zero-Loss Compaction (`/shake`)
* Prunes all tool output dumps.
* Retains **100% of User prompts, Assistant reasoning, Thoughts, and Errors verbatim**.
```text
/shake
```

#### ⚡ Full Deep Compaction (`/full-shake`)
* Prunes all tool output dumps.
* Retains **100% of User prompts, Assistant explanations, Decisions, and Errors verbatim**.
* Retains scratchpad thoughts (`thinking`) for the **last 20 turns** while dropping older thoughts (saving an extra ~400 KB – 500 KB on mega-threads).
* Automatically acts as a natural shake if the session has $\le 20$ turns.
```text
/full-shake
```
The agent will execute the pruner, report token savings, update the interactive anchor artifact, and allow you to **keep typing in the same chat tab**!

### 2. Fully Automatic (Background Hook)
You don't even have to remember to run `/shake`! The included `PreInvocation` hook continuously monitors transcript growth and automatically compacts the conversation when it crosses **200,000 tokens** (`660 KB`).

### 3. Standalone CLI Usage
```bash
# Compact a specific transcript in-place
shake-prune /path/to/brain/session_id/.system_generated/logs/transcript.jsonl

# Run full deep compaction with custom thought window
shake-prune /path/to/transcript.jsonl --full --thought-window 25

# Specify a custom output directory and keep the last 8 tool steps intact
shake-prune /path/to/transcript.jsonl /path/to/output_dir/ --recent-window 8

# Generate markdown report only without modifying transcript.jsonl
shake-prune /path/to/transcript.jsonl --no-in-place

# Run as Antigravity lifecycle hook
shake-prune --hook
```

---

## 📚 Technical Documentation & Deep Dives

* 🧠 **[Antigravity Lifecycle & Backend vs. UI Cache](references/antigravity_lifecycle.md)**: Deep dive on how Antigravity handles model prompt streams vs. webview DOM caches, Inode preservation mechanics, and hook lifecycles.
* 🛠️ **[How `/shake` Works Technical Reference](references/how_it_works.md)**: Comprehensive architectural breakdown of the 2-pass pruning pipeline, token calibration density, and security hardening.
* ⚖️ **[Comparison with Other Compaction Tools](references/omp_comparison.md)**: Detailed comparison between `/shake`, OMP, and traditional summarization techniques.

---

## 🛡️ Security & Reliability Architecture

* **Strict Input/Output Validation**: Rejects access to sensitive system paths (`/etc`, `/root`, `C:\Windows`).
* **Context Poisoning Prevention**: Restricts anchor discovery strictly to canonical system directories (`~/.gemini`).
* **Exclusive Concurrency (`fs2`)**: Holds exclusive file locks during in-place truncation and issues `fsync` (`sync_all()`) before unlocking.
* **Fail-Open Hook Guarantee**: The `PreInvocation` hook runs with `panic::catch_unwind` protection—it will always emit `{}` and exit cleanly on any error or panic.

---

## 📄 License

MIT License. Copyright (c) 2026 shitan198u.
