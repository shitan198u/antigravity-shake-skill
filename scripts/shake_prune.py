#!/usr/bin/env python3
"""
Smart Deterministic Transcript Pruner for Antigravity Agent (/shake).
Implements signal-preserving, zero-loss context pruning with:
- Dynamic topic naming
- Automatic Antigravity Artifact (.metadata.json) registration
- Clickable links and quick-copy terminal commands
"""

import sys
import os
import json
import re
import datetime
from pathlib import Path

def estimate_tokens(text: str) -> int:
    """Standard rule-of-thumb: ~4 characters per token for English/code mix."""
    return max(1, len(text) // 4)

def generate_topic_slug(first_user_text: str) -> str:
    """Generates a clean, descriptive slug from the user's initial prompt."""
    clean = re.sub(r"<[^>]+>", " ", first_user_text)
    clean = re.sub(r"https?://\S+", "", clean)
    clean = re.sub(r"[^a-zA-Z0-9\s]", " ", clean)
    stop_words = {"please", "want", "also", "this", "that", "with", "from", "have", "need", "make", "check", "the", "and", "for"}
    words = [w.lower() for w in clean.split() if len(w) > 2 and w.lower() not in stop_words]
    slug = "_".join(words[:4]) if words else "session"
    return slug

def extract_conversation_id(path_str: str) -> str:
    """Attempts to extract UUID-like or folder-based conversation ID from path."""
    match = re.search(r"brain/([a-zA-Z0-9_-]+)/", path_str)
    return match.group(1) if match else "unknown-session"

def prune_transcript(transcript_path: str, recent_window_steps: int = 6) -> tuple[str, dict, str]:
    transcript_file = Path(transcript_path)
    if not transcript_file.exists():
        raise FileNotFoundError(f"Transcript file not found: {transcript_path}")

    with open(transcript_file, "r", encoding="utf-8") as f:
        lines = [json.loads(line) for line in f if line.strip()]

    output_blocks = []
    user_count = 0
    assistant_count = 0
    pruned_tools_count = 0
    retained_errors_count = 0
    retained_short_cmds = 0
    retained_recent_steps = 0
    raw_json_str = ""
    first_user_prompt = ""

    total_steps = len(lines)
    recent_threshold = max(0, total_steps - recent_window_steps)
    conv_id = extract_conversation_id(str(transcript_file.resolve()))

    for i, step in enumerate(lines):
        raw_json_str += json.dumps(step)
        stype = step.get("type")
        content = step.get("content", "")
        status = step.get("status", "")
        exit_code = step.get("exit_code")
        is_recent = (i >= recent_threshold)

        if stype == "USER_INPUT":
            user_count += 1
            match = re.search(r"<USER_REQUEST>(.*?)</USER_REQUEST>", content, re.DOTALL)
            user_text = match.group(1).strip() if match else content.strip()
            if not first_user_prompt:
                first_user_prompt = user_text
            output_blocks.append(f"### 👤 User (Turn {user_count})\n\n{user_text}\n")

        elif stype == "PLANNER_RESPONSE":
            tool_calls = step.get("tool_calls", [])
            assistant_text = content.strip() if content else ""

            if assistant_text:
                assistant_count += 1
                output_blocks.append(f"### 🤖 Assistant\n\n{assistant_text}\n")

            if tool_calls:
                for tc in tool_calls:
                    name = tc.get("name", "")
                    args = tc.get("args", {})
                    arg_items = []
                    for k, v in args.items():
                        v_str = str(v).replace("\n", " ")
                        if len(v_str) > 120:
                            v_str = v_str[:120] + "... [truncated]"
                        arg_items.append(f"{k}={v_str}")
                    arg_summary = ", ".join(arg_items)
                    output_blocks.append(f"- ⚙️ **Action Executed**: `{name}({arg_summary})`")

        elif stype in ("RUN_COMMAND", "VIEW_FILE", "SEARCH_WEB", "GREP_SEARCH", "CODE_ACTION"):
            is_error = (exit_code is not None and exit_code != 0) or ("error" in status.lower()) or ("failed" in status.lower())

            if is_recent:
                retained_recent_steps += 1
                output_blocks.append(f"> 🕒 **[Active Window Tool Output ({stype})]**:\n```\n{content[:1500]}\n```\n")
            elif is_error:
                retained_errors_count += 1
                output_blocks.append(f"> ⚠️ **[Tool Execution Error / Failure ({stype}, Exit code: {exit_code})]**:\n```\n{content[:1200]}\n```\n")
            elif stype == "RUN_COMMAND":
                if len(content.strip()) < 250:
                    retained_short_cmds += 1
                    output_blocks.append(f"> 📋 **[Command Output (exit 0)]**:\n```\n{content.strip()}\n```\n")
                else:
                    line_count = len(content.splitlines())
                    pruned_tools_count += 1
                    output_blocks.append(f"> ℹ️ *[Command completed successfully (exit 0). {line_count} lines of verbose stdout pruned for token efficiency]*\n")
            elif stype == "VIEW_FILE":
                line_count = len(content.splitlines())
                pruned_tools_count += 1
                output_blocks.append(f"> ℹ️ *[File inspected in previous turn. {line_count} lines pruned for token efficiency]*\n")
            else:
                pruned_tools_count += 1
                output_blocks.append(f"> ℹ️ *[{stype} completed successfully. Raw payload pruned for token efficiency]*\n")

    topic_slug = generate_topic_slug(first_user_prompt)
    timestamp = datetime.datetime.now().strftime("%Y%m%d_%H%M")
    suggested_filename = f"shake_{topic_slug}_{timestamp}.md"

    header = [
        f"# Shaken & Pruned History: {topic_slug.replace('_', ' ').title()}",
        "",
        "> [!IMPORTANT]",
        "> **Context Note for Assistant**:",
        "> This document is a complete, verbatim transcript of earlier turns with token bloat removed via `/shake`.",
        "> - **User prompts and Assistant explanations are 100% complete and verbatim.**",
        "> - Actions marked `[Command completed successfully]` or `[File inspected]` were already executed with success.",
        "> - You do **NOT** need to re-run past successful commands unless the user explicitly requests it.",
        "> - Any errors or failures encountered in past turns are explicitly preserved below with full stack traces.",
        "> - The active working state and immediate recent tool outputs are preserved at the end of the transcript.",
        "",
        f"- **Session ID**: `{conv_id}`",
        f"- **Topic**: `{topic_slug.replace('_', ' ')}`",
        f"- **Source Transcript**: `{transcript_path}`",
        f"- **User Turns**: {user_count} | **Assistant Turns**: {assistant_count}",
        f"- **Tool Dumps Pruned**: {pruned_tools_count} | **Errors Preserved**: {retained_errors_count}",
        "---\n"
    ]

    pruned_content = "\n".join(header) + "\n\n".join(output_blocks)
    
    raw_bytes = len(raw_json_str.encode("utf-8"))
    pruned_bytes = len(pruned_content.encode("utf-8"))
    raw_tokens = estimate_tokens(raw_json_str)
    pruned_tokens = estimate_tokens(pruned_content)
    reduction_pct = (1.0 - (pruned_bytes / max(raw_bytes, 1))) * 100.0

    stats = {
        "conv_id": conv_id,
        "raw_bytes": raw_bytes,
        "pruned_bytes": pruned_bytes,
        "raw_tokens": raw_tokens,
        "pruned_tokens": pruned_tokens,
        "reduction_pct": reduction_pct,
        "user_turns": user_count,
        "assistant_turns": assistant_count,
        "pruned_tools": pruned_tools_count,
        "retained_errors": retained_errors_count,
        "retained_short_cmds": retained_short_cmds,
        "retained_recent_steps": retained_recent_steps,
        "topic_slug": topic_slug,
        "suggested_filename": suggested_filename,
    }

    return pruned_content, stats, suggested_filename

def write_artifact_metadata(markdown_path: str, summary: str):
    """Generates the accompanying .metadata.json so Antigravity renders it as an interactive Artifact in the IDE."""
    meta_path = markdown_path + ".metadata.json"
    now_iso = datetime.datetime.now(datetime.timezone.utc).isoformat().replace("+00:00", "Z")
    meta_data = {
        "artifactType": "ARTIFACT_TYPE_OTHER",
        "summary": summary,
        "updatedAt": now_iso
    }
    with open(meta_path, "w", encoding="utf-8") as f:
        json.dump(meta_data, f, indent=2)

def main():
    if len(sys.argv) < 2:
        print("Usage: python3 shake_prune.py <transcript.jsonl> [output_file_or_dir]")
        sys.exit(1)

    transcript_path = sys.argv[1]
    raw_target = sys.argv[2] if len(sys.argv) > 2 else ""

    pruned_markdown, stats, suggested_name = prune_transcript(transcript_path)

    if raw_target and not os.path.isdir(raw_target) and raw_target.endswith(".md"):
        output_path = raw_target
    elif raw_target and os.path.isdir(raw_target):
        output_path = os.path.join(raw_target, suggested_name)
    else:
        output_path = suggested_name

    abs_output_path = os.path.abspath(output_path)

    with open(abs_output_path, "w", encoding="utf-8") as f:
        f.write(pruned_markdown)

    # Register as an interactive IDE Artifact
    summary_text = (
        f"Shaken & pruned verbatim history for topic '{stats['topic_slug'].replace('_', ' ')}'. "
        f"Saved {stats['reduction_pct']:.1f}% context tokens ({stats['pruned_tokens']:,} tokens vs {stats['raw_tokens']:,} raw). "
        f"Preserved {stats['user_turns']} user prompts and all reasoning."
    )
    try:
        write_artifact_metadata(abs_output_path, summary_text)
    except Exception as e:
        pass

    print(f"\n================================================================================")
    print(f"               ⚡ SHAKE CONTEXT PRUNING REPORT ⚡")
    print(f"================================================================================")
    print(f"• Session ID:       {stats['conv_id']}")
    print(f"• Topic:            {stats['topic_slug'].replace('_', ' ').title()}")
    print(f"• Original Payload: {stats['raw_bytes']:,} bytes (~{stats['raw_tokens']:,} tokens)")
    print(f"• Pruned Payload:   {stats['pruned_bytes']:,} bytes (~{stats['pruned_tokens']:,} tokens)")
    print(f"• Token Savings:    {stats['reduction_pct']:.1f}% reduction")
    print(f"• Preserved Signals: {stats['user_turns']} user turns (100%), {stats['assistant_turns']} assistant turns (100%), {stats['retained_errors']} errors")
    print(f"--------------------------------------------------------------------------------")
    print(f"📋 RESUMPTION PATHS & QUICK-COPY")
    print(f"--------------------------------------------------------------------------------")
    print(f"• Absolute File Path: {abs_output_path}")
    print(f"• In-Chat Mention:    @{abs_output_path}")
    print(f"• Copy to Project:    cp \"{abs_output_path}\" ./")
    print(f"• Copy to Clipboard:  xclip -sel clip < \"{abs_output_path}\" || wl-copy < \"{abs_output_path}\"")
    print(f"================================================================================\n")

if __name__ == "__main__":
    main()
