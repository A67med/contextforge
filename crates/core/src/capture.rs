//! Processing of normalised lifecycle-hook events from any provider.
//!
//! Provider adapters (crates/integrations) parse Claude Code / Codex / Antigravity payloads into [`HookEvent`];
//! this module records sessions and activity, keeps snapshots of memories settled, derives verified command
//! memories deterministically, and returns the context to inject (if any).

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::context::BriefOptions;
use crate::memories::{EvidenceInput, NewMemory};
use crate::model::{Actor, AgentKind, Certainty, EventKind, EvidenceKind, MemoryKind, Origin};
use crate::privacy;
use crate::projects::Resolved;
use crate::sessions::{SessionStartInfo, insert_event, touch_agent_activity};
use crate::{Result, Store, git, paths, util};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileAction {
    Create,
    Edit,
    Delete,
    Write,
}

impl FileAction {
    pub fn as_str(self) -> &'static str {
        match self {
            FileAction::Create => "create",
            FileAction::Edit => "edit",
            FileAction::Delete => "delete",
            FileAction::Write => "write",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileTouch {
    pub path: String,
    pub action: FileAction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ToolUse {
    pub tool: String,
    pub files: Vec<FileTouch>,
    pub command: Option<String>,
    /// Some(true) when the provider reports success, Some(false) on failure, None if unknown.
    pub success: Option<bool>,
    pub exit_code: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HookEventKind {
    SessionStart {
        source: Option<String>,
    },
    /// The user submitted a prompt (Claude Code / Codex `UserPromptSubmit`). The text is used transiently.
    PromptSubmitted {
        prompt: String,
    },
    /// Antigravity `PreInvocation`: fires before every model call; a changed `lastUserInput` means a new prompt.
    BeforeModel {
        last_user_input: Option<String>,
        invocation: Option<i64>,
    },
    ToolUsed(ToolUse),
    TurnEnded {
        last_message: Option<String>,
        fully_idle: Option<bool>,
        reason: Option<String>,
    },
    SessionEnded {
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookEvent {
    pub agent: AgentKind,
    pub external_session_id: String,
    pub cwd: Option<PathBuf>,
    pub workspace_paths: Vec<PathBuf>,
    pub model: Option<String>,
    /// Provider event name, e.g. `SessionStart`, `PostToolUse`.
    pub raw_event: String,
    pub kind: HookEventKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct HookOutcome {
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    /// Text to inject into the agent's context, if any.
    pub context: Option<String>,
    /// Why nothing was done (unregistered project, capture disabled, …).
    pub skipped: Option<String>,
}

static BUILD_COMMAND: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)^(?:sudo\s+)?(?:
            cargo\s+(?:build|test|check|clippy|fmt|nextest|bench|doc)\b
          | npm\s+(?:test|ci|install|run\s+[\w:.\-]+)\b
          | pnpm\s+(?:test|install|i|run\s+[\w:.\-]+|build|lint|dev)\b
          | yarn(?:\s+(?:test|install|build|lint|run\s+[\w:.\-]+))?\s*$
          | bun\s+(?:test|install|run\s+[\w:.\-]+)\b
          | npx\s+(?:tsc|vitest|jest|eslint|prettier|playwright|tauri)\b
          | deno\s+(?:task|test|lint|fmt)\b
          | (?:python3?|py)\s+-m\s+(?:pytest|unittest|mypy|ruff|black)\b
          | pytest\b | tox\b | ruff\s+check\b | mypy\b
          | uv\s+(?:run|sync)\b | poetry\s+(?:run|install)\b
          | go\s+(?:build|test|vet)\b
          | make(?:\s+[\w.\-]+)?\s*$ | cmake\s+--build\b | ctest\b
          | dotnet\s+(?:build|test|restore|publish)\b
          | (?:\./)?gradlew(?:\.bat)?\s+\w+ | gradle\s+\w+ | mvn\s+[\w:\-]+
          | bundle\s+exec\s+\w+ | rake\s+\w+ | mix\s+(?:test|compile)\b
          | swift\s+(?:build|test)\b | flutter\s+(?:test|build)\b | dart\s+test\b | zig\s+build\b
          | terraform\s+(?:validate|fmt|plan)\b
        )",
    )
    .expect("build command regex")
});

/// Is this a build/test/lint command worth remembering when it succeeds?
pub fn is_build_command(cmd: &str) -> bool {
    let c = cmd.trim();
    if c.is_empty() || c.chars().count() > 160 || c.contains('\n') || c.contains("[REDACTED") {
        return false;
    }
    // Consider each segment of a simple chain (`cd app && npm test`).
    c.split("&&").map(str::trim).any(|seg| {
        let seg = strip_env_prefix(seg);
        BUILD_COMMAND.is_match(seg)
    })
}

fn strip_env_prefix(seg: &str) -> &str {
    let mut s = seg;
    loop {
        let t = s.trim_start();
        let Some(space) = t.find(' ') else { return t };
        let first = &t[..space];
        if first.contains('=') && !first.starts_with('-') && first.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') {
            s = &t[space + 1..];
        } else {
            return t;
        }
    }
}

impl Store {
    /// Process one hook event. Never fails for "expected" situations (unregistered project, capture disabled);
    /// those return an outcome with `skipped` set.
    pub fn process_hook_event(&mut self, ev: &HookEvent) -> Result<HookOutcome> {
        let settings = self.settings()?;
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(c) = &ev.cwd {
            candidates.push(c.clone());
        }
        candidates.extend(ev.workspace_paths.iter().cloned());
        let mut resolved: Option<Resolved> = None;
        for c in &candidates {
            if let Some(r) = self.resolve_project(c, settings.capture.auto_register_projects)? {
                resolved = Some(r);
                break;
            }
        }
        let Some(Resolved { project, checkout_root }) = resolved else {
            return Ok(HookOutcome { skipped: Some("not a registered ContextForge project".into()), ..Default::default() });
        };
        if !project.settings.capture_enabled {
            return Ok(HookOutcome {
                project_id: Some(project.id),
                skipped: Some("ContextForge is disabled for this project".into()),
                ..Default::default()
            });
        }
        let external =
            if ev.external_session_id.trim().is_empty() { "unknown".to_string() } else { ev.external_session_id.trim().to_string() };

        // One `git status` at session start (shared by freshness and the brief); turn/session end only need HEAD.
        let wt =
            if matches!(ev.kind, HookEventKind::SessionStart { .. }) && project.is_git { git::status(&checkout_root).ok() } else { None };
        let (head, branch) = match &wt {
            Some(w) => (w.head.clone(), w.branch.clone()),
            None if project.is_git && matches!(ev.kind, HookEventKind::TurnEnded { .. } | HookEventKind::SessionEnded { .. }) => {
                git::head_info(&checkout_root)
            }
            None => (None, None),
        };
        let cwd_str = ev.cwd.as_ref().map(|c| c.to_string_lossy().into_owned());
        let root_str = checkout_root.to_string_lossy().into_owned();
        let (session, _created) = self.upsert_session(&SessionStartInfo {
            project_id: &project.id,
            agent: ev.agent,
            external_id: &external,
            cwd: cwd_str.as_deref(),
            root: Some(&root_str),
            model: ev.model.as_deref(),
            branch: branch.as_deref(),
            head: head.as_deref(),
        })?;
        let actor = Actor::agent(ev.agent, Origin::Hook, Some(session.id.clone()));
        let now = util::now_ts();
        let mut outcome = HookOutcome { project_id: Some(project.id.clone()), session_id: Some(session.id.clone()), ..Default::default() };

        match &ev.kind {
            HookEventKind::SessionStart { source } => {
                let summary = format!(
                    "{} session {}",
                    ev.agent.label(),
                    source.as_deref().map(|s| format!("started ({s})")).unwrap_or_else(|| "started".into())
                );
                self.log_event(&project.id, &session.id, ev.agent, EventKind::SessionStart, &now, &summary, serde_json::json!({
                    "source": source, "branch": wt.as_ref().and_then(|w| w.branch.clone()), "head": wt.as_ref().and_then(|w| w.head.clone()),
                    "uncommitted": wt.as_ref().map(|w| w.dirty_count()), "model": ev.model,
                }))?;
                if settings.freshness.check_on_session_start
                    && let Err(e) = self.check_freshness_with(&project.id, Some(&checkout_root), wt.clone(), &actor)
                {
                    tracing::warn!(error = %e, "freshness check failed during session start");
                }
                let budget = project.settings.startup_budget_tokens.unwrap_or(settings.context.startup_budget_tokens);
                let mut opts = BriefOptions::new(budget);
                opts.target = Some(ev.agent);
                opts.session_id = Some(session.id.clone());
                opts.checkout_root = Some(checkout_root.clone());
                opts.working_tree = wt.clone();
                let brief = self.build_brief(&project.id, &opts)?;
                self.record_injections(&session.id, &brief.memory_ids, "startup")?;
                self.log_event(
                    &project.id,
                    &session.id,
                    ev.agent,
                    EventKind::ContextInjected,
                    &now,
                    &format!("Startup context injected: {} memories, ~{} tokens", brief.memory_ids.len(), brief.token_estimate),
                    serde_json::json!({ "memory_ids": brief.memory_ids, "tokens": brief.token_estimate, "channel": "startup" }),
                )?;
                outcome.context = Some(brief.text);
            }
            HookEventKind::PromptSubmitted { prompt } => {
                outcome.context = self.handle_prompt(&project.id, &session.id, ev.agent, prompt, &settings, &now)?;
            }
            HookEventKind::BeforeModel { last_user_input, .. } => {
                if let Some(input) = last_user_input.as_deref().filter(|s| !s.trim().is_empty()) {
                    let hash = util::sha256_hex(input.as_bytes());
                    let previous: Option<String> =
                        self.conn.query_row("SELECT last_prompt_hash FROM sessions WHERE id = ?1", [&session.id], |r| r.get(0))?;
                    if previous.as_deref() != Some(hash.as_str()) {
                        self.write_tx(|tx| {
                            tx.execute("UPDATE sessions SET last_prompt_hash = ?2 WHERE id = ?1", params![session.id, hash])?;
                            Ok(())
                        })?;
                        outcome.context = self.handle_prompt(&project.id, &session.id, ev.agent, input, &settings, &now)?;
                    }
                }
            }
            HookEventKind::ToolUsed(tu) => {
                self.handle_tool(&project, &checkout_root, ev, &session.id, tu, &settings, &now)?;
            }
            HookEventKind::TurnEnded { last_message, fully_idle, reason } => {
                let summary_text = if settings.capture.assistant_summaries {
                    last_message.as_deref().map(|m| privacy::redact_str(&util::truncate_chars(m.trim(), 600))).filter(|m| !m.is_empty())
                } else {
                    None
                };
                let head = head.clone();
                let prev_end: Option<String> =
                    self.conn
                        .query_row("SELECT COALESCE(end_commit, start_commit) FROM sessions WHERE id = ?1", [&session.id], |r| r.get(0))?;
                self.write_tx(|tx| {
                    tx.execute(
                        "UPDATE sessions SET summary = COALESCE(?2, summary), end_commit = COALESCE(?3, end_commit), last_activity_at = ?4 WHERE id = ?1",
                        params![session.id, summary_text, head, now],
                    )?;
                    Ok(())
                })?;
                let excerpt = summary_text.as_deref().map(|s| util::truncate_chars(&util::collapse_ws(s), 140));
                let line = match (&excerpt, reason) {
                    (Some(e), _) => format!("Turn ended: {e}"),
                    (None, Some(r)) => format!("Turn ended ({r})"),
                    (None, None) => "Turn ended".to_string(),
                };
                self.log_event(
                    &project.id,
                    &session.id,
                    ev.agent,
                    EventKind::TurnEnd,
                    &now,
                    &line,
                    serde_json::json!({ "reason": reason, "fully_idle": fully_idle }),
                )?;
                if let (Some(prev), Some(cur)) = (prev_end.as_deref(), head.as_deref())
                    && prev != cur
                {
                    let commits = git::log(&checkout_root, &format!("{prev}..{cur}"), 10);
                    if !commits.is_empty() {
                        let subjects: Vec<String> =
                            commits.iter().map(|c| format!("{} {}", &c.sha[..c.sha.len().min(8)], c.subject)).collect();
                        self.log_event(
                            &project.id,
                            &session.id,
                            ev.agent,
                            EventKind::Commits,
                            &now,
                            &format!("{} new commit(s): {}", commits.len(), util::truncate_chars(&subjects.join(" · "), 200)),
                            serde_json::json!({ "commits": commits }),
                        )?;
                    }
                }
                if *fully_idle != Some(false) {
                    self.settle_session_memories(&session.id, &checkout_root)?;
                }
            }
            HookEventKind::SessionEnded { reason } => {
                let head = head.clone();
                self.write_tx(|tx| {
                    tx.execute(
                        "UPDATE sessions SET ended_at = ?2, end_reason = ?3, end_commit = COALESCE(?4, end_commit), last_activity_at = ?2 WHERE id = ?1",
                        params![session.id, now, reason, head],
                    )?;
                    Ok(())
                })?;
                self.log_event(
                    &project.id,
                    &session.id,
                    ev.agent,
                    EventKind::SessionEnd,
                    &now,
                    &format!("{} session ended{}", ev.agent.label(), reason.as_deref().map(|r| format!(" ({r})")).unwrap_or_default()),
                    serde_json::json!({ "reason": reason }),
                )?;
            }
        }
        Ok(outcome)
    }

    #[allow(clippy::too_many_arguments)]
    fn log_event(
        &mut self,
        project_id: &str,
        session_id: &str,
        agent: AgentKind,
        kind: EventKind,
        at: &str,
        summary: &str,
        detail: serde_json::Value,
    ) -> Result<()> {
        let summary = privacy::redact_str(summary);
        self.write_tx(|tx| {
            insert_event(tx, project_id, Some(session_id), agent, kind, at, &summary, &detail)?;
            touch_agent_activity(tx, agent, kind.as_str(), at)?;
            tx.execute("UPDATE sessions SET last_activity_at = ?2 WHERE id = ?1 AND last_activity_at < ?2", params![session_id, at])?;
            Store::touch_project(tx, project_id, at)?;
            Ok(())
        })
    }

    fn handle_prompt(
        &mut self,
        project_id: &str,
        session_id: &str,
        agent: AgentKind,
        prompt: &str,
        settings: &crate::Settings,
        now: &str,
    ) -> Result<Option<String>> {
        let stored = if settings.capture.prompts { Some(privacy::redact_str(&util::truncate_chars(prompt.trim(), 500))) } else { None };
        let summary = match &stored {
            Some(p) => format!("Prompt: {}", util::truncate_chars(&util::collapse_ws(p), 140)),
            None => format!("Prompt submitted ({} chars, text not stored)", prompt.chars().count()),
        };
        self.write_tx(|tx| {
            tx.execute("UPDATE sessions SET turns = turns + 1 WHERE id = ?1", [session_id])?;
            Ok(())
        })?;
        self.log_event(project_id, session_id, agent, EventKind::Prompt, now, &summary, serde_json::json!({ "prompt": stored }))?;
        if !settings.context.prompt_injection {
            return Ok(None);
        }
        let Some(ctx) = self.prompt_context(project_id, prompt, Some(session_id), settings.context.prompt_budget_tokens)? else {
            return Ok(None);
        };
        self.record_injections(session_id, &ctx.memory_ids, "prompt")?;
        self.log_event(
            project_id,
            session_id,
            agent,
            EventKind::ContextInjected,
            now,
            &format!("Prompt-relevant context injected: {} memories", ctx.memory_ids.len()),
            serde_json::json!({ "memory_ids": ctx.memory_ids, "tokens": ctx.token_estimate, "channel": "prompt" }),
        )?;
        Ok(Some(ctx.text))
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_tool(
        &mut self,
        project: &crate::projects::Project,
        checkout_root: &Path,
        ev: &HookEvent,
        session_id: &str,
        tu: &ToolUse,
        settings: &crate::Settings,
        now: &str,
    ) -> Result<()> {
        let matcher = self.sensitive_matcher(&project.settings.excluded_globs)?;
        let extra: Vec<PathBuf> =
            project.all_roots().into_iter().filter(|r| paths::path_key(r) != paths::path_key(&project.root())).collect();
        let mut touched: Vec<(String, FileAction, bool)> = Vec::new();
        for f in &tu.files {
            match paths::normalize_file_ref(&project.root(), &extra, ev.cwd.as_deref().or(Some(checkout_root)), &f.path) {
                Ok(rel) => {
                    let sensitive = matcher.is_sensitive(&rel);
                    if !touched.iter().any(|(p, _, _)| p == &rel) {
                        touched.push((rel, f.action, sensitive));
                    }
                }
                Err(_) => continue, // outside the project: not recorded
            }
        }
        if !touched.is_empty() {
            let names: Vec<String> =
                touched.iter().take(4).map(|(p, _, s)| if *s { format!("{p} (sensitive)") } else { p.clone() }).collect();
            let summary = if touched.len() == 1 {
                format!("{} {}", capitalize(touched[0].1.as_str()), names[0])
            } else {
                format!("Edited {} files: {}{}", touched.len(), names.join(", "), if touched.len() > 4 { ", …" } else { "" })
            };
            let files_json: Vec<serde_json::Value> =
                touched.iter().map(|(p, a, s)| serde_json::json!({ "path": p, "action": a.as_str(), "sensitive": s })).collect();
            let tool = tu.tool.clone();
            self.write_tx(|tx| {
                for (p, a, s) in &touched {
                    tx.execute(
                        "INSERT INTO session_files(session_id, path, action, sensitive, touches, last_at) VALUES (?1, ?2, ?3, ?4, 1, ?5)
                         ON CONFLICT(session_id, path) DO UPDATE SET touches = touches + 1, last_at = excluded.last_at,
                             action = CASE WHEN excluded.action = 'delete' THEN 'delete' ELSE session_files.action END",
                        params![session_id, p, a.as_str(), *s as i64, now],
                    )?;
                }
                tx.execute(
                    "UPDATE sessions SET files_touched = (SELECT COUNT(*) FROM session_files WHERE session_id = ?1) WHERE id = ?1",
                    [session_id],
                )?;
                Ok(())
            })?;
            self.log_event(
                &project.id,
                session_id,
                ev.agent,
                EventKind::FileEdit,
                now,
                &summary,
                serde_json::json!({ "tool": tool, "files": files_json }),
            )?;
        }

        if let Some(cmd) = tu.command.as_deref().filter(|c| !c.trim().is_empty())
            && settings.capture.commands
        {
            let (redacted, n) = privacy::redact(cmd.trim());
            let shown = util::truncate_chars(&util::collapse_ws(&redacted), 300);
            let status = match (tu.success, tu.exit_code) {
                (_, Some(code)) => format!("exit {code}"),
                (Some(true), None) => "succeeded".into(),
                (Some(false), None) => "failed".into(),
                (None, None) => "result unknown".into(),
            };
            self.write_tx(|tx| {
                tx.execute("UPDATE sessions SET commands_run = commands_run + 1 WHERE id = ?1", [session_id])?;
                Ok(())
            })?;
            let error = tu.error.as_deref().map(|e| privacy::redact_str(&util::truncate_chars(e, 300)));
            self.log_event(
                &project.id,
                session_id,
                ev.agent,
                EventKind::Command,
                now,
                &format!("Ran `{}` ({status})", util::truncate_chars(&shown, 160)),
                serde_json::json!({ "command": shown, "success": tu.success, "exit_code": tu.exit_code, "error": error, "tool": tu.tool }),
            )?;

            let succeeded = tu.success == Some(true) && tu.exit_code.is_none_or(|c| c == 0);
            if succeeded && n == 0 && settings.capture.auto_command_memories && is_build_command(&redacted) {
                let title = util::truncate_chars(&util::collapse_ws(&redacted), 160);
                let mut new = NewMemory::new(
                    project.id.clone(),
                    MemoryKind::Command,
                    format!("`{title}`"),
                    "Build/test/lint command that has run successfully in this project (captured automatically).",
                );
                new.certainty = Some(Certainty::Verified);
                new.tags = vec!["auto".into()];
                new.evidence = vec![EvidenceInput {
                    kind: EvidenceKind::Command,
                    reference: title.clone(),
                    excerpt: Some(format!("{} reported success ({status})", ev.agent.label())),
                }];
                let actor = Actor::agent(ev.agent, Origin::Auto, Some(session_id.to_string()));
                if let Err(e) = self.save_memory(new, &actor) {
                    tracing::warn!(error = %e, "could not save automatic command memory");
                }
            }
        }
        Ok(())
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_build_commands() {
        for c in [
            "cargo test --workspace",
            "npm run build",
            "cd app && npm test",
            "RUST_LOG=debug cargo test",
            "pytest -q",
            "python -m pytest tests",
            "go test ./...",
            "dotnet build",
            "make",
            "./gradlew test",
        ] {
            assert!(is_build_command(c), "{c}");
        }
        for c in ["ls -la", "git commit -m x", "rm -rf target", "cat Cargo.toml", "npm view react", "echo cargo test", ""] {
            assert!(!is_build_command(c), "{c}");
        }
        assert!(!is_build_command("cargo test\nrm -rf /"));
        assert!(!is_build_command("TOKEN=[REDACTED:secret] cargo test"));
    }
}
