# Integration Research (verified 2026-11-12)

This file records what the official documentation (and, for Codex, the open-source
implementation) says about each provider's extension points. Every adapter in
`crates/integrations` is built against these facts, not against assumptions.
Field names are reproduced exactly; prose is summarized.

Installed versions on the development machine at research time:
Claude Code `2.1.306`, codex-cli `0.167.0` (bundled with the Codex desktop app), Antigravity CLI `agy 1.5.5`,
Antigravity IDE (Electron app in `%LOCALAPPDATA%\Programs\Antigravity`).

## Claude Code

Sources: <https://code.claude.com/docs/en/hooks>, <https://code.claude.com/docs/en/mcp>,
<https://code.claude.com/docs/en/settings>.

| Topic | Verified behaviour |
|---|---|
| Hook config files | `~/.claude/settings.json` (user), `.claude/settings.json` (project, shared), `.claude/settings.local.json` (project, personal). Hook arrays from all levels merge. `--settings <file>` adds a layer for one run. |
| Hook shape | `hooks.<Event>[] = { matcher?, hooks: [ { type: "command", command, args?, timeout?, ... } ] }`. With `args` set the handler runs in **exec form** (no shell); `command` must be a real executable. |
| Events used | `SessionStart` (matcher on `source`: startup/resume/clear/compact/fork), `UserPromptSubmit`, `PostToolUse` (matcher on tool name), `Stop`, `SessionEnd`. |
| Common input | snake_case JSON on stdin: `session_id`, `transcript_path`, `cwd`, `hook_event_name`, `permission_mode`, … Event fields: `source`, `prompt`, `tool_name`, `tool_input`, `tool_response`, `tool_use_id`, `last_assistant_message`, `stop_hook_active`, `reason`. |
| Context injection | `{"hookSpecificOutput":{"hookEventName":"SessionStart"|"UserPromptSubmit","additionalContext":"…"}}`. Plain stdout on exit 0 is also added for SessionStart/UserPromptSubmit. Cap: 10,000 characters. |
| Exit codes | 0 = success; 2 = blocking error (not used by ContextForge); other = non-blocking error. |
| Timeouts | Default 600 s for command hooks; `SessionEnd` budget defaults to 1.5 s. |
| Trust | Interactive sessions withhold hooks until workspace trust is accepted. `claude -p` treats the folder as trusted. |
| MCP | `claude mcp add --scope user|project|local <name> -- <command> [args…]`; user/local scope stored in `~/.claude.json`, project scope in `.mcp.json` (`{"mcpServers":{…}}`). `--mcp-config` + `--strict-mcp-config` load servers for one run. Tool names: `mcp__<server>__<tool>`. |

## OpenAI Codex

Sources: <https://learn.chatgpt.com/docs/hooks> (redirect target of developers.openai.com/codex/hooks),
<https://learn.chatgpt.com/docs/extend/mcp>, <https://learn.chatgpt.com/docs/config-file/config-reference>,
and `openai/codex` source (`codex-rs/hooks/src/engine/command_runner.rs`, `discovery.rs`, `codex-rs/config/src/hook_config.rs`).

| Topic | Verified behaviour |
|---|---|
| Hook config files | `~/.codex/hooks.json`, inline `[hooks]` in `~/.codex/config.toml`, and the same two under `<repo>/.codex/` (project layer loads only when the project is trusted). All sources run; one layer with both forms merges with a startup warning. |
| Hook shape | `{"description"?, "hooks": { "<Event>": [ { "matcher"?, "hooks": [ { "type":"command", "command", "commandWindows"?, "timeout"?, "statusMessage"?, "additionalContextLimit"?, "async"? } ] } ] } }` |
| Feature flag | Hooks are on by default (`[features] hooks`); `codex features list` on the dev machine reports `hooks stable true`. |
| Events used | `SessionStart` (matcher on `source`), `UserPromptSubmit`, `PostToolUse` (tool names `Bash`, `apply_patch`, `mcp__…`), `Stop`, `SessionEnd`. |
| Input | snake_case: `session_id`, `transcript_path`, `cwd`, `hook_event_name`, `model`, `turn_id`, `permission_mode`, plus `source`, `prompt`, `tool_name`, `tool_use_id`, `tool_input` (`tool_input.command` for Bash and apply_patch), `tool_response`, `last_assistant_message`, `reason`. |
| Context injection | `hookSpecificOutput.additionalContext` (SessionStart, UserPromptSubmit, PostToolUse). Default model-visible limit ≈2,500 tokens per hook (`additionalContextLimit`). |
| Output rules | `Stop` must print JSON when exiting 0 (ContextForge prints `{}`); `SessionEnd` is advisory, synchronous, default timeout 1 s (max 3 s). |
| Windows execution | Source: commands run through `%COMSPEC%` (cmd.exe) as `/C "<command>"`; `commandWindows` overrides `command` on Windows. |
| **Trust review** | Non-managed hooks run only after the user reviews and trusts the exact definition (hash) in `/hooks` (CLI) or Settings → Hooks (desktop). Trust is persisted as `[hooks.state."<source>:<event>:<group>:<handler>"] trusted_hash = …` in `config.toml`. **ContextForge never writes trust state and never uses `--dangerously-bypass-hook-trust`.** |
| MCP | `[mcp_servers.<name>] command, args, env, cwd, startup_timeout_sec (10), tool_timeout_sec (60), enabled, …`; `codex mcp add <name> -- <cmd>` / `list` / `get` / `remove`. |

## Google Antigravity (App, IDE, CLI)

Sources: <https://antigravity.google/docs/hooks>, <https://antigravity.google/docs/mcp>,
<https://antigravity.google/docs/permissions>, `agy --help`, `agy mcp add --help`.

| Topic | Verified behaviour |
|---|---|
| Hook config files | **Shared by App, IDE and CLI:** workspace `.agents/hooks.json`, global `~/.gemini/config/hooks.json` (CLI also loads plugin `hooks.json`; hooks in `~/.gemini/antigravity-cli/settings.json` are *not* loaded). |
| Hook shape | Object keyed by hook name: `{"<name>": {"enabled"?: bool, "SessionStart": [handler…], "PreInvocation": [handler…], "Stop": [handler…], "PostToolUse": [ {"matcher": "<regex>", "hooks": [handler…]} ] } }`. Handler: `{ "type": "command", "command": "<shell command>", "timeout": <seconds, default 30> }`. Non-tool events take handlers directly (matcher ignored). |
| Events | `SessionStart`, `PreToolUse`, `PostToolUse`, `PreInvocation`, `PostInvocation`, `Stop`. There is **no** SessionEnd and **no** UserPromptSubmit. |
| Input | camelCase JSON on stdin: `conversationId`, `workspacePaths[]`, `transcriptPath`, `artifactDirectoryPath`, `modelName`, `lastUserInput`; tool events add `toolCall {name,args}`, `stepIdx`, `error`; PreInvocation adds `invocationNum`, `initialNumSteps`; Stop adds `executionNum`, `terminationReason`, `error`, `fullyIdle`. |
| Surface detection | `transcriptPath` lives under `~/.gemini/antigravity` (App), `~/.gemini/antigravity-ide` (IDE) or `~/.gemini/antigravity-cli` (CLI). ContextForge uses this to attribute provenance to the right surface. |
| Context injection | `{"injectSteps":[{"systemMessage":{"systemMessage":"…"}}]}` from SessionStart; PreInvocation may return `{"injectSteps":[{"ephemeralMessage":"…"}]}`. |
| Required outputs | `PostToolUse` → `{}`; `Stop` → `decision` is required ("continue" keeps running; any other value allows the stop — ContextForge returns `{"decision":"stop"}`). |
| MCP | **Shared by App, IDE and CLI:** `~/.gemini/config/mcp_config.json` (global) and `<workspace>/.agents/mcp_config.json`. `{"mcpServers":{"<name>":{"command","args","env","cwd","disabled","enabledTools","disabledTools","tools":{"<tool>":{"eager":bool}},"timeoutSeconds"}}}`. Tools are lazy-loaded by default and invoked via the built-in `call_mcp_tool` (`ServerName`, `ToolName`, `Arguments`). `agy mcp add/remove/list/enable/disable` exists in CLI 1.5.5. |
| Permissions | Unconfigured MCP tools run in *Ask* mode. CLI rules live in `~/.gemini/antigravity-cli/settings.json` → `permissions.allow|ask|deny` with `mcp(server/*)` patterns; `agy --settings '<json>'` applies settings for one session only. |
| Not documented | Hook exit-code semantics, the shell used on Windows, and hook trust prompts. These are measured by live tests (see `docs/TEST_RESULTS.md`). |
