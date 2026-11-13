# Test Results

Environment: Windows 11 (10.0.26300), Rust 1.99.1, Git 2.56, Claude Code 2.1.306, codex-cli 0.167.0, agy 1.5.5,
Antigravity desktop 2.31.0.

## Automated suites (`cargo test --workspace`) — 2026-11-13

| Suite | Tests | Result |
|---|---|---|
| core unit | 38 | pass |
| core integration (memory, context, freshness with real git, capture, storage/concurrency) | 42 | pass |
| integrations (adapters contract tests, installers, plans/backups/rollback) | 34 | pass |
| mcp unit | 2 | pass |
| cli: MCP protocol (real rmcp client ↔ built `contextforge mcp`) | 3 | pass |
| cli: end-to-end cross-agent flow, hostile payloads, hook latency | 3 | pass |

`cargo clippy --workspace --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.

## Live provider tests (scripts/live-tests) — 2026-11-13

All runs are isolated (throwaway repo and data dir, per-run config); the user's agent configuration is not modified.

| Provider | What ran | Result |
|---|---|---|
| Claude Code 2.1.306 | `claude -p` with `--settings` (exec-form hooks) and `--strict-mcp-config` | **Hooks executed by the real binary** (SessionStart and SessionEnd recorded). Model turn **not run**: the standalone CLI is not signed in on this machine (`claude auth status` → loggedIn false). Context injection and MCP calls from Claude are therefore **unverified live** (covered by contract tests). |
| Codex 0.167.0 | `codex exec` with per-run `-c mcp_servers.contextforge…` | **PASS**: agent called `search_memory` (found marker) and `record_task`; provenance `source=codex`, client `codex-mcp-client 0.167.0`. Hooks not executed live: Codex requires the user's trust review (`/hooks`), which ContextForge does not bypass. |
| Antigravity CLI 1.5.5 | `agy -p` with workspace `.agents/hooks.json` + `.agents/mcp_config.json`, `--settings` allowing only `mcp(contextforge/*)` | **PASS**: model quoted the marker from SessionStart `injectSteps` without tools; PreInvocation and Stop fired; MCP `record_task` succeeded; session attributed to Antigravity CLI. Exe path with spaces: PASS after switching to 8.3 short paths. |
| Antigravity App/IDE 2.31.0 | — | **Not run live** (GUI only). Uses the same documented config files and payload schema as the CLI. |

### Findings from live testing (fixed)
* Antigravity runs hook commands through `cmd.exe` (probe: `%COMSPEC%` expands, `$PSVersionTable` does not) in a way that cannot start a **quoted** executable path; `call "…"` and double-wrapping also fail. Fix: use the 8.3 short path when the path has spaces; warn if short names are unavailable.
* Antigravity's MCP client did not identify itself; MCP writes are now attributed to the agent session active in the project (recorded in metadata).
* Hooks now drain oversized stdin instead of stopping at the cap, so agents never see a broken pipe.

### Measured hook latency (debug build, warm)
Antigravity live run: SessionStart 171–228 ms (includes git status + freshness), PreInvocation 7–57 ms, Stop 200–500 ms.
