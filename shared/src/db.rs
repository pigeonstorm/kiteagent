use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex};

const MIGRATIONS: &[&str] = &[
    r#"
    CREATE TABLE IF NOT EXISTS forecasts (
        id         INTEGER PRIMARY KEY AUTOINCREMENT,
        fetched_at TEXT NOT NULL,
        source     TEXT NOT NULL,
        valid_from TEXT NOT NULL,
        valid_to   TEXT NOT NULL,
        raw_json   TEXT NOT NULL,
        fetch_ok   INTEGER NOT NULL
    );
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS analysis_runs (
        id            INTEGER PRIMARY KEY AUTOINCREMENT,
        forecast_id   INTEGER NOT NULL REFERENCES forecasts(id),
        analyzed_at   TEXT NOT NULL,
        windows_found INTEGER NOT NULL,
        result_json   TEXT NOT NULL
    );
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS notifications_sent (
        id           INTEGER PRIMARY KEY AUTOINCREMENT,
        sent_at      TEXT NOT NULL,
        window_start TEXT NOT NULL,
        window_end   TEXT NOT NULL,
        wind_avg_kn  REAL NOT NULL,
        wind_dir_deg REAL NOT NULL,
        disciplines  TEXT NOT NULL,
        message_hash TEXT NOT NULL
    );
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS push_subscriptions (
        id         INTEGER PRIMARY KEY AUTOINCREMENT,
        created_at TEXT NOT NULL,
        endpoint   TEXT NOT NULL UNIQUE,
        p256dh     TEXT NOT NULL,
        auth       TEXT NOT NULL
    );
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS errors (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        occurred_at TEXT NOT NULL,
        source      TEXT NOT NULL,
        error       TEXT NOT NULL,
        details     TEXT
    );
    "#,
];

#[derive(Clone)]
pub struct Db(Arc<Mutex<Connection>>);

impl Db {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path).context("failed to open SQLite database")?;
        run_migrations(&conn)?;
        Ok(Self(Arc::new(Mutex::new(conn))))
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().context("failed to open in-memory database")?;
        run_migrations(&conn)?;
        Ok(Self(Arc::new(Mutex::new(conn))))
    }

    pub fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.0.lock().unwrap()
    }
}

pub fn run_migrations(conn: &Connection) -> Result<()> {
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        conn.execute_batch(sql)
            .with_context(|| format!("migration {} failed", i))?;
    }
    Ok(())
}

// --- Forecast operations ---

#[derive(Debug)]
pub struct ForecastRow {
    pub id: i64,
    pub fetched_at: String,
    pub source: String,
    pub valid_from: String,
    pub valid_to: String,
    pub raw_json: String,
    pub fetch_ok: bool,
}

impl Db {
    pub fn insert_forecast(
        &self,
        fetched_at: &str,
        source: &str,
        valid_from: &str,
        valid_to: &str,
        raw_json: &str,
        fetch_ok: bool,
    ) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO forecasts (fetched_at, source, valid_from, valid_to, raw_json, fetch_ok) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![fetched_at, source, valid_from, valid_to, raw_json, if fetch_ok { 1 } else { 0 }],
        )
        .context("insert forecast")?;
        Ok(conn.last_insert_rowid())
    }

    pub fn last_forecast(&self) -> Result<Option<ForecastRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, fetched_at, source, valid_from, valid_to, raw_json, fetch_ok \
             FROM forecasts ORDER BY id DESC LIMIT 1",
        )?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            let fetched_at: String = row.get(1)?;
            let source: String = row.get(2)?;
            let valid_from: String = row.get(3)?;
            let valid_to: String = row.get(4)?;
            let raw_json: String = row.get(5)?;
            let fetch_ok: i32 = row.get(6)?;
            return Ok(Some(ForecastRow {
                id,
                fetched_at,
                source,
                valid_from,
                valid_to,
                raw_json,
                fetch_ok: fetch_ok != 0,
            }));
        }
        Ok(None)
    }
}

// --- Analysis run operations ---

impl Db {
    pub fn insert_analysis_run(
        &self,
        forecast_id: i64,
        analyzed_at: &str,
        windows_found: i32,
        result_json: &str,
    ) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO analysis_runs (forecast_id, analyzed_at, windows_found, result_json) \
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![forecast_id, analyzed_at, windows_found, result_json],
        )
        .context("insert analysis run")?;
        Ok(conn.last_insert_rowid())
    }

    pub fn last_analysis(&self) -> Result<Option<(String, i32, String)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT analyzed_at, windows_found, result_json \
             FROM analysis_runs ORDER BY id DESC LIMIT 1",
        )?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            return Ok(Some((row.get(0)?, row.get(1)?, row.get(2)?)));
        }
        Ok(None)
    }
}

// --- Notification dedup operations ---

impl Db {
    pub fn notification_recently_sent(&self, message_hash: &str, cooldown_hours: i64) -> Result<bool> {
        let conn = self.conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM notifications_sent \
             WHERE message_hash = ?1 AND sent_at > datetime('now', ?2)",
            rusqlite::params![message_hash, format!("-{} hours", cooldown_hours)],
            |r| r.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn notifications_count_today(&self) -> Result<i64> {
        let conn = self.conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM notifications_sent WHERE date(sent_at) = date('now', 'localtime')",
            [],
            |r| r.get(0),
        )?;
        Ok(count)
    }

    pub fn insert_notification_sent(
        &self,
        sent_at: &str,
        window_start: &str,
        window_end: &str,
        wind_avg_kn: f64,
        wind_dir_deg: f64,
        disciplines: &str,
        message_hash: &str,
    ) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO notifications_sent (sent_at, window_start, window_end, wind_avg_kn, wind_dir_deg, disciplines, message_hash) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![sent_at, window_start, window_end, wind_avg_kn, wind_dir_deg, disciplines, message_hash],
        )
        .context("insert notification sent")?;
        Ok(conn.last_insert_rowid())
    }

    pub fn last_notification_sent(&self) -> Result<Option<(String, String)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT sent_at, window_start FROM notifications_sent ORDER BY id DESC LIMIT 1",
        )?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            return Ok(Some((row.get(0)?, row.get(1)?)));
        }
        Ok(None)
    }
}

// --- Push subscription operations ---

#[derive(Debug, Clone)]
pub struct PushSubscriptionRow {
    pub id: i64,
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
}

impl Db {
    pub fn insert_push_subscription(&self, endpoint: &str, p256dh: &str, auth: &str) -> Result<i64> {
        let conn = self.conn();
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        conn.execute(
            "INSERT OR REPLACE INTO push_subscriptions (created_at, endpoint, p256dh, auth) \
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![now, endpoint, p256dh, auth],
        )
        .context("insert push subscription")?;
        Ok(conn.last_insert_rowid())
    }

    pub fn all_push_subscriptions(&self) -> Result<Vec<PushSubscriptionRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT id, endpoint, p256dh, auth FROM push_subscriptions")?;
        let rows = stmt.query_map([], |row| {
            Ok(PushSubscriptionRow {
                id: row.get(0)?,
                endpoint: row.get(1)?,
                p256dh: row.get(2)?,
                auth: row.get(3)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn delete_push_subscription_by_endpoint(&self, endpoint: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM push_subscriptions WHERE endpoint = ?1", [endpoint])?;
        Ok(())
    }

    pub fn subscribers_count(&self) -> Result<i64> {
        let conn = self.conn();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM push_subscriptions", [], |r| r.get(0))?;
        Ok(count)
    }
}

// --- Error operations ---

impl Db {
    pub fn insert_error(&self, source: &str, error: &str, details: Option<&str>) -> Result<i64> {
        let conn = self.conn();
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        conn.execute(
            "INSERT INTO errors (occurred_at, source, error, details) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![now, source, error, details],
        )
        .context("insert error")?;
        Ok(conn.last_insert_rowid())
    }

    pub fn errors_count_last_24h(&self) -> Result<i64> {
        let conn = self.conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM errors WHERE occurred_at > datetime('now', '-24 hours')",
            [],
            |r| r.get(0),
        )?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Db {
        Db::open_in_memory().unwrap()
    }

    #[test]
    fn insert_and_query_forecast() {
        let db = test_db();
        let id = db
            .insert_forecast(
                "2026-02-24T16:00:00Z",
                "open-meteo-hrrr",
                "2026-02-24T18:00:00",
                "2026-02-26T18:00:00",
                "{}",
                true,
            )
            .unwrap();
        assert!(id > 0);
        let last = db.last_forecast().unwrap().unwrap();
        assert_eq!(last.source, "open-meteo-hrrr");
        assert_eq!(last.fetch_ok, true);
    }

    #[test]
    fn analysis_run_references_forecast() {
        let db = test_db();
        db.insert_forecast("2026-02-24T16:00:00Z", "nws", "a", "b", "{}", true).unwrap();
        let id = db
            .insert_analysis_run(1, "2026-02-24T16:00:03Z", 2, r#"[{"start":"a","end":"b"}]"#)
            .unwrap();
        assert!(id > 0);
        let last = db.last_analysis().unwrap().unwrap();
        assert_eq!(last.1, 2);
    }

    #[test]
    fn notification_dedup_query_returns_recent_hash() {
        let db = test_db();
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        db.insert_notification_sent(&now, "start", "end", 18.0, 210.0, "kitefoil", "abc123").unwrap();
        let recent = db.notification_recently_sent("abc123", 4).unwrap();
        assert!(recent);
    }

    #[test]
    fn stale_notification_not_returned_by_dedup_query() {
        let db = test_db();
        let old = "2020-01-01T00:00:00Z";
        db.insert_notification_sent(old, "start", "end", 18.0, 210.0, "kitefoil", "oldhash").unwrap();
        let recent = db.notification_recently_sent("oldhash", 4).unwrap();
        assert!(!recent);
    }

    #[test]
    fn error_insert_and_count_last_24h() {
        let db = test_db();
        db.insert_error("weather_fetch", "timeout", None).unwrap();
        let count = db.errors_count_last_24h().unwrap();
        assert!(count >= 1);
    }
}
