//! Aggregate statistics for the Overview screen and `contextforge status`.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::model::{AgentKind, EventRecord, Freshness, MemoryKind, SessionInfo};
use crate::sessions::{AgentActivity, SessionQuery};
use crate::{Result, Store, util};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Count<T> {
    pub key: T,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Overview {
    pub projects: i64,
    pub memories: i64,
    pub by_kind: Vec<Count<MemoryKind>>,
    pub by_freshness: Vec<Count<Freshness>>,
    pub open_tasks: i64,
    pub open_bugs: i64,
    pub needs_review: i64,
    pub sessions_total: i64,
    pub sessions_last_7d: Vec<Count<AgentKind>>,
    pub handoffs: i64,
    pub agent_activity: Vec<AgentActivity>,
    pub recent_sessions: Vec<SessionInfo>,
    pub recent_events: Vec<EventRecord>,
    pub database_path: String,
    pub database_bytes: u64,
    pub schema_version: i64,
}

impl Store {
    pub fn overview(&self, project_id: Option<&str>) -> Result<Overview> {
        let pid = project_id.map(str::to_string);
        let projects: i64 = match &pid {
            Some(_) => 1,
            None => self.conn.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?,
        };
        let (memories, open_tasks, open_bugs, needs_review): (i64, i64, i64, i64) = self.conn.query_row(
            "SELECT COUNT(*) FILTER (WHERE freshness != 'superseded'),
                    COUNT(*) FILTER (WHERE kind = 'task' AND status IN ('pending', 'in_progress') AND freshness != 'superseded'),
                    COUNT(*) FILTER (WHERE kind = 'bug' AND status = 'open' AND freshness != 'superseded'),
                    COUNT(*) FILTER (WHERE freshness IN ('potentially_stale', 'needs_review'))
             FROM memories WHERE (?1 IS NULL OR project_id = ?1)",
            [&pid],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        let mut stmt = self.conn.prepare_cached(
            "SELECT kind, COUNT(*) FROM memories WHERE (?1 IS NULL OR project_id = ?1) AND freshness != 'superseded' GROUP BY kind ORDER BY 2 DESC",
        )?;
        let by_kind = stmt.query_map([&pid], |r| Ok(Count { key: r.get(0)?, count: r.get(1)? }))?.collect::<rusqlite::Result<_>>()?;
        let mut stmt = self.conn.prepare_cached(
            "SELECT freshness, COUNT(*) FROM memories WHERE (?1 IS NULL OR project_id = ?1) GROUP BY freshness ORDER BY 2 DESC",
        )?;
        let by_freshness = stmt.query_map([&pid], |r| Ok(Count { key: r.get(0)?, count: r.get(1)? }))?.collect::<rusqlite::Result<_>>()?;
        let sessions_total: i64 =
            self.conn.query_row("SELECT COUNT(*) FROM sessions WHERE (?1 IS NULL OR project_id = ?1)", [&pid], |r| r.get(0))?;
        let since = util::ts(chrono::Utc::now() - chrono::Duration::days(7));
        let mut stmt = self.conn.prepare_cached(
            "SELECT agent, COUNT(*) FROM sessions WHERE (?1 IS NULL OR project_id = ?1) AND last_activity_at >= ?2 GROUP BY agent ORDER BY 2 DESC",
        )?;
        let sessions_last_7d =
            stmt.query_map(params![pid, since], |r| Ok(Count { key: r.get(0)?, count: r.get(1)? }))?.collect::<rusqlite::Result<_>>()?;
        let handoffs: i64 =
            self.conn.query_row("SELECT COUNT(*) FROM handoffs WHERE (?1 IS NULL OR project_id = ?1)", [&pid], |r| r.get(0))?;
        let recent_sessions = self.list_sessions(&SessionQuery { project_id: pid.clone(), limit: 8, ..Default::default() })?;
        let recent_events = self.recent_events(project_id, 25)?;
        let database_bytes = ["", "-wal"]
            .iter()
            .filter_map(|suffix| std::fs::metadata(format!("{}{suffix}", self.db_path().display())).ok())
            .map(|m| m.len())
            .sum();
        Ok(Overview {
            projects,
            memories,
            by_kind,
            by_freshness,
            open_tasks,
            open_bugs,
            needs_review,
            sessions_total,
            sessions_last_7d,
            handoffs,
            agent_activity: self.agent_activity()?,
            recent_sessions,
            recent_events,
            database_path: self.db_path().to_string_lossy().into_owned(),
            database_bytes,
            schema_version: self.schema_version()?,
        })
    }
}
