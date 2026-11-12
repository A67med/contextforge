//! Full-text and filtered memory search.

use rusqlite::types::Value;
use serde::{Deserialize, Serialize};

use crate::model::{AgentKind, Certainty, Freshness, Memory, MemoryKind, MemoryStatus};
use crate::{Result, Store, util};

/// How free-text terms are combined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    /// Every term must match (interactive search). The last term matches as a prefix.
    #[default]
    All,
    /// Any term may match, ranked by BM25 (prompt-relevance retrieval).
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    #[default]
    Relevance,
    Updated,
    Created,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SearchQuery {
    pub project_id: Option<String>,
    pub text: Option<String>,
    pub mode: MatchMode,
    pub kinds: Vec<MemoryKind>,
    pub statuses: Vec<MemoryStatus>,
    pub freshness: Vec<Freshness>,
    pub certainty: Vec<Certainty>,
    pub agents: Vec<AgentKind>,
    pub pinned_only: bool,
    pub include_superseded: bool,
    /// Only memories referencing this project-relative file path.
    pub file: Option<String>,
    pub limit: usize,
    pub offset: usize,
    pub sort: SortOrder,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchHit {
    pub memory: Memory,
    /// Highlighted excerpt of the content (FTS matches wrapped in «»).
    pub snippet: Option<String>,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchResults {
    pub hits: Vec<SearchHit>,
    pub total: i64,
}

/// Build a safe FTS5 MATCH expression from user text: every term is quoted (so FTS syntax in user input
/// cannot break the query), and terms are joined with AND or OR.
pub fn fts_expression(text: &str, mode: MatchMode) -> Option<String> {
    let terms: Vec<String> = match mode {
        MatchMode::All => util::word_tokens(text),
        MatchMode::Any => util::keywords(text, 12),
    };
    if terms.is_empty() {
        return None;
    }
    let quoted: Vec<String> = terms
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let esc = t.replace('"', "\"\"");
            if mode == MatchMode::All && i == terms.len() - 1 { format!("\"{esc}\"*") } else { format!("\"{esc}\"") }
        })
        .collect();
    Some(quoted.join(if mode == MatchMode::All { " AND " } else { " OR " }))
}

impl Store {
    pub fn search_memories(&self, q: &SearchQuery) -> Result<SearchResults> {
        let limit = if q.limit == 0 { 20 } else { q.limit.min(200) };
        let fts = q.text.as_deref().and_then(|t| fts_expression(t, q.mode));
        let mut where_clauses: Vec<String> = Vec::new();
        let mut params: Vec<Value> = Vec::new();

        if let Some(expr) = &fts {
            params.push(Value::Text(expr.clone()));
            where_clauses.push(format!("memories_fts MATCH ?{}", params.len()));
        }
        if let Some(pid) = &q.project_id {
            params.push(Value::Text(pid.clone()));
            where_clauses.push(format!("m.project_id = ?{}", params.len()));
        }
        let push_in = |col: &str, values: Vec<String>, where_clauses: &mut Vec<String>, params: &mut Vec<Value>| {
            if values.is_empty() {
                return;
            }
            let start = params.len() + 1;
            for v in values {
                params.push(Value::Text(v));
            }
            let placeholders: Vec<String> = (start..=params.len()).map(|i| format!("?{i}")).collect();
            where_clauses.push(format!("{col} IN ({})", placeholders.join(", ")));
        };
        push_in("m.kind", q.kinds.iter().map(|k| k.as_str().to_string()).collect(), &mut where_clauses, &mut params);
        push_in("m.status", q.statuses.iter().map(|k| k.as_str().to_string()).collect(), &mut where_clauses, &mut params);
        push_in("m.freshness", q.freshness.iter().map(|k| k.as_str().to_string()).collect(), &mut where_clauses, &mut params);
        push_in("m.certainty", q.certainty.iter().map(|k| k.as_str().to_string()).collect(), &mut where_clauses, &mut params);
        push_in("m.source_agent", q.agents.iter().map(|k| k.as_str().to_string()).collect(), &mut where_clauses, &mut params);
        if q.pinned_only {
            where_clauses.push("m.pinned = 1".into());
        }
        if !q.include_superseded && !q.freshness.contains(&Freshness::Superseded) {
            where_clauses.push("m.freshness != 'superseded'".into());
        }
        if let Some(file) = &q.file {
            params.push(Value::Text(file.replace('\\', "/")));
            where_clauses.push(format!("EXISTS (SELECT 1 FROM memory_files f WHERE f.memory_id = m.id AND f.path = ?{})", params.len()));
        }

        let from = if fts.is_some() { "memories m JOIN memories_fts ON memories_fts.rowid = m.seq" } else { "memories m" };
        let where_sql = if where_clauses.is_empty() { String::new() } else { format!("WHERE {}", where_clauses.join(" AND ")) };
        let order = match (q.sort, fts.is_some()) {
            (SortOrder::Relevance, true) => "bm25(memories_fts, 4.0, 1.0, 2.0), m.pinned DESC, m.updated_at DESC",
            (SortOrder::Relevance, false) => "m.pinned DESC, m.updated_at DESC",
            (SortOrder::Updated, _) => "m.updated_at DESC",
            (SortOrder::Created, _) => "m.created_at DESC",
        };
        let snippet = if fts.is_some() { "snippet(memories_fts, 1, '«', '»', '…', 14)" } else { "NULL" };

        let total: i64 =
            self.conn
                .query_row(&format!("SELECT COUNT(*) FROM {from} {where_sql}"), rusqlite::params_from_iter(params.iter()), |r| r.get(0))?;

        let sql = format!(
            "SELECT {cols}, {snippet} AS snip FROM {from} {where_sql} ORDER BY {order} LIMIT {limit} OFFSET {offset}",
            cols = crate::memories::MEMORY_COLUMNS_M,
            offset = q.offset,
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), |r| {
            let memory = crate::memories::row_to_memory(r)?;
            let snip: Option<String> = r.get("snip")?;
            Ok((memory, snip))
        })?;
        let mut hits = Vec::new();
        for row in rows {
            let (memory, snippet) = row?;
            let files = self.memory_file_paths(&memory.id)?;
            hits.push(SearchHit { memory, snippet, files });
        }
        Ok(SearchResults { hits, total })
    }

    pub(crate) fn memory_file_paths(&self, memory_id: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare_cached("SELECT path FROM memory_files WHERE memory_id = ?1 ORDER BY path")?;
        let paths = stmt.query_map([memory_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts_expressions_quote_terms() {
        assert_eq!(fts_expression("wal locking", MatchMode::All).unwrap(), "\"wal\" AND \"locking\"*");
        assert_eq!(fts_expression("fix the WAL lock", MatchMode::Any).unwrap(), "\"wal\" OR \"lock\"");
        // FTS operators in user input are neutralised by quoting.
        assert_eq!(fts_expression("NEAR(a b) OR \"x", MatchMode::All).unwrap(), "\"near\" AND \"or\"*");
        assert!(fts_expression("   ", MatchMode::All).is_none());
    }
}
