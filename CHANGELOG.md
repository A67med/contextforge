# Changelog

All notable changes are documented here. The project follows [Semantic Versioning](https://semver.org/).

## [0.1.0] — 2026-11-13
First public release (pre-1.0).

### Added
- Local-first memory engine: SQLite (WAL) + FTS5, structured memories (decision, convention, fact, task, bug, note,
  command, research) with lifecycle status, categorical certainty and freshness, revisions, evidence, file/commit
  references, links, de-duplication and supersession.
- Git-aware freshness: snapshots per memory; modified/deleted/renamed files and rewritten history are detected and
  explained; reversible across branch switches; worktrees map to one project.
- Budgeted, deterministic startup briefs; prompt-relevant retrieval without storing prompts; structured handoff briefs.
- Adapters and installers for Claude Code, OpenAI Codex and Google Antigravity (App, IDE, CLI) with previewable plans,
  backups, atomic writes, rollback, surgical uninstall and backup restore. Codex hook trust is detected, never written.
- MCP server (stdio, 14 tools) built on the official Rust SDK.
- `contextforge` CLI, lifecycle hook runner and doctor.
- Desktop app (Tauri 2): Overview, Projects, Memory Explorer, Session Timeline, Handoff Center, Integrations, Settings,
  command palette, dark and light themes.
- Secret redaction, sensitive-file exclusions and prompt-injection safeguards.
- Windows NSIS installer (per-user, unsigned).
