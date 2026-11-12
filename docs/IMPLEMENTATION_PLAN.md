# Implementation Plan

| Phase | Scope | Exit condition |
|---|---|---|
| 1 Research & architecture | Verify provider docs, design schema and components | `INTEGRATION_RESEARCH.md`, `DECISIONS.md`, `ARCHITECTURE.md` written |
| 2 Core backend | `crates/core`: storage, migrations, memory model, search, provenance, dedupe, redaction, project identity | Core unit + integration tests pass |
| 3 MCP & CLI | `crates/mcp` (rmcp stdio server), `crates/cli` (`contextforge` binary), context retrieval, handoff | Real MCP client test passes against the built binary |
| 4 Agent integrations | `crates/integrations`: Claude Code, Codex, Antigravity adapters, installers, backups, detection | Contract tests pass; live tests run where providers are installed |
| 5 Git intelligence | Snapshots, freshness evaluation, renames, worktrees, branch switches, rebases | Git scenario tests pass without corrupting history |
| 6 Desktop app | `apps/desktop` Tauri 2 + React + TS + Tailwind; all screens on real data | Every visible action works against the real backend |
| 7 Hardening & QA | Security review, concurrency, failure recovery, docs vs. behaviour | Full suite green, no known critical defects |
| 8 Packaging | NSIS installer with bundled CLI sidecar | Installer builds and installs; integrations reference installed binary |
| 9 Publication | Public GitHub repo, CI, v0.1.0 release with installer | Release assets published after checks pass |

## Workspace layout

```
crates/core          memory engine (SQLite, search, freshness, context, handoff, redaction, git)
crates/integrations  provider adapters, config installers, detection, diagnostics
crates/mcp           MCP stdio server (rmcp)
crates/cli           `contextforge` binary (CLI + `mcp` + `hook` subcommands) and end-to-end tests
apps/desktop         Tauri 2 desktop app (React + TypeScript + Tailwind)
docs/                architecture, decisions, progress, test results, research
scripts/             build, packaging and live-provider test scripts
```
