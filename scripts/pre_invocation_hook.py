#!/usr/bin/env python3
"""
Antigravity PreInvocation Hook for /shake Context Anchoring (Pathway D).
Fires before every model invocation. If a session has an active /shake anchor,
it injects an ephemeral system message pinning the model's reasoning strictly
to the shaken artifact without requiring a new chat window.
"""

import sys
import os
import json
from pathlib import Path

def main():
    try:
        raw_input = sys.stdin.read()
        if not raw_input.strip():
            print(json.dumps({}))
            return

        payload = json.loads(raw_input)
    except Exception:
        print(json.dumps({}))
        return

    conv_id = payload.get("conversationId", "")
    artifact_dir = payload.get("artifactDirectoryPath", "")
    transcript_path = payload.get("transcriptPath", "")

    # Look for active_shake_anchor.json in artifact dir or alongside transcript
    candidate_paths = []
    if artifact_dir:
        candidate_paths.append(Path(artifact_dir) / "active_shake_anchor.json")
    if transcript_path:
        candidate_paths.append(Path(transcript_path).parent.parent / "active_shake_anchor.json")
    if conv_id:
        # Check standard brain directories
        for base in ["~/.gemini/antigravity-ide/brain", "~/.gemini/antigravity/brain", "~/.gemini/antigravity-cli/brain"]:
            candidate_paths.append(Path(os.path.expanduser(base)) / conv_id / "active_shake_anchor.json")

    anchor_data = None
    for cp in candidate_paths:
        if cp.exists():
            try:
                with open(cp, "r", encoding="utf-8") as f:
                    data = json.load(f)
                    if data.get("active"):
                        anchor_data = data
                        break
            except Exception:
                pass

    if not anchor_data:
        # No active anchor; standard invocation
        print(json.dumps({}))
        return

    shaken_file = anchor_data.get("shaken_file", "")
    topic = anchor_data.get("topic", "Active Session").replace("_", " ").title()
    anchored_step = anchor_data.get("anchored_at_step", "earlier")
    savings_pct = anchor_data.get("token_savings_pct", 0.0)

    ephemeral_msg = (
        f"🚨 [CONTEXT COMPACTION ANCHOR ACTIVE — /shake] 🚨\n"
        f"The user has compacted this session's context memory (~{savings_pct:.1f}% token bloat pruned up to Step {anchored_step}).\n\n"
        f"📌 **Primary Working State & Memory Anchor**:\n"
        f"You MUST anchor your active context, decisions, and reasoning strictly on the clean state in:\n"
        f"👉 `{shaken_file}`\n\n"
        f"Guidelines for this turn:\n"
        f"1. Treat all earlier raw tool outputs (prior to Step {anchored_step}) as archived history.\n"
        f"2. Continue seamlessly in this same chat window based on the active goals and pending steps in `{shaken_file}`.\n"
        f"3. Do NOT re-run past successful tool calls unless specifically asked."
    )

    response = {
        "injectSteps": [
            {
                "ephemeralMessage": ephemeral_msg
            }
        ]
    }

    print(json.dumps(response))

if __name__ == "__main__":
    main()
