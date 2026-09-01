# Antigravity `/shake` Context Compaction & Verbatim Pruning Skill

A high-performance context management, compaction, and tree-shaking skill for the **Google Antigravity AI Agent** platform (CLI, IDE, and 2.0).

Inspired by the `/shake` context compaction mechanism in **omp (Oh-My-Pi)**, this tool solves "context rot" and token exhaustion in long-running coding sessions by **deterministically stripping verbose command outputs, file views, and tool payloads** while preserving **100% of User prompts, Assistant reasoning, and Thought processes verbatim**.

---

## 📸 Interactive IDE Artifact Preview

When `/shake` runs, it generates an **Interactive IDE Artifact** with explicit context instructions and session metadata:

![Shaken Transcript Artifact Preview](./assets/artifact_preview.png)

```markdown
# Shaken & Pruned History: EXECUTIVE SUMMARY YOUR OPOBUDS

> [!IMPORTANT]
> **Context Note for Assistant**: This document is a complete, verbatim transcript of earlier turns with token bloat removed via `/shake`.
> - **User prompts, Assistant explanations, and Thought processes are 100% complete and verbatim.**
> - Actions marked `[Command completed successfully]` or `[File inspected]` were already executed with success.
> - You do **NOT** need to re-run past successful commands unless the user explicitly requests it.
> - Any errors or failures encountered in past turns are explicitly preserved below with full stack traces.
> - The active working state and immediate recent tool outputs are preserved at the end of the transcript.

- **Session ID**: `c6e172d1-5cc6-42ad-b0b8-56e2ac668326`
- **Topic**: `executive summary your opobuds`
- **Source Transcript**: `/home/shsrra/.gemini/antigravity-ide/brain/.../transcript.jsonl`
- **User Turns**: 17 | **Assistant Turns**: 13
- **Tool Dumps Pruned**: 232 | **Errors Preserved**: 21
---
```

---

## ⚡ Key Highlights

* **🟢 Seamless In-Window Continuity (No New Tab Required!)**:
  * Utilizes an Antigravity **`PreInvocation` Lifecycle Hook** (`hooks.json`).
  * When `/shake` completes, it creates an active session anchor.
  * **You stay in the exact same chat window**—when you press Send on your next turn, the hook automatically pins the model's working memory to the clean shaken artifact!
* **80%–90% Token Reduction**: Recovers hundreds of thousands of tokens from bloated transcripts instantly.
* **Zero Loss of Meaning**: Unlike lossy LLM summarizers, User instructions, bug investigations, Assistant architectural decisions, and Thoughts are preserved **word-for-word**.
* **Smart Signal Preservation**:
  * ⚠️ **Errors & Failures**: Never deletes non-zero exit codes, compiler tracebacks, or test failures.
  * 🕒 **Active Working Window**: Retains recent tool execution outputs to preserve immediate task momentum.
  * 📋 **Status Commands**: Keeps compact status checks (`git branch`, `pwd`, short checks).
  * ⚙️ **Action Receipts**: Replaces bulky file reads and terminal logs with clear single-line action receipts.
* **Prebuilt High-Speed Engine**:
  * **Precompiled Native Binary (`bin/shake-prune`)**: Included out-of-the-box (sub-10ms execution on multi-megabyte transcripts).
  * **Rust Source Included (`shake-prune-rs/`)**: Ready to recompile via `cargo` on non-x86_64 systems.
  * **Universal Python Fallback (`scripts/shake_prune.py`)**: Zero-dependency fallback for any environment.
* **Dynamic Topic Naming**: Auto-generates clean, timestamped filenames based on the conversation topic (e.g. `shake_earbud_fit_test_20260901_2018.md`).

---

## 🚀 Quick Installation

Clone or copy this folder to your machine, then run the installer:

```bash
cd antigravity-shake
./install.sh
```

### 🛡️ Smart 3-Tier Installation Cascade
The installer automatically:
1. Installs the native Linux binary (`bin/shake-prune`) or compiles via `cargo`.
2. Sets up the universal Python fallback engine (`scripts/shake_prune.py`).
3. **Registers the `PreInvocation` hook in `~/.gemini/config/hooks.json`** for in-window continuity.

---

## 💻 How to Use

### 1. In Any Antigravity Chat
When a conversation gets long or starts to experience context degradation, simply type:

```text
/shake
```

The agent will execute the pruner, set the session anchor, and output the report:

```text
================================================================================
               ⚡ SHAKE CONTEXT PRUNING REPORT (RUST NATIVE) ⚡
================================================================================
• Session ID:           6b25943b-bbe3-48a4-9731-72f07389d0b4
• Topic:                GOAL RELEVANT DEVICE PLUGGED
• Original Payload:     2,678,240 bytes (~669,560 tokens)
• Pruned Payload:       497,697 bytes (~124,424 tokens)
• Token Savings:        81.4% reduction (~545k tokens saved!)
• Preserved Signals:    53 user turns (100%), 70 assistant turns (100%), 14 errors
--------------------------------------------------------------------------------
📋 RESUMPTION PATHS & QUICK-COPY
--------------------------------------------------------------------------------
• In-Window Continuity: 🟢 ACTIVE (Next message in this tab will use clean context)
• Absolute File Path:   /home/shsrra/.gemini/antigravity-ide/brain/.../shake_topic_YYYYMMDD_HHMM.md
• In-Chat Mention:      @/home/shsrra/.gemini/antigravity-ide/brain/.../shake_topic_YYYYMMDD_HHMM.md
• Copy to Project:      cp "/home/shsrra/.gemini/antigravity-ide/brain/.../shake_topic_YYYYMMDD_HHMM.md" ./
• Copy to Clipboard:    xclip -sel clip < "/home/shsrra/.gemini/antigravity-ide/brain/.../shake_topic_YYYYMMDD_HHMM.md"
================================================================================
```

### 2. Keep Chatting in the Same Tab!
You don't have to leave the window! Just type your next message and press **Send**. The `PreInvocation` hook automatically ensures the agent continues with full clarity based on the clean shaken state.

---

## 📦 Package Contents

```text
antigravity-shake/
├── assets/
│   └── artifact_preview.png      # Rendered preview screenshot
├── bin/
│   └── shake-prune               # Precompiled native Linux binary (x86_64)
├── hooks.json                     # PreInvocation lifecycle hook configuration
├── install.sh                     # Automated 3-tier installer (sets up binary + hook)
├── README.md                      # Documentation & usage guide
├── SKILL.md                       # Antigravity skill definition
├── references/
│   └── omp_comparison.md          # Technical analysis of omp vs Antigravity pruning
├── scripts/
│   ├── pre_invocation_hook.py     # Seamless in-window hook interceptor
│   └── shake_prune.py             # Universal Python fallback engine
└── shake-prune-rs/                # High-speed Rust crate source
    ├── Cargo.toml                 # Dependencies & release profile
    └── src/
        ├── main.rs                # CLI entry point, anchor generator & report
        ├── metadata.rs            # IDE Artifact & active_shake_anchor.json writer
        ├── models.rs              # Typed Antigravity event schemas
        ├── pruner.rs              # Signal-preserving filter & markdown generator
        └── slug.rs                # Topic slug & filename generator
```

---

## 📄 License
MIT License. Free for distribution across all Antigravity users.
