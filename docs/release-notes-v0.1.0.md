First public release of **ContextForge** — one local, provenance-aware memory shared by Claude Code, OpenAI Codex and Google Antigravity. Pre-1.0: expect rough edges.

## Highlights
- **Shared memory across agents**: decisions, conventions, facts, tasks, bugs, notes, commands and research, with lifecycle status, evidence, file/commit references, revisions and supersession.
- **Automatic context**: a budgeted brief (~1,200 tokens by default) at session start and prompt-relevant memories on each new prompt, delivered through each agent's own hooks.
- **MCP server** (stdio, 14 tools) for search, recording decisions/tasks/bugs, handoffs and memory health.
- **Git-aware freshness**: modified, deleted and renamed files and rewritten history are detected and explained; nothing is deleted automatically.
- **Integration manager**: install, repair, uninstall and validate integrations with previewed diffs, backups, atomic writes and rollback.
- **Privacy**: local only, no telemetry, secret redaction, sensitive-file exclusions, prompts not stored, prompt-injection safeguards.
- **Desktop app** (Tauri 2) and **CLI** (`contextforge`).

## Install
Download `ContextForge_0.1.0_x64-setup.exe`, verify it against `SHA256SUMS.txt`, and run it (per-user install, no admin rights). Then add a project and connect your agents in **Integrations**.

> ⚠️ **The installer is not code-signed.** Windows SmartScreen may warn ("Windows protected your PC"). Verify the SHA-256 checksum, then choose *More info → Run anyway*.

## Verification status
| Agent | Status in this release |
|---|---|
| Antigravity CLI | Live-verified: hooks, context injection and MCP tools |
| Codex | Live-verified: MCP tools. Hooks require you to trust them in `/hooks` (never bypassed) |
| Claude Code | Hooks executed live; model-side context/MCP not live-verified (contract-tested) |
| Antigravity App / IDE | Same documented configuration as the CLI; contract-tested, not GUI-tested |

Full details: [docs/COMPATIBILITY.md](https://github.com/A67med/contextforge/blob/main/docs/COMPATIBILITY.md) · [docs/TEST_RESULTS.md](https://github.com/A67med/contextforge/blob/main/docs/TEST_RESULTS.md)

## Known limitations
- Windows x64 only (the core also runs on Linux; no desktop builds for macOS/Linux yet).
- Codex does not report shell exit codes to hooks, so Codex command outcomes are recorded as unknown.
- Antigravity cannot start hook commands from paths with spaces unless 8.3 short names exist (the installer warns).
- Unsigned installer.
