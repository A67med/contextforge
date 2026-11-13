//! Agent sessions, the activity event log, and per-session context-injection tracking.

use std::collections::HashSet;

use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::model::{AgentKind, EventKind, EventRecord, SessionFile, SessionInfo};
use crate::{Error, Result, Store, util};

const SESSION_COLUMNS: &str = "id, project_id, agent, external_id, cwd, root_path, model, branch, start_commit, end_commit, \
    started_at, last_activity_at, ended_at, end_reason, summary, files_touched, commands_run, turns";

fn row_to_session(r: &Row<'_>) -> rusqlite::Result<SessionInfo> {
    Ok(SessionInfo {
        id: r.get("id")?,
        project_id: r.get("project_id")?,
        agent: r.get("agent")?,
        external_id: r.get("external_id")?,
        cwd: r.get("cwd")?,
        root_path: r.get("root_path")?,
        model: r.get("model")?,
        branch: r.get("branch")?,
        start_commit: r.get("start_commit")?,
        end_commit: r.get("end_commit")?,
        started_at: r.get("started_at")?,
        last_activity_at: r.get("last_activity_at")?,
        ended_at: r.get("ended_at")?,
        end_reason: r.get("end_reason")?,
        summary: r.get("summary")?,
        files_touched: r.get("files_touched")?,
        commands_run: r.get("commands_run")?,
        turns: r.get("turns")?,
    })
}

fn row_to_event(r: &Row<'_>) -> rusqlite::Result<EventRecord> {
    let detail: String = r.get("detail_json")?;
    Ok(EventRecord {
        id: r.get("id")?,
        project_id: r.get("project_id")?,
        session_id: r.get("session_id")?,
        agent: r.get("agent")?,
        kind: r.get("kind")?,
        occurred_at: r.get("occurred_at")?,
        summary: r.get("summary")?,
        detail: serde_json::from_str(&detail).unwrap_or(serde_json::Value::Null),
    })
}

/// Append an activity event (inside an existing write transaction).
#[allow(clippy::too_many_arguments)]
pub(crate) fn insert_event(
    tx: &rusqlite::Transaction<'_>,
    project_id: &str,
    session_id: Option<&str>,
    agent: AgentKind,
    kind: EventKind,
    at: &str,
    summary: &str,
    detail: &serde_json::Value,
) -> Result<i64> {
    tx.execute(
        "INSERT INTO events(project_id, session_id, agent, kind, occurred_at, summary, detail_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![project_id, session_id, agent, kind, at, util::truncate_chars(summary, 300), serde_json::to_string(detail)?],
    )?;
    Ok(tx.last_insert_rowid())
}

pub(crate) fn touch_agent_activity(tx: &rusqlite::Transaction<'_>, agent: AgentKind, kind: &str, at: &str) -> Result<()> {
    tx.execute(
        "INSERT INTO agent_activity(agent, last_event_at, last_event_kind, event_count) VALUES (?1, ?2, ?3, 1)
         ON CONFLICT(agent) DO UPDATE SET last_event_at = excluded.last_event_at, last_event_kind = excluded.last_event_kind,
             event_count = agent_activity.event_count + 1",
        params![agent, at, kind],
    )?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SessionQuery {
    pub project_id: Option<String>,
    pub agent: Option<AgentKind>,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentActivity {
    pub agent: AgentKind,
    pub last_event_at: String,
    pub last_event_kind: String,
    pub event_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionDetail {
    pub session: SessionInfo,
    pub events: Vec<EventRecord>,
    pub files: Vec<SessionFile>,
    pub memories: Vec<(String, String, String)>,
}

pub(crate) struct SessionStartInfo<'a> {
    pub project_id: &'a str,
    pub agent: AgentKind,
    pub external_id: &'a str,
    pub cwd: Option<&'a str>,
    pub root: Option<&'a str>,
    pub model: Option<&'a str>,
    pub branch: Option<&'a str>,
    pub head: Option<&'a str>,
}

impl Store {
    /// Find or create the session for a provider session id. Returns (session, created).
    pub(crate) fn upsert_session(&mut self, info: &SessionStartInfo<'_>) -> Result<(SessionInfo, bool)> {
        let existing: Option<String> = self
            .conn
            .query_row("SELECT id FROM sessions WHERE agent = ?1 AND external_id = ?2", params![info.agent, info.external_id], |r| r.get(0))
            .optional()?;
        let now = util::now_ts();
        if let Some(id) = existing {
            self.write_tx(|tx| {
                tx.execute(
                    "UPDATE sessions SET last_activity_at = ?2, model = COALESCE(?3, model), branch = COALESCE(?4, branch),
                            ended_at = NULL, end_reason = NULL WHERE id = ?1",
                    params![id, now, info.model, info.branch],
                )?;
                Ok(())
            })?;
            return Ok((self.get_session(&id)?, false));
        }
        let id = util::new_id("ses");
        self.write_tx(|tx| {
            tx.execute(
                "INSERT INTO sessions(id, project_id, agent, external_id, cwd, root_path, model, branch, start_commit, started_at,
                                      last_activity_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
                params![id, info.project_id, info.agent, info.external_id, info.cwd, info.root, info.model, info.branch, info.head, now],
            )?;
            Ok(())
        })?;
        Ok((self.get_session(&id)?, true))
    }

    pub fn get_session(&self, id: &str) -> Result<SessionInfo> {
        self.conn
            .query_row(&format!("SELECT {SESSION_COLUMNS} FROM sessions WHERE id = ?1"), [id], row_to_session)
            .optional()?
            .ok_or_else(|| Error::not_found(format!("session '{id}'")))
    }

    pub fn list_sessions(&self, q: &SessionQuery) -> Result<Vec<SessionInfo>> {
        let limit = if q.limit == 0 { 50 } else { q.limit.min(500) };
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SESSION_COLUMNS} FROM sessions
             WHERE (?1 IS NULL OR project_id = ?1) AND (?2 IS NULL OR agent = ?2)
             ORDER BY last_activity_at DESC LIMIT ?3 OFFSET ?4"
        ))?;
        let rows = stmt
            .query_map(params![q.project_id, q.agent, limit as i64, q.offset as i64], row_to_session)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn session_detail(&self, id: &str) -> Result<SessionDetail> {
        let session = self.get_session(id)?;
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, project_id, session_id, agent, kind, occurred_at, summary, detail_json FROM events
             WHERE session_id = ?1 ORDER BY id LIMIT 500",
        )?;
        let events = stmt.query_map([id], row_to_event)?.collect::<rusqlite::Result<_>>()?;
        let files = self.session_files(id)?;
        let mut stmt = self.conn.prepare_cached("SELECT id, kind, title FROM memories WHERE source_session_id = ?1 ORDER BY created_at")?;
        let memories = stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
        Ok(SessionDetail { session, events, files, memories })
    }

    pub fn session_files(&self, session_id: &str) -> Result<Vec<SessionFile>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT path, action, sensitive, touches, last_at FROM session_files WHERE session_id = ?1 ORDER BY last_at DESC",
        )?;
        let rows = stmt
            .query_map([session_id], |r| {
                Ok(SessionFile {
                    path: r.get(0)?,
                    action: r.get(1)?,
                    sensitive: r.get::<_, i64>(2)? != 0,
                    touches: r.get(3)?,
                    last_at: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Most recent events, optionally for one project.
    pub fn recent_events(&self, project_id: Option<&str>, limit: usize) -> Result<Vec<EventRecord>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, project_id, session_id, agent, kind, occurred_at, summary, detail_json FROM events
             WHERE (?1 IS NULL OR project_id = ?1) ORDER BY occurred_at DESC, id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![project_id, limit.min(1000) as i64], row_to_event)?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn agent_activity(&self) -> Result<Vec<AgentActivity>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT agent, last_event_at, last_event_kind, event_count FROM agent_activity ORDER BY last_event_at DESC")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(AgentActivity { agent: r.get(0)?, last_event_at: r.get(1)?, last_event_kind: r.get(2)?, event_count: r.get(3)? })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Most recent session of `agent` in a project that is still active (not ended, recent activity).
    /// Used to attribute MCP writes to the agent session that made them.
    pub fn active_session_for(&self, project_id: &str, agents: &[AgentKind], within_minutes: i64) -> Result<Option<String>> {
        let cutoff = util::ts(chrono::Utc::now() - chrono::Duration::minutes(within_minutes));
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, agent FROM sessions WHERE project_id = ?1 AND ended_at IS NULL AND last_activity_at >= ?2
             ORDER BY last_activity_at DESC LIMIT 20",
        )?;
        let rows: Vec<(String, AgentKind)> =
            stmt.query_map(params![project_id, cutoff], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        Ok(rows.into_iter().find(|(_, a)| agents.contains(a)).map(|(id, _)| id))
    }

    pub(crate) fn injected_memory_ids(&self, session_id: &str) -> Result<HashSet<String>> {
        let mut stmt = self.conn.prepare_cached("SELECT memory_id FROM session_injections WHERE session_id = ?1")?;
        let ids = stmt.query_map([session_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(ids)
    }

    pub(crate) fn record_injections(&mut self, session_id: &str, ids: &[String], channel: &str) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let now = util::now_ts();
        self.write_tx(|tx| {
            for id in ids {
                tx.execute(
                    "INSERT OR IGNORE INTO session_injections(session_id, memory_id, injected_at, channel) VALUES (?1, ?2, ?3, ?4)",
                    params![session_id, id, now, channel],
                )?;
            }
            Ok(())
        })
    }

    /// Delete raw activity events older than `days`. Sessions, memories and handoffs are kept.
    pub fn prune_events(&mut self, days: u32) -> Result<usize> {
        let cutoff = util::ts(chrono::Utc::now() - chrono::Duration::days(days as i64));
        self.write_tx(|tx| Ok(tx.execute("DELETE FROM events WHERE occurred_at < ?1", [cutoff])?))
    }
}
