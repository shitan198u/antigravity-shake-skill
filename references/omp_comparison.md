# Technical Comparison: `omp` Agent `/shake` vs Antigravity Implementation

## What is `/shake` in `omp` (Oh-My-Pi)?

In the **Oh-My-Pi (omp)** CLI coding agent, `/shake` is a context-management command designed to solve the problem of **context bloat**, **token exhaustion**, and **attention degradation** in long-running LLM coding sessions.

### Core Mechanisms in `omp`:
1. **Tool Output Compaction**: Old, raw tool outputs (massive file views, build logs, stack traces, directory listings) are pruned or converted into short reference tokens.
2. **Conversation Pruning**: Intermediate reasoning chatter is compressed, retaining only high-level goals, system state, and essential diffs.
3. **Recent State Anchor**: The last active tool executions and user messages are preserved to prevent loss of task continuity.
4. **Token Savings**: Frees up the context window to prevent the model from hitting context limits or suffering from "lost-in-the-middle" retrieval issues.

---

## How It Is Mapped to Google Antigravity

Antigravity operates with a rich suite of artifacts, knowledge items, transcripts, and progressive disclosure skills. In Antigravity, `/shake` is realized as a high-performance **deterministic transcript pruner & native in-window hook**:

| Feature | `omp` Agent (CLI Runtime) | Antigravity Implementation (`antigravity-shake`) |
| :--- | :--- | :--- |
| **Invocation** | `/shake` terminal command | `/shake` slash command / skill trigger (`SKILL.md`) |
| **Execution Engine** | In-memory JavaScript/Python loop | Native compiled Rust binary (`shake-prune`, sub-10ms) |
| **Message Handling** | In-place mutation of RAM array | Deterministic stream pruning + Native `PreInvocation` hook |
| **In-Window Continuity** | Retains terminal scrollback | **Seamless in-window continuity** (`shake-prune --hook`) |
| **Lossy Summarization?** | No (strips tool outputs only) | **No** (100% verbatim dialogue, thoughts, & diffs) |
| **Error Handling** | May prune errors unless recent | **Strict signal preservation** (all error traces retained) |
| **Active Momentum** | Last N messages kept | Configurable active working window (default: last 6 tool steps) |
| **IDE Integration** | Terminal output only | Automatic **Interactive IDE Artifact** (`.metadata.json`) |
| **Platform Portability** | Node / Bun runtime | Self-contained ELF binary (0 Python/Rust runtime dependencies) |

---

## Why Deterministic Pruning Beats LLM Summaries

Many agent harnesses attempt to solve context bloat by asking an LLM to "summarize previous turns". This approach suffers from critical weaknesses:
1. **Loss of Critical Invariants**: Summaries frequently drop subtle edge-case instructions, specific flags, or exact error strings.
2. **Token Waste & Latency**: LLM summarization consumes hundreds of tokens and introduces 5–15 seconds of latency.
3. **Hallucination Risk**: Summaries can unintentionally rewrite previous agreements or omit unresolved bugs.

The `/shake` engine uses **purely deterministic, zero-LLM parsing** to strip only low-signal tool noise (compilation streams, file cat dumps, repeated status queries) while preserving 100% of the original developer dialogue, thoughts, and model reasoning.
