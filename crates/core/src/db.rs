//! Database location, connection setup and migrations.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, TransactionBehavior};

use crate::{Error, Result, util};

/// Ordered list of schema migrations. Never edit an applied migration; append a new one.
const MIGRATIONS: &[(i64, &str, &str)] = &[(1, "initial schema", include_str!("../migrations/0001_initial.sql"))];

pub const SCHEMA_VERSION: i64 = 1;

/// Environment variable that overrides the data directory (used by tests and portable installs).
pub const HOME_ENV: &str = "CONTEXTFORGE_HOME";

/// Where ContextForge keeps its database, logs and backups.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(HOME_ENV).filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("ContextForge")
}

/// Pointer file that lets users relocate the database (Settings → Database location).
fn location_file(data_dir: &Path) -> PathBuf {
    data_dir.join("location.json")
}

/// Resolve the database path for a data directory, honouring a relocation pointer.
pub fn database_path(data_dir: &Path) -> PathBuf {
    let pointer = location_file(data_dir);
    if let Ok(text) = std::fs::read_to_string(&pointer)
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
        && let Some(p) = value.get("database_path").and_then(|v| v.as_str())
        && !p.trim().is_empty()
    {
        return PathBuf::from(p);
    }
    data_dir.join("contextforge.db")
}

/// Record a custom database location (the database itself must already exist there).
pub fn set_database_location(data_dir: &Path, db_path: Option<&Path>) -> Result<()> {
    let pointer = location_file(data_dir);
    match db_path {
        Some(p) => {
            let json = serde_json::json!({ "database_path": p.to_string_lossy() });
            write_atomic(&pointer, serde_json::to_string_pretty(&json)?.as_bytes())?;
        }
        None => {
            if pointer.exists() {
                std::fs::remove_file(pointer)?;
            }
        }
    }
    Ok(())
}

/// Write a file atomically: temp file in the same directory, flush, then rename over the target.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let dir = path.parent().ok_or_else(|| Error::invalid("path has no parent directory"))?;
    std::fs::create_dir_all(dir)?;
    let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{file_name}.{}.tmp", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub struct OpenOptions {
    /// How long a writer waits for a competing writer before failing with `busy`.
    pub busy_timeout: Duration,
}

impl OpenOptions {
    /// Interactive use (desktop app, CLI, MCP server).
    pub fn interactive() -> Self {
        OpenOptions { busy_timeout: Duration::from_millis(5000) }
    }
    /// Lifecycle hooks: keep agents responsive even under contention.
    pub fn hook() -> Self {
        OpenOptions { busy_timeout: Duration::from_millis(1500) }
    }
}

/// The main handle to ContextForge storage. One per process (or per thread).
pub struct Store {
    pub(crate) conn: Connection,
    data_dir: PathBuf,
    db_path: PathBuf,
}

impl Store {
    /// Open (creating and migrating if needed) the database in the default data directory.
    pub fn open_default(opts: OpenOptions) -> Result<Store> {
        let dir = data_dir();
        let db = database_path(&dir);
        Store::open_at(&dir, &db, opts)
    }

    /// Open the database inside an explicit data directory (tests, portable mode).
    pub fn open_in(data_dir: &Path, opts: OpenOptions) -> Result<Store> {
        let db = database_path(data_dir);
        Store::open_at(data_dir, &db, opts)
    }

    pub fn open_at(data_dir: &Path, db_path: &Path, opts: OpenOptions) -> Result<Store> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::create_dir_all(data_dir)?;
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(db_path, flags)?;
        configure(&conn, opts)?;
        let mut store = Store { conn, data_dir: data_dir.to_path_buf(), db_path: db_path.to_path_buf() };
        store.migrate()?;
        Ok(store)
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Direct access for advanced read-only queries (diagnostics, tests).
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Changes whenever another connection commits; cheap to poll for live UI refresh.
    pub fn data_version(&self) -> Result<i64> {
        Ok(self.conn.query_row("PRAGMA data_version", [], |r| r.get(0))?)
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self.conn.query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |r| r.get(0))?)
    }

    /// Run `f` inside a write transaction that takes the write lock up front (avoids lock-upgrade deadlocks).
    pub(crate) fn write_tx<T>(&mut self, f: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T>) -> Result<T> {
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }

    fn migrate(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                applied_at TEXT NOT NULL
            );",
        )?;
        let current: i64 = self.conn.query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |r| r.get(0))?;
        if current > SCHEMA_VERSION {
            return Err(Error::Conflict(format!(
                "database schema v{current} is newer than this ContextForge build (v{SCHEMA_VERSION}); please update ContextForge"
            )));
        }
        if current == SCHEMA_VERSION {
            return Ok(());
        }
        for (version, name, sql) in MIGRATIONS {
            // Each migration runs in its own immediate transaction; a concurrent process that loses the race
            // waits on the busy timeout and then sees the migration as applied.
            let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let applied: bool =
                tx.query_row("SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)", [version], |r| r.get(0))?;
            if !applied {
                tx.execute_batch(sql)?;
                tx.execute(
                    "INSERT INTO schema_migrations(version, name, applied_at) VALUES (?1, ?2, ?3)",
                    rusqlite::params![version, name, util::now_ts()],
                )?;
                tracing::info!(version, name, "applied migration");
            }
            tx.commit()?;
        }
        Ok(())
    }
}

fn configure(conn: &Connection, opts: OpenOptions) -> Result<()> {
    conn.busy_timeout(opts.busy_timeout)?;
    let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        // Switching to WAL needs a brief exclusive lock; retry a few times under contention.
        let mut attempts = 0;
        loop {
            match conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0)) {
                Ok(_) => break,
                Err(e) if crate::error::is_busy(&e) && attempts < 20 => {
                    attempts += 1;
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA synchronous = NORMAL;
         PRAGMA temp_store = MEMORY;
         PRAGMA trusted_schema = OFF;",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_migrates_and_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in(dir.path(), OpenOptions::interactive()).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        let mode: String = store.conn.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
        let fk: i64 = store.conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        assert_eq!(fk, 1);
        drop(store);
        let again = Store::open_in(dir.path(), OpenOptions::interactive()).unwrap();
        assert_eq!(again.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn fts5_is_available() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in(dir.path(), OpenOptions::interactive()).unwrap();
        let n: i64 = store.conn.query_row("SELECT count(*) FROM memories_fts", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn location_pointer_redirects_database() {
        let dir = tempfile::tempdir().unwrap();
        let custom = dir.path().join("elsewhere").join("cf.db");
        set_database_location(dir.path(), Some(&custom)).unwrap();
        assert_eq!(database_path(dir.path()), custom);
        set_database_location(dir.path(), None).unwrap();
        assert_eq!(database_path(dir.path()), dir.path().join("contextforge.db"));
    }

    #[test]
    fn concurrent_openers_migrate_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let handles: Vec<_> = (0..6)
            .map(|_| {
                let p = path.clone();
                std::thread::spawn(move || Store::open_in(&p, OpenOptions::interactive()).map(|_| ()))
            })
            .collect();
        for h in handles {
            h.join().unwrap().unwrap();
        }
        let store = Store::open_in(&path, OpenOptions::interactive()).unwrap();
        let n: i64 = store.conn.query_row("SELECT count(*) FROM schema_migrations", [], |r| r.get(0)).unwrap();
        assert_eq!(n, MIGRATIONS.len() as i64);
    }
}
