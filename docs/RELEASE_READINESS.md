# Release Readiness — v0.1.0

| Gate | Status | Evidence |
|---|---|---|
| Memory database works; persists across restarts | ✅ | storage/memory tests, `records_persist_across_reopen` |
| Search correct; project isolation | ✅ | `search_filters_and_project_isolation` |
| Provenance preserved | ✅ | memory/MCP protocol tests; live Codex provenance |
| Context budgets enforced | ✅ | `brief_respects_budget_at_every_size`, handoff budget tests |
| Git freshness tracking | ✅ | 12 real-git scenario tests |
| Claude Code adapter implemented and tested | ✅ contract · ⚠️ live partial | hooks executed live; model-side unverified (CLI not signed in) |
| Codex adapter implemented and tested | ✅ contract · ✅ live MCP · ⚠️ hooks pending trust | docs/COMPATIBILITY.md |
| Antigravity CLI adapter | ✅ contract · ✅ live | hooks, context injection, MCP verified with agy |
| Antigravity IDE/App adapter | ✅ contract · ⚠️ not live-tested | shared documented config; GUI not automatable here |
| MCP server protocol tested | ✅ | real rmcp client against the built binary |
| Cross-agent handoff tested | ✅ | `claude_to_codex_to_antigravity_handoff_flow` |
| Integration install/removal tested | ✅ | installer tests incl. byte-exact restore, backup restore, clean uninstall |
| UI launches, navigation, every action real | ✅ | 14 UI E2E checks against the real app |
| Settings persist | ✅ | UI E2E + settings tests |
| No committed secrets / personal data / transcripts | ✅ | pattern scan of tree and history |
| No telemetry / network exposure | ✅ | stdio MCP; no network code |
| No dangerous config overwrites | ✅ | plans verify `before`, backups, rollback, invalid files refused |
| Production build, installer exists, binaries packaged | ✅ | NSIS installer with CLI sidecar |
| CI passes | ⏳ | checked after push |
| Code signing | ❌ | installer is unsigned (disclosed in README and release notes) |
