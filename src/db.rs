use dirs::data_local_dir;
use rusqlite::{params, Connection, Result};
use std::fs::create_dir_all;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct AppSummary {
    pub app_id: String,
    pub duration_sec: i64,
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open() -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let base_dir = data_local_dir().ok_or("Could not locate local data directory")?;
        let waytime_dir = base_dir.join("waytime");
        create_dir_all(&waytime_dir)?;

        let db_path: PathBuf = waytime_dir.join("data.db");
        let conn = Connection::open(db_path)?;

        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS activity_intervals (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                app_id TEXT NOT NULL,
                window_title TEXT NOT NULL,
                started_at INTEGER NOT NULL,
                ended_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_intervals_time ON activity_intervals(started_at, ended_at);
            CREATE INDEX IF NOT EXISTS idx_intervals_app ON activity_intervals(app_id);",
        )?;

        Ok(Self { conn })
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

    pub fn get_summary_since(&self, since_ts: i64) -> Result<Vec<AppSummary>, rusqlite::Error> {
        let now = chrono::Local::now().timestamp();
        let mut stmt = self.conn.prepare(
            "SELECT app_id, SUM(MAX(0, MIN(ended_at, ?2) - MAX(started_at, ?1))) AS total_duration
             FROM activity_intervals
             WHERE ended_at >= ?1
             GROUP BY app_id
             HAVING total_duration > 0
             ORDER BY total_duration DESC",
        )?;

        let rows = stmt.query_map(params![since_ts, now], |row| {
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
}
