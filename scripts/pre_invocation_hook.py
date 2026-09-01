#!/usr/bin/env python3
"""
Lightweight Antigravity PreInvocation Hook for /shake Context Anchoring.
Injects a concise 1-line anchor directive if an active /shake snapshot exists.
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

    candidate_paths = []
    if artifact_dir:
        candidate_paths.append(Path(artifact_dir) / "active_shake_anchor.json")
    if transcript_path:
        candidate_paths.append(Path(transcript_path).parent.parent / "active_shake_anchor.json")
    if conv_id:
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
        print(json.dumps({}))
        return

    shaken_file = anchor_data.get("shaken_file", "")
    anchored_step = anchor_data.get("anchored_at_step", "")

    # Ultra-concise, zero-token-waste anchor directive
    ephemeral_msg = (
        f"[Context compacted via /shake. Active state anchored in @{shaken_file} "
        f"(Step {anchored_step}+). Treat prior raw tool stdout as archived.]"
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
