-- ContextForge schema v1.
-- Timestamps are RFC 3339 UTC strings with millisecond precision ("2026-11-12T10:15:30.123Z"),
-- which sort lexicographically. Paths stored in *_key columns are normalised comparison keys
-- (forward slashes, lower-case on Windows); display paths keep their original form.

CREATE TABLE projects (
    id                 TEXT PRIMARY KEY,
    name               TEXT NOT NULL,
    root_path          TEXT NOT NULL,
    path_key           TEXT NOT NULL UNIQUE,
    is_git             INTEGER NOT NULL DEFAULT 0,
    git_common_dir_key TEXT,
    repo_fingerprint   TEXT,
    remote_url         TEXT,
    default_branch     TEXT,
    settings_json      TEXT NOT NULL DEFAULT '{}',
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL,
    last_activity_at   TEXT
);
CREATE INDEX idx_projects_common_dir ON projects(git_common_dir_key);

CREATE TABLE project_roots (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    path       TEXT NOT NULL,
    path_key   TEXT NOT NULL UNIQUE,
    kind       TEXT NOT NULL CHECK (kind IN ('main', 'worktree')),
    branch     TEXT,
    added_at   TEXT NOT NULL,
    PRIMARY KEY (project_id, path_key)
);

CREATE TABLE sessions (
    id               TEXT PRIMARY KEY,
    project_id       TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    agent            TEXT NOT NULL,
    external_id      TEXT,
    cwd              TEXT,
    root_path        TEXT,
    model            TEXT,
    branch           TEXT,
    start_commit     TEXT,
    end_commit       TEXT,
    started_at       TEXT NOT NULL,
    last_activity_at TEXT NOT NULL,
    ended_at         TEXT,
    end_reason       TEXT,
    summary          TEXT,
    files_touched    INTEGER NOT NULL DEFAULT 0,
    commands_run     INTEGER NOT NULL DEFAULT 0,
    turns            INTEGER NOT NULL DEFAULT 0,
    last_prompt_hash TEXT,
    UNIQUE (agent, external_id)
);
CREATE INDEX idx_sessions_project_time ON sessions(project_id, last_activity_at DESC);
CREATE INDEX idx_sessions_time ON sessions(last_activity_at DESC);

CREATE TABLE events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    session_id  TEXT REFERENCES sessions(id) ON DELETE CASCADE,
    agent       TEXT NOT NULL,
    kind        TEXT NOT NULL,
    occurred_at TEXT NOT NULL,
    summary     TEXT NOT NULL,
    detail_json TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX idx_events_session ON events(session_id, id);
CREATE INDEX idx_events_project_time ON events(project_id, occurred_at DESC);
CREATE INDEX idx_events_time ON events(occurred_at DESC);

CREATE TABLE session_files (
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    path       TEXT NOT NULL,
    action     TEXT NOT NULL,
    sensitive  INTEGER NOT NULL DEFAULT 0,
    touches    INTEGER NOT NULL DEFAULT 1,
    last_at    TEXT NOT NULL,
    PRIMARY KEY (session_id, path)
);

CREATE TABLE memories (
    seq               INTEGER PRIMARY KEY AUTOINCREMENT,
    id                TEXT NOT NULL UNIQUE,
    project_id        TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    kind              TEXT NOT NULL CHECK (kind IN ('decision', 'convention', 'fact', 'task', 'bug', 'note', 'command', 'research')),
    status            TEXT NOT NULL,
    certainty         TEXT NOT NULL CHECK (certainty IN ('verified', 'decided', 'unverified', 'hypothesis')),
    freshness         TEXT NOT NULL DEFAULT 'current' CHECK (freshness IN ('current', 'potentially_stale', 'needs_review', 'superseded')),
    title             TEXT NOT NULL,
    content           TEXT NOT NULL DEFAULT '',
    tags_json         TEXT NOT NULL DEFAULT '[]',
    pinned            INTEGER NOT NULL DEFAULT 0,
    user_locked       INTEGER NOT NULL DEFAULT 0,
    branch            TEXT,
    origin_branch     TEXT,
    source_agent      TEXT NOT NULL,
    source_session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    source_origin     TEXT NOT NULL,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    last_validated_at TEXT,
    validated_commit  TEXT,
    superseded_by     TEXT REFERENCES memories(id) ON DELETE SET NULL,
    stale_reason      TEXT,
    injection_flag    INTEGER NOT NULL DEFAULT 0,
    content_hash      TEXT NOT NULL,
    revision          INTEGER NOT NULL DEFAULT 1,
    reinforced_count  INTEGER NOT NULL DEFAULT 0,
    metadata_json     TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX idx_memories_project_kind ON memories(project_id, kind, status);
CREATE INDEX idx_memories_project_fresh ON memories(project_id, freshness);
CREATE INDEX idx_memories_project_updated ON memories(project_id, updated_at DESC);
CREATE INDEX idx_memories_hash ON memories(project_id, content_hash);
CREATE INDEX idx_memories_session ON memories(source_session_id);

CREATE TABLE memory_files (
    memory_id    TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    path         TEXT NOT NULL,
    content_hash TEXT,
    line_start   INTEGER,
    line_end     INTEGER,
    sensitive    INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (memory_id, path)
);
CREATE INDEX idx_memory_files_path ON memory_files(path);

CREATE TABLE memory_commits (
    memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    sha       TEXT NOT NULL,
    relation  TEXT NOT NULL DEFAULT 'references',
    PRIMARY KEY (memory_id, sha)
);

CREATE TABLE memory_evidence (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    memory_id   TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,
    reference   TEXT NOT NULL,
    excerpt     TEXT,
    agent       TEXT NOT NULL,
    session_id  TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    captured_at TEXT NOT NULL
);
CREATE INDEX idx_evidence_memory ON memory_evidence(memory_id);

CREATE TABLE memory_links (
    from_id    TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    to_id      TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    relation   TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (from_id, to_id, relation)
);
CREATE INDEX idx_links_to ON memory_links(to_id);

CREATE TABLE memory_revisions (
    memory_id  TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    revision   INTEGER NOT NULL,
    title      TEXT NOT NULL,
    content    TEXT NOT NULL,
    status     TEXT NOT NULL,
    certainty  TEXT NOT NULL,
    changed_by TEXT NOT NULL,
    changed_at TEXT NOT NULL,
    reason     TEXT,
    PRIMARY KEY (memory_id, revision)
);

CREATE TABLE freshness_findings (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    memory_id   TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    detected_at TEXT NOT NULL,
    kind        TEXT NOT NULL,
    path        TEXT,
    detail      TEXT NOT NULL,
    head_commit TEXT,
    branch      TEXT,
    resolved_at TEXT,
    resolution  TEXT
);
CREATE INDEX idx_findings_memory ON freshness_findings(memory_id, resolved_at);

CREATE TABLE session_injections (
    session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    memory_id   TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    injected_at TEXT NOT NULL,
    channel     TEXT NOT NULL,
    PRIMARY KEY (session_id, memory_id)
);

CREATE TABLE handoffs (
    id             TEXT PRIMARY KEY,
    project_id     TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    target_agent   TEXT,
    requested_by   TEXT NOT NULL,
    session_id     TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    branch         TEXT,
    head_commit    TEXT,
    created_at     TEXT NOT NULL,
    token_estimate INTEGER NOT NULL,
    content_md     TEXT NOT NULL,
    content_json   TEXT NOT NULL
);
CREATE INDEX idx_handoffs_project ON handoffs(project_id, created_at DESC);

CREATE TABLE settings (
    key        TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE agent_activity (
    agent           TEXT PRIMARY KEY,
    last_event_at   TEXT NOT NULL,
    last_event_kind TEXT NOT NULL,
    event_count     INTEGER NOT NULL DEFAULT 0
);

CREATE VIRTUAL TABLE memories_fts USING fts5(
    title,
    content,
    tags_json,
    content = 'memories',
    content_rowid = 'seq',
    tokenize = 'porter unicode61 remove_diacritics 2'
);

CREATE TRIGGER memories_fts_insert AFTER INSERT ON memories BEGIN
    INSERT INTO memories_fts(rowid, title, content, tags_json)
    VALUES (new.seq, new.title, new.content, new.tags_json);
END;

CREATE TRIGGER memories_fts_delete AFTER DELETE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, title, content, tags_json)
    VALUES ('delete', old.seq, old.title, old.content, old.tags_json);
END;

CREATE TRIGGER memories_fts_update AFTER UPDATE OF title, content, tags_json ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, title, content, tags_json)
    VALUES ('delete', old.seq, old.title, old.content, old.tags_json);
    INSERT INTO memories_fts(rowid, title, content, tags_json)
    VALUES (new.seq, new.title, new.content, new.tags_json);
END;
