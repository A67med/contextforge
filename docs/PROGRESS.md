# Progress

**Current phase:** v0.1.0 released 2026-11-13 — https://github.com/A67med/contextforge/releases/tag/v0.1.0

| Phase | Status |
|---|---|
| 1 Research & architecture | done — docs/INTEGRATION_RESEARCH.md, docs/DECISIONS.md, ARCHITECTURE.md |
| 2 Core backend | done — 80 tests |
| 3 MCP & CLI | done — real-client protocol tests |
| 4 Agent integrations | done — contract tests; live: Codex MCP, Antigravity CLI hooks+context+MCP, Claude hooks |
| 5 Git intelligence | done — renames, deletions, branch switches, rewritten history, worktrees, uncommitted changes |
| 6 Desktop app | done — all screens on real data, 14 UI E2E checks |
| 7 Hardening & QA | done — fmt/clippy/tests green, secret/PII scan clean |
| 8 Packaging | done — NSIS installer built, installed, uninstalled |
| 9 Publication | done — public repo, CI green, v0.1.0 installer built by CI, install-tested, published |

## Open items (need the user)
1. **Claude Code live model checks:** run `claude` and sign in (`/login`), then `pwsh scripts/live-tests/claude.ps1`.
2. **Codex live hook checks:** install the Codex integration, trust the ContextForge hooks in `/hooks`, start a Codex
   session in a registered project, and confirm the session appears in the timeline.
3. **Antigravity App/IDE:** open a registered project in the Antigravity App or IDE and confirm the session appears
   as "Antigravity App"/"Antigravity IDE" in the timeline.

## Next steps after 0.1.0
Code signing, macOS/Linux builds, optional local-model summarization, symbol-level evidence.
