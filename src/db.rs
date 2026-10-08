use dirs::data_local_dir;
use rusqlite::{Connection, Result, params};
use std::collections::BTreeSet;
use std::fs::create_dir_all;
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migration {
    pub version: i64,
    pub sql: &'static str,
}

pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: "CREATE TABLE IF NOT EXISTS activity_intervals (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            app_id TEXT NOT NULL,
            window_title TEXT NOT NULL,
            started_at INTEGER NOT NULL,
            ended_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_intervals_time ON activity_intervals(started_at, ended_at);
        CREATE INDEX IF NOT EXISTS idx_intervals_app ON activity_intervals(app_id);",
}];

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedMigration {
    pub version: i64,
    pub applied_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppSummary {
    pub app_id: String,
    pub duration_sec: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TitleSummary {
    pub title: String,
    pub duration_sec: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppDetailedSummary {
    pub app_id: String,
    pub total_duration_sec: i64,
    pub titles: Vec<TitleSummary>,
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open() -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let base_dir = data_local_dir().ok_or("Could not locate local data directory")?;
        let waytime_dir = base_dir.join("waytime");
        let db_path: PathBuf = waytime_dir.join("data.db");
        Self::open_at(&db_path)
    }

    pub fn open_at(db_path: &Path) -> std::result::Result<Self, Box<dyn std::error::Error>> {
        if let Some(parent) = db_path.parent()
            && !parent.as_os_str().is_empty()
        {
            create_dir_all(parent)?;
        }

        let mut conn = Connection::open(db_path)?;

        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;

        Self::run_migrations(&mut conn)?;

        Ok(Self { conn })
    }

    #[allow(dead_code)]
    pub fn migrations() -> &'static [Migration] {
        MIGRATIONS
    }

    pub fn run_migrations(conn: &mut Connection) -> Result<()> {
        Self::run_migrations_list(conn, MIGRATIONS)
    }

    pub fn run_migrations_list(conn: &mut Connection, migrations: &[Migration]) -> Result<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS _schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;

        let applied = Self::get_applied_versions(conn)?;

        let mut sorted_migrations = migrations.to_vec();
        sorted_migrations.sort_by_key(|m| m.version);

        for migration in sorted_migrations {
            if !applied.contains(&migration.version) {
                let tx = conn.transaction()?;
                tx.execute_batch(migration.sql)?;
                tx.execute(
                    "INSERT INTO _schema_migrations (version) VALUES (?1)",
                    params![migration.version],
                )?;
                tx.commit()?;
            }
        }

        Ok(())
    }

    pub fn get_applied_versions(conn: &Connection) -> Result<BTreeSet<i64>> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS _schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;

        let mut stmt =
            conn.prepare("SELECT version FROM _schema_migrations ORDER BY version ASC")?;
        let rows = stmt.query_map([], |row| row.get::<_, i64>(0))?;
        let mut set = BTreeSet::new();
        for r in rows {
            set.insert(r?);
        }
        Ok(set)
    }

    #[allow(dead_code)]
    pub fn get_applied_migrations(conn: &Connection) -> Result<Vec<AppliedMigration>> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS _schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;

        let mut stmt = conn
            .prepare("SELECT version, applied_at FROM _schema_migrations ORDER BY version ASC")?;
        let rows = stmt.query_map([], |row| {
            Ok(AppliedMigration {
                version: row.get(0)?,
                applied_at: row.get(1)?,
            })
        })?;
        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    #[allow(dead_code)]
    pub fn applied_migrations(&self) -> Result<Vec<i64>> {
        let set = Self::get_applied_versions(&self.conn)?;
        Ok(set.into_iter().collect())
    }

    #[allow(dead_code)]
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    #[allow(dead_code)]
    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    pub fn insert_interval(
        &self,
        app_id: &str,
        title: &str,
        started_at: i64,
        ended_at: i64,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO activity_intervals (app_id, window_title, started_at, ended_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![app_id, title, started_at, ended_at],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_interval_end(&self, id: i64, ended_at: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE activity_intervals SET ended_at = ?1 WHERE id = ?2",
            params![ended_at, id],
        )?;
        Ok(())
    }

    pub fn get_summary_range(
        &self,
        start_ts: i64,
        end_ts: i64,
    ) -> Result<Vec<AppSummary>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT app_id, SUM(MAX(0, MIN(ended_at, ?2) - MAX(started_at, ?1))) AS total_duration
             FROM activity_intervals
             WHERE ended_at >= ?1 AND started_at <= ?2
             GROUP BY app_id
             HAVING total_duration > 0
             ORDER BY total_duration DESC",
        )?;

        let rows = stmt.query_map(params![start_ts, end_ts], |row| {
            Ok(AppSummary {
                app_id: row.get(0)?,
                duration_sec: row.get(1)?,
            })
        })?;

        let mut summaries = Vec::new();
        for row in rows {
            summaries.push(row?);
        }
        Ok(summaries)
    }

    pub fn get_detailed_summary_range(
        &self,
        start_ts: i64,
        end_ts: i64,
    ) -> Result<Vec<AppDetailedSummary>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT app_id, window_title, SUM(MAX(0, MIN(ended_at, ?2) - MAX(started_at, ?1))) AS total_duration
             FROM activity_intervals
             WHERE ended_at >= ?1 AND started_at <= ?2
             GROUP BY app_id, window_title
             HAVING total_duration > 0
             ORDER BY app_id, total_duration DESC",
        )?;

        let rows = stmt.query_map(params![start_ts, end_ts], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;

        let mut apps: Vec<AppDetailedSummary> = Vec::new();
        for row in rows {
            let (app_id, title, duration_sec) = row?;
            if let Some(existing) = apps.iter_mut().find(|a| a.app_id == app_id) {
                existing.total_duration_sec += duration_sec;
                existing.titles.push(TitleSummary {
                    title,
                    duration_sec,
                });
            } else {
                apps.push(AppDetailedSummary {
                    app_id,
                    total_duration_sec: duration_sec,
                    titles: vec![TitleSummary {
                        title,
                        duration_sec,
                    }],
                });
            }
        }

        apps.sort_by_key(|a| std::cmp::Reverse(a.total_duration_sec));

        for app in &mut apps {
            app.titles
                .sort_by_key(|a| std::cmp::Reverse(a.duration_sec));
        }

        Ok(apps)
    }

    #[allow(dead_code)]
    pub fn get_summary_since(&self, since_ts: i64) -> Result<Vec<AppSummary>, rusqlite::Error> {
        self.get_summary_range(since_ts, chrono::Local::now().timestamp())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_db_path(prefix: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("waytime_{prefix}_{nonce}.db"))
    }

    #[test]
    fn test_schema_migrations_and_initial_schema() {
        let db_path = temp_db_path("test_schema_migrations");
        let db = Database::open_at(&db_path).expect("failed to open database");

        // Verify _schema_migrations exists and has version 1
        let applied = db
            .applied_migrations()
            .expect("failed to get applied migrations");
        assert_eq!(applied, vec![1]);

        let records = Database::get_applied_migrations(db.connection())
            .expect("failed to get migration records");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].version, 1);
        assert!(!records[0].applied_at.is_empty());

        // Verify activity_intervals table exists and works
        let row_id = db
            .insert_interval("firefox", "Mozilla Firefox", 1000, 1050)
            .expect("insert failed");
        assert!(row_id > 0);

        let summaries = db.get_summary_range(900, 1100).expect("query failed");
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].app_id, "firefox");
        assert_eq!(summaries[0].duration_sec, 50);

        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn test_migration_runner_is_idempotent() {
        let db_path = temp_db_path("test_idempotent");
        {
            let mut db = Database::open_at(&db_path).expect("failed to open database");
            assert_eq!(db.applied_migrations().unwrap(), vec![1]);

            // Running migrations again on the same DB connection is a no-op
            Database::run_migrations(db.connection_mut()).expect("second run_migrations failed");
            assert_eq!(db.applied_migrations().unwrap(), vec![1]);

            db.insert_interval("terminal", "kitty", 500, 600)
                .expect("insert failed");
        }

        // Reopen database from disk
        {
            let db = Database::open_at(&db_path).expect("reopen failed");
            assert_eq!(db.applied_migrations().unwrap(), vec![1]);

            // Existing data remains intact
            let summaries = db.get_summary_range(400, 700).expect("query failed");
            assert_eq!(summaries.len(), 1);
            assert_eq!(summaries[0].app_id, "terminal");
            assert_eq!(summaries[0].duration_sec, 100);
        }

        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn test_pre_existing_activity_intervals_zero_data_loss() {
        let db_path = temp_db_path("test_pre_existing_data");

        // Simulate an existing database created before migrations existed
        {
            let raw_conn = Connection::open(&db_path).expect("raw open failed");
            raw_conn
                .execute_batch(
                    "CREATE TABLE activity_intervals (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        app_id TEXT NOT NULL,
                        window_title TEXT NOT NULL,
                        started_at INTEGER NOT NULL,
                        ended_at INTEGER NOT NULL
                    );
                    CREATE INDEX idx_intervals_time ON activity_intervals(started_at, ended_at);
                    CREATE INDEX idx_intervals_app ON activity_intervals(app_id);
                    INSERT INTO activity_intervals (app_id, window_title, started_at, ended_at)
                    VALUES ('neovim', 'src/db.rs', 100, 200);",
                )
                .expect("raw schema creation failed");
        }

        // Open with waytime Database - runs migration 1 without data loss
        let db = Database::open_at(&db_path).expect("Database::open_at failed");
        assert_eq!(db.applied_migrations().unwrap(), vec![1]);

        let summaries = db.get_summary_range(50, 250).expect("query failed");
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].app_id, "neovim");
        assert_eq!(summaries[0].duration_sec, 100);

        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn test_wal_mode_and_busy_timeout_intact() {
        let db_path = temp_db_path("test_pragmas");
        let db = Database::open_at(&db_path).expect("open failed");

        let journal_mode: String = db
            .connection()
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .expect("query journal_mode failed");
        assert_eq!(journal_mode.to_lowercase(), "wal");

        let busy_timeout: i64 = db
            .connection()
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
            .expect("query busy_timeout failed");
        assert_eq!(busy_timeout, 5000);

        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn test_transaction_rollback_on_failed_migration() {
        let db_path = temp_db_path("test_rollback");
        let mut conn = Connection::open(&db_path).expect("open failed");

        let test_migrations = [
            Migration {
                version: 1,
                sql: "CREATE TABLE test_table_1 (id INTEGER PRIMARY KEY);",
            },
            Migration {
                version: 2,
                sql: "THIS IS INVALID SQL STATEMENT AND SHOULD FAIL;",
            },
        ];

        let result = Database::run_migrations_list(&mut conn, &test_migrations);
        assert!(result.is_err());

        // Version 1 should have succeeded, but Version 2 must NOT be recorded
        let applied = Database::get_applied_versions(&conn).expect("get_applied failed");
        assert!(applied.contains(&1));
        assert!(!applied.contains(&2));

        let _ = fs::remove_file(db_path);
    }
}
