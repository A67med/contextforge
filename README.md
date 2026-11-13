<p align="center">
  <img src="assets/logo.svg" width="96" alt="ContextForge logo" />
</p>

<h1 align="center">ContextForge</h1>
<p align="center"><b>One memory. Every coding agent.</b></p>
<p align="center">
  Local-first, provenance-aware project memory shared by <b>Claude Code</b>, <b>OpenAI Codex</b> and <b>Google Antigravity</b>.
</p>
<p align="center">
  <a href="https://github.com/A67med/contextforge/actions/workflows/ci.yml"><img src="https://github.com/A67med/contextforge/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <img src="https://img.shields.io/badge/platform-Windows%2010%2F11%20x64-0078d4" alt="Windows" />
  <img src="https://img.shields.io/badge/license-MIT-green" alt="MIT" />
  <img src="https://img.shields.io/badge/status-0.1%20(pre--release)-orange" alt="0.1" />
</p>

---

Switching between coding agents loses context: each one re-reads the codebase, re-discovers decisions and forgets
what the previous one was doing. **ContextForge** records what matters — architectural decisions, conventions,
verified facts, tasks, bugs, commands and research — in one local database, and hands it to whichever agent you open
next:

* **Automatically**, through each agent's own lifecycle hooks: a compact, budgeted brief (~1,200 tokens by default) at
  session start, plus prompt-relevant memories the session has not seen yet.
* **On demand**, through an MCP server with 14 tools (search, get context, record decisions/tasks/bugs, supersede,
  revalidate, handoff, health).
* **Honestly**: every memory carries provenance (which agent, which session, what evidence), a categorical certainty
  (verified / decided / unverified / hypothesis) and a freshness state derived from Git — so outdated knowledge is
  flagged with the reason, not silently trusted.

No account, no cloud, no telemetry, no paid API. Everything stays on your machine.

![Overview](docs/screenshots/overview.png)

## Features

| | |
|---|---|
| 🧠 **Structured memory** | Decisions, conventions, facts, tasks, bugs, notes, commands and research with lifecycle status, tags, file and commit references, evidence, links, full revision history, de-duplication and supersession. |
| 🔁 **Cross-agent handoff** | Claude Code → Codex → Antigravity → Claude Code. Each session starts with the latest decisions, open work, last activity and possibly-stale items; structured handoff briefs on request. |
| 🌿 **Git-aware freshness** | Snapshots of referenced files. Modified → *possibly stale*, deleted → *needs review*, renamed → reference follows, rewritten history → *needs review*. Reversible across branch switches; worktrees share one project. Nothing is deleted automatically. |
| ✅ **No false certainty** | “Verified” requires evidence (a command that ran, a commit, a file). Successful build/test commands reported by agents become verified command memories automatically. |
| 🔌 **Integration manager** | Detects installed agents and versions; installs, repairs and removes configuration with a reviewable diff, backups, atomic writes and rollback; validates MCP and hooks live. |
| 🔒 **Privacy by design** | Secrets redacted before storage, sensitive files referenced by path only, prompts not stored by default, memory treated as untrusted data (prompt-injection safeguards), stdio-only MCP. |
| 🖥️ **Native desktop app** | Overview, Projects, Memory Explorer, Session Timeline, Handoff Center, Integrations and Settings — dark and light themes, command palette (Ctrl+K), keyboard navigation (Ctrl+1…7). |
| ⌨️ **CLI** | Everything scriptable: `contextforge project add`, `memory search`, `context`, `handoff`, `freshness`, `integrations install`, `doctor`, export/import, backup/restore. |

<table>
<tr><td><img src="docs/screenshots/memory-explorer.png" alt="Memory Explorer" /></td><td><img src="docs/screenshots/memory-stale.png" alt="Stale memory with reason" /></td></tr>
<tr><td align="center">Memory Explorer with provenance</td><td align="center">Why a memory may be outdated</td></tr>
<tr><td><img src="docs/screenshots/timeline.png" alt="Session timeline" /></td><td><img src="docs/screenshots/handoffs.png" alt="Handoff center" /></td></tr>
<tr><td align="center">Session timeline across agents</td><td align="center">Handoff center</td></tr>
<tr><td><img src="docs/screenshots/integrations.png" alt="Integration manager" /></td><td><img src="docs/screenshots/integration-plan.png" alt="Install plan with diff" /></td></tr>
<tr><td align="center">Integration manager</td><td align="center">Every config change is previewed first</td></tr>
</table>

<sub>Screenshots are of the real application, showing a demo project whose data was produced through the real hook and MCP code paths (`scripts/seed-demo.ps1`).</sub>

## Supported integrations

| Agent | Context at session start | Prompt-time context | MCP tools | Activity capture | Live-verified (v0.1.0) |
|---|---|---|---|---|---|
| **Claude Code** | `SessionStart` → `additionalContext` | `UserPromptSubmit` | `claude mcp add` (user/local) | edits, commands (with outcome), turns, session end | hooks executed by Claude Code 2.1.306; model-side checks pending (CLI was not signed in on the test machine) |
| **OpenAI Codex** (CLI + desktop) | `SessionStart` → `additionalContext` | `UserPromptSubmit` | `~/.codex/config.toml` | `apply_patch` edits, commands (outcome not reported by Codex), turns | MCP tools used by codex-cli 0.167.0; hooks require your trust review in `/hooks` |
| **Google Antigravity** (CLI, App, IDE) | `SessionStart` → `injectSteps` | `PreInvocation` | `~/.gemini/config/mcp_config.json` | edits, commands (with outcome), turns | hooks, context injection and MCP verified with `agy` 1.5.x; App/IDE share the same config (not GUI-tested) |

Details, limits and test evidence: [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md) · [docs/TEST_RESULTS.md](docs/TEST_RESULTS.md) · [docs/INTEGRATION_RESEARCH.md](docs/INTEGRATION_RESEARCH.md)

## Installation

**Requirements:** Windows 10 (1809+) or Windows 11, x64 · WebView2 runtime (preinstalled on Windows 11; the installer
fetches it if missing) · Git on `PATH` (for branch/commit/rename awareness) · at least one of Claude Code, Codex or Antigravity.

1. Download `ContextForge_0.1.0_x64-setup.exe` from the [latest release](https://github.com/A67med/contextforge/releases/latest)
   and verify it against `SHA256SUMS.txt`.
2. Run it. It installs per-user into `%LOCALAPPDATA%\ContextForge` (no administrator rights).

> **The installer is not code-signed.** Windows SmartScreen may show “Windows protected your PC”. Choose *More info →
> Run anyway* only if the checksum matches the release.

Uninstalling removes the app and ContextForge's own entries from your agents' user-level configuration; your memory
database is kept in `%LOCALAPPDATA%\ContextForge` (delete that folder to remove all data).

## Quick start

1. **Add a project** — *Projects → Add project* (or `contextforge project add C:\code\my-app`). Only registered projects
   are recorded.
2. **Connect your agents** — *Integrations → Install* for each agent. Review the exact file changes, then apply.
3. **Work as usual.** Start Claude Code, Codex or Antigravity in the project. Each session receives the project brief;
   agents record decisions, tasks and findings through the ContextForge MCP tools, and hooks capture edits, commands and
   summaries.
4. **Switch agents freely.** The next agent starts with what the previous one did. Use the *Handoff Center* for a
   detailed brief, and the *Memory Explorer* to review, pin, correct or supersede memories.

## Agent setup

The Integration Manager does all of this for you; the commands below are equivalent.

### Claude Code
```powershell
contextforge integrations install claude --yes
```
Adds lifecycle hooks to `~\.claude\settings.json` (exec form: no shell quoting) and registers the MCP server with
`claude mcp add --scope user contextforge`. Use `--project <path>` to install into `.claude\settings.local.json` and
local MCP scope for a single project instead. Restart running sessions.

### OpenAI Codex
```powershell
contextforge integrations install codex --yes
```
Adds hooks to `~\.codex\hooks.json` and `[mcp_servers.contextforge]` to `~\.codex\config.toml` (formatting and other
settings preserved). **Then open Codex and run `/hooks`** (or *Settings → Hooks* in the Codex app) and trust the
ContextForge hooks — Codex skips untrusted hooks, and ContextForge never approves them on your behalf. The
Integration Manager shows when review is still pending.

### Google Antigravity (App, IDE and CLI)
```powershell
contextforge integrations install antigravity --yes
```
Adds a `contextforge` entry to `~\.gemini\config\hooks.json` and `~\.gemini\config\mcp_config.json`, shared by the
Antigravity App, IDE and CLI. Antigravity asks before running MCP tools; to allow only ContextForge's tools, add
`"mcp(contextforge/*)"` to your permission allow list. `--project <path>` installs into the workspace `.agents\` folder.

Check everything with `contextforge doctor` or *Integrations → Validate*.

## Privacy and security

* **Local only.** SQLite database in `%LOCALAPPDATA%\ContextForge`. No telemetry and no network requests; the MCP server
  uses stdio and opens no ports.
* **Opt-in per project**, with per-project and global exclusion patterns.
* **Secrets are redacted** before anything is stored (API keys, tokens, private keys, passwords in URLs/assignments).
  `.env*`, keys, certificates, SSH/AWS/GPG folders, credential files and Terraform state are referenced by path only —
  never read, hashed or stored.
* **Prompts are not stored** by default (used transiently to find relevant memories).
* **Memory is untrusted data**: injected inside a labelled block, sanitized, and memories containing instruction-like
  text are withheld from automatic injection until you review them. No tool executes code.
* **Safe configuration changes**: previewed diffs, backups, atomic writes, rollback, surgical uninstall, one-click
  restore; invalid config files are never overwritten.

See [SECURITY.md](SECURITY.md) to report a vulnerability.

## Architecture

```
agents ──hooks──► contextforge hook <provider> <event> ─┐
agents ──MCP────► contextforge mcp (stdio, rmcp)  ──────┼──► contextforge-core ──► SQLite (WAL + FTS5)
desktop app (Tauri 2 + React) ── in-process ────────────┘        (memories, sessions, freshness, handoffs)
```

A single Rust binary serves hooks, MCP and the CLI; agents start it on demand, so no background service is needed and
the desktop app can stay closed. Provider adapters translate each agent's documented hook schema into one internal
event model. Read [ARCHITECTURE.md](ARCHITECTURE.md) for the data model, flows and security model, and
[docs/DECISIONS.md](docs/DECISIONS.md) for the design decisions.

## Building from source

```powershell
git clone https://github.com/A67med/contextforge
cd contextforge
pwsh scripts/build-installer.ps1        # → target\release\bundle\nsis\ContextForge_0.1.0_x64-setup.exe
```
Requires Rust (MSVC), Node.js 20+ and the Visual Studio C++ build tools. See [DEVELOPMENT.md](DEVELOPMENT.md).

## Testing

```powershell
cargo test --workspace          # 120+ Rust tests: unit, real-git freshness, concurrency, MCP protocol, cross-agent E2E
cd apps/desktop; npm test       # frontend unit tests
node e2e/run.mjs <app.exe> <data-dir>   # UI end-to-end tests against the real app (WebView2 + Playwright over CDP)
pwsh scripts/live-tests/antigravity.ps1 # live tests against installed agents (also claude.ps1, codex.ps1)
```

## Troubleshooting

| Symptom | Fix |
|---|---|
| Agent sessions do not appear in the timeline | The project must be registered; run `contextforge doctor`; for Codex, trust the hooks in `/hooks`. |
| “not a registered ContextForge project” from MCP tools | Register the folder, or pass `project="<repo path>"` in the tool call. |
| Codex shows a hook review warning | Expected until you trust the ContextForge hooks in `/hooks` (or Settings → Hooks). |
| Antigravity asks before every ContextForge tool | Add `"mcp(contextforge/*)"` to your Antigravity permission allow list. |
| Memories marked “possibly stale” | Open the memory to see why; *Revalidate* if still accurate, or *Supersede* it. |
| Integrations show *Outdated* after moving the app | Click *Repair* — only ContextForge's own entries are rewritten. |
| SmartScreen blocks the installer | The installer is unsigned; verify the SHA-256 checksum, then *More info → Run anyway*. |

Logs: `%LOCALAPPDATA%\ContextForge\logs` (also viewable in *Settings*).

## Roadmap

* Code signing for the Windows installer
* macOS and Linux builds (the core is cross-platform and tested on Linux in CI)
* Optional local-model summarization of sessions into *unverified* candidate memories
* More agents (the adapter model is designed for it)
* Symbol-level evidence (track functions, not just files)

## Contributing

Contributions are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md) and the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

[MIT](LICENSE) © ContextForge contributors. Claude Code, Codex and Antigravity are trademarks of their respective
owners; ContextForge is an independent project and is not affiliated with Anthropic, OpenAI or Google.
