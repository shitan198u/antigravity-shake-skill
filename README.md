# Antigravity `/shake` Context Compaction & Verbatim Pruning Skill

A high-performance context management, compaction, and tree-shaking skill for the **Google Antigravity AI Agent** platform (CLI, IDE, and 2.0) across **Linux, macOS, and Windows**.

Inspired by the `/shake` context compaction mechanism in **omp (Oh-My-Pi)**, this tool solves "context rot" and token exhaustion in long-running coding sessions by **deterministically stripping verbose command outputs, file views, and tool payloads** while preserving **100% of User prompts, Assistant reasoning, and Thought processes verbatim**.

---

## 📸 Interactive IDE Artifact Preview

When `/shake` runs, it generates an **Interactive IDE Artifact** with explicit context instructions and session metadata:

![Shaken Transcript Artifact Preview](./assets/artifact_preview.png)

---

## ⚡ Key Highlights

* **🟢 Seamless In-Window Continuity (No New Tab Required!)**:
  * Utilizes a native Antigravity **`PreInvocation` Hook** (`shake-prune --hook`).
  * When `/shake` completes, it creates an active session anchor.
  * **You stay in the exact same chat window**—when you press Send on your next turn, the native hook automatically pins the model's working memory to the clean shaken artifact!
* **🦀 100% Self-Contained Native Binary**:
  * One single binary (`shake-prune` / `shake-prune.exe`) handles both context pruning and sub-millisecond hook execution (`<0.2ms`).
  * **Zero Python & Zero Rust dependencies** required at runtime.
* **🌐 Cross-Platform Support (Linux, macOS, Windows)**:
  * Pure Bash installer (`install.sh`) for Linux and macOS.
  * Native PowerShell installer (`install.ps1`) for Windows.
  * Automated GitHub Actions multi-platform binary compilation.
* **🤖 Agent-Friendly (`AGENTS.md`)**:
  * Built-in instructions for other AI coding agents to auto-configure and adapt this skill on unsupported machines.
* **80%–90% Token Reduction**: Recovers hundreds of thousands of tokens from bloated transcripts instantly.
* **Zero Loss of Meaning**: Unlike lossy LLM summarizers, User instructions, bug investigations, Assistant architectural decisions, and Thoughts are preserved **word-for-word**.
* **Smart Signal Preservation**:
  * ⚠️ **Errors & Failures**: Never deletes non-zero exit codes, compiler tracebacks, or test failures.
  * 🕒 **Active Working Window**: Retains recent tool execution outputs to preserve immediate task momentum.
  * 📋 **Status Commands**: Keeps compact status checks (`git branch`, `pwd`, short checks).
  * ⚙️ **Action Receipts**: Replaces bulky file reads and terminal logs with clear single-line action receipts.
* **Dynamic Topic Naming**: Auto-generates clean, timestamped filenames based on the conversation topic (e.g. `shake_earbud_fit_test_20260901_2018.md`).

---

## 🚀 Quick Installation

### 🐧 Linux & 🍎 macOS
```bash
git clone https://github.com/shitan198u/antigravity-shake-skill.git
cd antigravity-shake-skill
./install.sh
```

### 🪟 Windows (PowerShell)
```powershell
git clone https://github.com/shitan198u/antigravity-shake-skill.git
cd antigravity-shake-skill
.\install.ps1
```

---

## 💻 How to Use

### 1. In Any Antigravity Chat
When a conversation gets long or starts to experience context degradation, simply type:

```text
/shake
```

The agent executes the native pruner, sets the session anchor, and presents the clean report:

```markdown
# ⚡ Context Compaction & Tree-Shaking Report

Context for this session has been compacted and anchored in this chat window.
All **User prompts, Assistant reasoning, Thoughts, and Error signals are 100% preserved verbatim**.

---

### 📊 Token Reduction Metrics

| Metric | Original | Pruned | Savings |
| :--- | :--- | :--- | :--- |
| **Payload Size** | `2.7 MB` | `497.7 KB` | **81.4% reduction** |
| **Estimated Tokens** | `~669,560` | `~124,424` | **~545,136 tokens saved** |
| **Preserved Signals** | 53 User turns (100%) | 70 Assistant turns (100%) | 14 Error traces (100%) |

---

### 🟢 In-Window Continuity Active
> **Ready to continue**: Your context memory is now pinned to the clean state. Simply type your next prompt and press **Send** in this chat.

- **Interactive Artifact**: [📄 shake_topic_20260901_2018.md](file:///path/to/shake_topic.md) *(Click to preview in side pane)*

<details>
<summary>📋 Need to export or copy this session elsewhere?</summary>

- **In-Chat Mention**: `@/path/to/shake_topic.md`
- **Copy to Project**: `cp "/path/to/shake_topic.md" ./`
- **Copy to Clipboard**: `xclip -sel clip < "/path/to/shake_topic.md" || wl-copy < "/path/to/shake_topic.md"`
</details>
```

### 2. Keep Chatting in the Same Tab!
You don't have to leave the window! Just type your next message and press **Send**. The native `PreInvocation` hook automatically ensures the agent continues with full clarity based on the clean shaken state.

---

## 📦 Package Contents

```text
antigravity-shake/
├── .github/
│   └── workflows/
│       └── release.yml           # Multi-platform CI/CD (Linux, macOS, Windows)
├── assets/
│   └── artifact_preview.png      # Rendered preview screenshot
├── bin/
│   └── shake-prune               # Precompiled native binary (x86_64, includes --hook)
├── install.sh                     # Automated installer for Linux & macOS
├── install.ps1                    # Automated installer for Windows (PowerShell)
├── AGENTS.md                      # Cross-platform adaptation guide for AI agents
├── README.md                      # Comprehensive documentation
├── SKILL.md                       # Antigravity skill definition (Zero-guesswork workflow)
├── references/
│   └── omp_comparison.md          # Technical analysis of omp vs Antigravity pruning
├── scripts/
│   └── shake_prune.py             # Universal Python fallback engine
└── shake-prune-rs/                # High-speed Rust crate source
    ├── Cargo.toml                 # Dependencies & release profile
    └── src/
        ├── main.rs                # CLI entry point, anchor generator & report
        ├── hook.rs                # Native sub-millisecond PreInvocation hook runner
        ├── metadata.rs            # Atomic writes for IDE Artifact & active_shake_anchor.json
        ├── models.rs              # Typed Antigravity event schemas
        ├── pruner.rs              # Signal-preserving filter & markdown generator
        └── slug.rs                # Topic slug & filename generator
```

---

## 📄 License
MIT License. Free for distribution across all Antigravity users.
