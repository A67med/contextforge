# Architecture Decision Log

Short, dated records of decisions that shape the codebase. Newest last.

## D1 — Single Rust binary for CLI, MCP server and hooks (2026-11-12)
`contextforge.exe` provides `contextforge mcp` (stdio MCP server), `contextforge hook <provider> <event>`
(lifecycle hooks) and the human CLI. Agents spawn it on demand, so no background service is needed and the
desktop app can be closed. Startup cost is a few milliseconds, which keeps hook latency low.

## D2 — SQLite (bundled) with WAL, FTS5 and `BEGIN IMMEDIATE` writes (2026-11-12)
Every process (desktop app, CLI, each MCP server, each hook) opens its own connection. WAL allows concurrent
readers with one writer; `busy_timeout` plus immediate transactions avoid lock-upgrade deadlocks. Migrations are
versioned SQL files applied transactionally. FTS5 (external-content table kept in sync by triggers) powers search.

## D3 — Verification state is categorical, never a numeric score (2026-11-12)
`certainty` ∈ {verified, decided, unverified, hypothesis}; `freshness` ∈ {current, potentially_stale, needs_review,
superseded}. `verified` requires at least one evidence item; requests that claim `verified` without evidence are
downgraded to `unverified` and the response says so.

## D4 — Freshness is recomputed from snapshots, not sticky flags (2026-11-12)
When a memory is created or revalidated, ContextForge stores the SHA-256 of each referenced file
(line endings normalised) and the HEAD commit. A freshness check compares the current working tree with that
snapshot: modified → potentially stale, deleted → needs review, renamed (via `git diff -M`) → reference follows the
rename, missing commit → needs review. Returning to a matching state (e.g. switching back to a branch) clears the
automatic flag. Only explicit actions (revalidate, supersede, edit) change the snapshot. Nothing is deleted
automatically. Decisions and conventions are only flagged on deletion or substantial (≥50 %) change.

## D5 — Git via the `git` CLI with timeouts (2026-11-12)
Avoids a C dependency (libgit2) and matches the user's own git behaviour (worktrees, config, safe.directory).
All calls are read-only, have a timeout, and never commit/reset/clean the user's repository.

## D6 — Provider adapters normalise to one internal event model (2026-11-12)
`crates/integrations` parses each provider's documented payload (snake_case for Claude/Codex, camelCase for
Antigravity) into `HookEvent`, and renders provider-specific JSON output. Antigravity's App/IDE/CLI surfaces share
config files and payload schema; the surface is derived from `transcriptPath`.

## D7 — Config installation is surgical, previewable and reversible (2026-11-12)
Installers compute a plan (exact file diffs and external commands), back up every file before writing, write
atomically (temp file + rename), roll back all files on failure, and uninstall only ContextForge's own entries so
later user edits survive. Unparseable config files are never overwritten. Claude Code MCP registration uses the
official `claude mcp add/remove` CLI because `~/.claude.json` is continuously rewritten by Claude Code itself.

## D8 — Codex hook trust is the user's decision (2026-11-12)
ContextForge writes `~/.codex/hooks.json` but never writes `[hooks.state]` trust entries and never passes
`--dangerously-bypass-hook-trust`. The Integration Manager detects pending review (read-only) and tells the user to
open `/hooks` or Settings → Hooks.

## D9 — Hooks never block agents (2026-11-12)
Hook commands always exit 0, cap stdin size, use short SQLite busy timeouts, and fall back to the provider's no-op
response on any internal error (errors go to the log file, never stdout). Unregistered projects are a fast no-op.

## D10 — Memory is untrusted data (2026-11-12)
Injected context is wrapped in a delimited block that labels it as data, strips control characters, neutralises
the delimiter, truncates items, and withholds the body of memories containing instruction-like text until a user
reviews them. Secrets are redacted before anything is stored. Prompts are used transiently for retrieval and are
not stored unless the user enables it.

## D11 — Desktop app talks to the core in-process (2026-11-12)
The Tauri app links `contextforge-core` and `contextforge-integrations` directly (no IPC to a daemon). It polls
SQLite `PRAGMA data_version` to refresh views when hooks or MCP servers write from other processes.
