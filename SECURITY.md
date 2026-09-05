# Security Policy

## Threat Model & Safety Guarantees

`antigravity-shake` and its companion binary `shake-prune` run locally on the user's workstation or CI runner. They operate on sensitive conversation transcripts, execution receipts, and terminal output.

### 1. Concurrency & Pre-Commit Protection
- **Advisory File Locking**: `shake-prune` uses advisory file locking via `fs2` (`lock_exclusive` / `try_lock_exclusive`) on the target transcript file.
- **Snapshot Fingerprinting**: Before truncating or rewriting any transcript, `shake-prune` verifies the file's current size and modification time against the pre-read snapshot fingerprint. If an uncooperative concurrent process modified the file while compaction was computing, `shake-prune` aborts immediately to avoid overwriting unread data.
- **Intent Journaling & Crash Recovery**: Compaction writes a pending journal entry to `.shake_in_progress` with `0600` permissions on Unix. If interrupted (power failure, process kill), subsequent runs automatically detect the journal and restore state safely. Under hook execution, crash recovery uses non-blocking lock acquisition with a watchdog deadline to ensure the agent never deadlocks or hangs.

> [!NOTE]
> File locks on Linux and macOS are advisory by default. Any custom tooling modifying conversation logs in parallel should observe standard file locking conventions.

### 2. Platform Permissions & Windows ACLs
- **Unix (Linux & macOS)**: All sensitive internal files (`.shake_in_progress`, `shake_metadata.json`, `shake_hook.log`, `.jsonl.bak`, and master archive `transcript_full.jsonl`) are created with restricted `0600` (read/write only by owner) permissions.
- **Windows**: Windows relies on standard NTFS Access Control Lists (ACLs) inherited from the user's home profile (`%USERPROFILE%`). Explicit Windows DACL hardening is out of scope; ensure user directories are not shared across local accounts.

### 3. Secret Redaction & Permanent Archives
When `redact_secrets = true` is configured or `--redact` is passed:
- **Redacted Targets**: Compaction summaries (`shake_latest.md`), in-memory pruned context injected into the agent's context window, and `shake-prune show --redact` output have credentials masked (`[REDACTED_SECRET]`). Detected patterns include OpenAI API keys (`sk-...`), Google API keys (`AIza...`), Slack tokens (`xox...`), GitHub tokens (`ghp_...`, `gho_...`), AWS access keys (`AKIA...`), and generic credential assignments.
- **Forensic Audit Archive**: The master transcript archive (`transcript_full.jsonl`) and crash recovery backup (`.jsonl.bak`) preserve verbatim conversation steps without truncation or redaction by design, serving as an immutable, complete record. Users operating in shared multi-tenant environments must ensure proper directory permissions.

### 4. Subcommand Path Validation
All CLI subcommands (`run`, `preview`, `status`, `undo`, `show`) validate input transcript paths:
- Traversal attacks (`..`) attempting to read system directories (e.g. `/etc`, `/proc`, `/sys`, `/dev`, `C:\Windows`, `C:\System32`) are blocked with a validation error.
- Only `.jsonl` transcript files are accepted.

---

## Supported Versions

| Version | Supported | Notes |
| :--- | :--- | :--- |
| `0.2.x` | ✅ Yes | Current stable release line |
| `< 0.2.0` | ❌ No | Upgrade to `0.2.1` or later |

---

## Reporting a Vulnerability

If you discover a security vulnerability or potential data leak in `antigravity-shake`, please do NOT open a public issue.

Instead:
1. Open a private security advisory on GitHub: [Security Advisories](https://github.com/shitan198u/antigravity-shake-skill/security/advisories/new)
2. Provide a clear description of the vulnerability, reproduction steps, and potential impact.

We will review and respond to reports within 48 hours and work with you to coordinate a patched release.
