# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.2.0] - 2026-09-06

### Highlights
- **Unified Adaptive `/shake`**: Consolidated multiple skill variations into a single, intelligent `/shake` command that automatically selects Standard or Deep compaction based on conversation depth and token bloat.
- **Zero-Dependency Native Rust Engine**: Replaced Python prototype scripts with a standalone, memory-safe native Rust binary (`shake-prune`) featuring sub-millisecond execution.
- **Cross-Platform Prebuilt Distribution**: Automated multi-platform GitHub Releases with cryptographically verified binaries for Linux (x86_64, aarch64 musl), macOS (Apple Silicon, Intel), and Windows (x86_64).
- **Streamlined Installers & Clean Git Tree**: Removed bundled binaries from git tracking, reducing repository bloat, while updating `install.sh` and `install.ps1` to download release assets with SHA-256 verification and CI override support.

### Added
- **Native Rust Engine (`shake-prune`)**:
  - High-throughput streaming JSONL parser and in-place compaction engine.
  - Advisory file locking via `fs2` preventing concurrent writer corruption.
  - Inode-preserving atomic truncate-and-write pattern.
  - Pre-commit fingerprint validation to detect uncooperative concurrent writes prior to truncation.
  - Intent journaling via `.shake_in_progress` enabling automatic crash recovery.
  - Multi-phase watchdog deadline enforcement across initialization, locking, archive sync, parsing, compaction loops, and commit phases.
  - Fail-open hook architecture: `--hook` exits with status `0` and `{}` upon any error or lock contention, ensuring zero agent interruption.
- **Unified CLI Utility Suite**:
  - `shake-prune run <transcript> [output] [--mode auto|standard|deep]`: Adaptive transcript compaction.
  - `shake-prune preview <transcript> [--json]`: Non-destructive dry-run showing reduction metrics and continuity anchors.
  - `shake-prune status <transcript> [--json]`: Real-time inspection of token volume, archive health, and compaction recommendations.
  - `shake-prune undo <transcript> [--force]`: Instant rollback from `.jsonl.bak` with `.pre_restore` backup snapshots and anchor cache invalidation.
  - `shake-prune show <transcript> --step N|--line N [--redact] [--pretty]`: Terminal inspection of historical tool runs from `transcript_full.jsonl`.
  - `shake-prune doctor [--json]`: Comprehensive diagnostics verifying hooks, configuration, storage paths, permissions, and binary integrity.
- **Privacy, Security & Retention**:
  - Configurable regex secret redaction covering OpenAI API keys (`sk-...`), Google API keys (`AIza...`), Slack tokens (`xox...`), and generic credential patterns.
  - Strict path traversal guardrails forbidding arbitrary filesystem access outside workspace and configuration roots.
  - Restrictive file permissions (`0600` on Unix) on metadata, journals, and archives.
  - Master archive (`transcript_full.jsonl`) preservation ensuring an unpruned audit trail is always maintained on disk.
- **Configuration & Documentation**:
  - Multi-tiered configuration support via `shake.toml` and environment variable overrides (`SHAKE_*`).
  - Added canonical configuration template `shake.example.toml` documenting all configuration options.
  - Added `SECURITY.md` detailing threat models, file locking, permissions, and vulnerability reporting.
- **Automated CI/CD Release Pipeline**:
  - GitHub Actions matrix workflow building release binaries across 5 platforms.
  - Automated SHA-256 checksum generation (`SHA256SUMS.txt`) and release asset publishing.

### Changed
- **Installer Simplification**:
  - `install.sh` and `install.ps1` now download prebuilt binaries with SHA-256 integrity verification by default, eliminating local Rust toolchain requirements for end users.
  - Retained local binary detection (`bin/` and `target/release/`) for seamless development and CI testing.
  - Removed committed binaries from repository tracking and added `bin/` to `.gitignore`.
- **Skill Definitions**:
  - Consolidated skill into a single, clean `/shake` definition in `skills/shake/SKILL.md` and repository root `SKILL.md`.
  - Updated `AGENTS.md` and documentation to guide agents on using the unified CLI suite.

### Fixed
- **Windows Concurrency**: Resolved mandatory file lock contention in `count_unpruned_tools` and `run_compaction_pipeline` on Windows.
- **Config Precedence**: Fixed `ShakePrivacyConfig.redact_secrets` deserialization (`Option<bool>`) so explicit `false` values in `[privacy]` are not overwritten by legacy `[shake]` defaults.
- **Deep Compaction Thought Window**: User-specified `--thought-window` arguments are now preserved in `apply_deep` rather than unconditionally falling back to 20.
- **Hook Artifact Link Integrity**: Hook mode now writes canonical `shake_latest.md` (and maintains timestamped archives when retention > 0), ensuring continuity anchor links never 404.
- **Uninstall Flow**: Fixed unclosed uninstall block in `install.ps1` and silenced non-Unix warnings on Windows.

---

## [0.1.10] - 2026-09-04
- Baseline release of context compaction tooling with initial Python scripts and experimental hook handlers.
