# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.2.1] - 2026-09-05

### Added
- **Multi-Phase Watchdog**: Hook watchdog deadline checks now occur across initialization, locking, master archive sync, transcript parsing, compaction loops (checked every 50 steps), and pre-commit snapshot validation, guaranteeing the 2.5s limit is respected even on massive transcripts.
- **Non-Blocking Hook Recovery**: Journal recovery in hook mode now uses non-blocking lock acquisition (`try_lock_exclusive()`) with a watchdog deadline, failing open immediately if another process holds the lock.
- **Adaptive Hook Compaction**: Auto-shake hook now inspects transcript turn metrics before compaction, automatically selecting `Deep` mode with Milestone Horizon preservation for marathon sessions (> 30 user turns).
- **Extended Secret Redaction Patterns**: Added regex patterns covering OpenAI API keys (`sk-...`), Google API keys (`AIza...`), Slack tokens (`xox...`), and generic credential assignments (`api_key = ...`).
- **CLI Show Redaction**: Added `--redact` flag to `shake-prune show` for safe terminal inspection of archived tool executions.
- **Anchor Invalidation on Undo**: `shake-prune undo` now cleans up or deactivates `active_shake_anchor.json` in the artifact directory, preventing stale anchor notices after restoring a backup.
- **Canonical Configuration Template**: Added `shake.example.toml` documenting modern config tables (`[shake]`, `[advanced]`, `[privacy]`, `[retention]`, `[diagnostics]`).
- **Security Policy**: Added `SECURITY.md` detailing advisory file locking, pre-commit snapshot fingerprints, platform permissions, and audit log guarantees.
- **Omitted Files Documentation**: Updated `scripts/bundle_repo.py` with an explicit list of intentionally omitted binary and build cache artifacts.

### Fixed
- **Config Boolean Normalization**: Fixed `ShakePrivacyConfig.redact_secrets` deserialization (`Option<bool>`) so explicit `false` values in `[privacy]` are not overwritten by legacy `[shake]` defaults.
- **Subcommand Path Validation**: Enforced strict path traversal and sensitive directory validation on `undo` and `show` subcommands.
- **Deep Compaction Thought Window**: User-specified `--thought-window` arguments are now preserved in `apply_deep` rather than unconditionally overridden to 20.
- **Hook Artifact Link Integrity**: Hook mode now writes canonical `shake_latest.md` (and maintains timestamped archives when retention > 0), ensuring continuity anchor links never 404.
- **Documentation Parity**: Aligned `README.md` configuration tables, CLI subcommand documentation (including `doctor` and `--redact`), and environment variable lists.

---

## [0.2.0] - 2026-09-04

### Added
- **Complete Rust Engine Rewrite**: Ported Python implementation to a zero-runtime-dependency, memory-safe Rust binary (`shake-prune`).
- **Atomic Operations**: Inode-preserving truncate-and-write with advisory file locking via `fs2`.
- **Pre-Commit Fingerprint Validation**: Detects uncooperative concurrent writes prior to file truncation.
- **Intent Journaling**: `.shake_in_progress` journal enables atomic crash recovery.
- **Master Archive**: Automatic appending of unpruned steps to `transcript_full.jsonl` prior to compaction.
- **Adaptive Compaction Engine**: Standard (10 turns / 20 tools / 30 errors) and Deep (Milestone Horizon + scratchpad retention) modes.
- **Unified CLI Suite**: `run`, `preview`, `status`, `undo`, `show`, and `doctor` subcommands.
- **Fail-Open Hook Architecture**: `--hook` exits with code 0 and `{}` on any error or lock timeout, ensuring zero agent interruption.
- **Comprehensive CI/CD**: Cross-platform matrix testing on Linux, macOS, and Windows with automated binary releases.
