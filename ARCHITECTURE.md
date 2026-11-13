# ContextForge Architecture

ContextForge gives Claude Code, OpenAI Codex and Google Antigravity one shared, local, provenance-aware project
memory. There is no daemon: agents launch the `contextforge` executable for lifecycle hooks and as a stdio MCP
server, and every process opens the same SQLite database.

```
 Claude Code ──┐ hooks (exec form)          ┌── contextforge hook claude <event>
 Codex ────────┼ hooks (cmd.exe /C "…")  ───┼── contextforge hook codex <event>        ┐
 Antigravity ──┘ hooks (cmd.exe, short path)└── contextforge hook antigravity <event>  │  crates/integrations
                                                                                        │  adapters → HookEvent
 any agent ──── MCP over stdio ──────────────── contextforge mcp  (crates/mcp, rmcp)    │
                                                                                        ▼
 ContextForge desktop (Tauri) ── in-process ──► crates/core  ◄── contextforge CLI (crates/cli)
                                                  │
                                       SQLite (WAL, FTS5) in %LOCALAPPDATA%\ContextForge
```

## Components

| Crate / app | Responsibility |
|---|---|
| `crates/core` | Storage (migrations, WAL, immediate transactions), memory model, de-duplication, revisions, evidence, links, FTS5 search, project identity (incl. worktrees), Git inspection, freshness, budgeted context briefs, handoffs, hook-event capture, redaction, export/import, backup/restore, statistics. |
| `crates/integrations` | Provider adapters (payload parsing + output rendering), configuration installers with previewable plans, backups, atomic writes, rollback and surgical uninstall; agent detection; live diagnostics (MCP handshake, hook execution). |
| `crates/mcp` | MCP server (official Rust SDK `rmcp`), 14 tools, client-identity provenance, project inference. |
| `crates/cli` | `contextforge` binary: CLI, `hook` runner, `mcp` server entry. Protocol and end-to-end tests. |
| `apps/desktop` | Tauri 2 + React + TypeScript + Tailwind desktop app; Rust commands call the core directly. |

## Data model

* **projects** / **project_roots** — a project is a folder (Git top-level when inside a repository). Linked worktrees
  share the project through the Git common directory. Paths are matched with normalised keys (case-insensitive on
  Windows).
* **memories** — `kind` (decision, convention, fact, task, bug, note, command, research) × lifecycle `status`
  (tasks: pending/in_progress/completed/cancelled; bugs: open/resolved/wont_fix; research: open/resolved). The ten
  required categories (decisions, conventions, facts, completed tasks, pending tasks, known bugs, resolved bugs,
  notes, commands/environment, research/hypotheses) are derived from kind + status.
  * `certainty`: verified (requires evidence), decided, unverified, hypothesis — never a numeric score.
  * `freshness`: current, potentially_stale, needs_review, superseded — derived from unresolved findings.
  * provenance: `source_agent`, `source_session_id`, `source_origin` (mcp/hook/cli/ui/import/auto), MCP client
    identity in metadata, origin branch, `validated_commit`, `last_validated_at`.
  * `user_locked`: user-written memories cannot be rewritten by agents; agents may change status, add evidence or
    propose a replacement (which flags the original for review).
* **memory_files** (path + SHA-256 snapshot, line endings normalised; never computed for sensitive files),
  **memory_commits**, **memory_evidence**, **memory_links**, **memory_revisions** (full history),
  **freshness_findings** (why a memory is flagged; resolved findings are kept).
* **sessions**, **events**, **session_files**, **session_injections** (which memories a session already received),
  **handoffs**, **settings**, **agent_activity**.
* **memories_fts** — FTS5 external-content index keyed on an explicit integer `seq` (stable across VACUUM).

## Key flows

**Session start.** The adapter parses the provider payload → project resolution (fast path: registered roots;
slow path: Git common dir for new worktrees) → session upsert → one `git status` shared by the freshness check and
the brief → budgeted brief (pinned → open tasks → open bugs → decisions/conventions → recent completions → commands
→ facts/questions → possibly stale) → injected via the provider's mechanism → injected memory ids recorded.

**Prompt.** Keywords from the prompt (never stored by default) → FTS5 OR query → memories this session has not
seen → small context block. Antigravity has no prompt event; its `PreInvocation` hook fires before every model call,
so a changed `lastUserInput` hash marks a new prompt.

**Tool use.** File edits (Claude `Write/Edit/…`, Codex `apply_patch` envelopes, Antigravity `write_to_file/…`) and
commands are recorded, redacted. Commands the provider reports as successful and that look like build/test/lint
commands become verified `command` memories (de-duplicated). Codex does not report exit codes, so Codex commands are
recorded as "result unknown".

**Turn end.** Redacted summary, commits made during the session, and snapshot settling for memories the same session
recorded (so its own later edits do not flag them).

**Freshness.** For each memory with references: unchanged → current; modified → potentially stale (decisions and
conventions only for ≥50 % change); deleted → needs review; renamed (git `-M`) → reference follows; referenced
commit missing (rewritten history) → needs review. Re-evaluated from snapshots each time, so switching back to a
branch clears automatic flags. Content is never modified or deleted by a check.

## Concurrency and failure handling

* WAL; `BEGIN IMMEDIATE` for writes; busy timeouts (5 s interactive, 1.5 s hooks); migrations are idempotent under
  concurrent first-open.
* Hooks read bounded stdin (drain beyond 8 MiB), catch panics, and always exit 0 with the provider's documented
  no-op output; logs go to `%LOCALAPPDATA%\ContextForge\logs` (never stdout).
* Git calls have timeouts and `GIT_OPTIONAL_LOCKS=0`; ContextForge never commits, resets, cleans or checks out.
* Configuration writes: plan → verify unchanged → backup → atomic write → rollback on any failure.

## Security model

Local only; no network access or telemetry. MCP is stdio only (no listening sockets). Secrets are redacted before
storage; sensitive files are referenced by path only. Memory content is treated as untrusted data: injected inside a
delimited block labelled as data, control characters stripped, delimiters neutralised, and bodies of memories with
instruction-like text withheld until reviewed. No tool executes code. Codex hook trust is left to the user.

## Extending to another agent

Add an adapter module (`parse` → `HookEvent`, `render` → provider output), an installer (`plan`/`status`), a
detection entry, and an `AgentKind`. The core, MCP server and UI need no changes.
