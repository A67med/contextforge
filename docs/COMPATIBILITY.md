# Integration Compatibility Matrix

Tested on Windows 11 with Claude Code 2.1.306, codex-cli 0.167.0 (Codex desktop app bundle), Antigravity CLI
(`agy`) 1.5.5 and the Antigravity desktop app 2.31.0. "Contract" = automated tests feeding the provider's documented
payloads through the real `contextforge` binary. "Live" = the real agent executable ran against ContextForge.

| Capability | Claude Code | Codex | Antigravity CLI | Antigravity App / IDE |
|---|---|---|---|---|
| MCP config location | `claude mcp add --scope user\|local` (`~/.claude.json`) | `~/.codex/config.toml` `[mcp_servers.contextforge]` | `~/.gemini/config/mcp_config.json` or `.agents/mcp_config.json` | same files as CLI |
| Hook config location | `~/.claude/settings.json` or `.claude/settings.local.json` | `~/.codex/hooks.json` or `<repo>/.codex/hooks.json` | `~/.gemini/config/hooks.json` or `.agents/hooks.json` | same files as CLI |
| Events used | SessionStart, UserPromptSubmit, PostToolUse, PostToolUseFailure, Stop, SessionEnd | SessionStart, UserPromptSubmit, PostToolUse, Stop, SessionEnd | SessionStart, PreInvocation, PostToolUse, Stop | same as CLI |
| Startup context | `hookSpecificOutput.additionalContext` | `hookSpecificOutput.additionalContext` | `injectSteps[].systemMessage` | same as CLI |
| Prompt context | UserPromptSubmit | UserPromptSubmit | PreInvocation (new `lastUserInput`) | same as CLI |
| Command outcome | success / failure (PostToolUseFailure) | **unknown** (no exit code in payload) | success / `exit status N` | same as CLI |
| Windows hook execution | exec form (`command` + `args`, no shell) | `cmd.exe /C "…"` (quoted path OK) | `cmd.exe` — quoted paths fail; 8.3 short path used | same as CLI |
| Trust / approval | `-p` runs hooks; interactive sessions need workspace trust | **user must trust hooks in `/hooks`** (never bypassed) | MCP tools ask unless allowed (`mcp(contextforge/*)`) | MCP tools ask unless allowed |
| Install / uninstall / repair | contract-tested | contract-tested (config.toml restored byte-for-byte) | contract-tested; workspace install used live | shared config, contract-tested |
| Contract tests | pass | pass | pass | pass (surface detected from `transcriptPath`) |
| **Live: hooks executed** | **yes** (SessionStart, SessionEnd) | not run — awaiting user trust review | **yes** (SessionStart, PreInvocation, Stop) | not run (GUI only) |
| **Live: context reached the model** | unverified — standalone CLI not signed in | not run (hooks) | **yes** (model quoted injected marker) | not run |
| **Live: MCP tool calls** | unverified — CLI not signed in | **yes** (search_memory, record_task) | **yes** (record_task) | not run |
| Live: provenance | — | `source=codex`, client `codex-mcp-client 0.167.0` | `source=antigravity_cli` (via active session; client sends no name) | — |

## Known limitations
* Antigravity's App and IDE were not exercised live (no automation surface). They read the same documented
  configuration files and send the same documented payloads; the adapter attributes them by `transcriptPath`.
* Codex hooks run only after the user trusts them. ContextForge shows the trust state (read-only) and never writes
  `[hooks.state]` or passes `--dangerously-bypass-hook-trust`.
* If the ContextForge path contains spaces and the volume has 8.3 names disabled, Antigravity cannot start the hook
  command; the installer warns. The default per-user install path has no spaces unless the Windows user name does.
