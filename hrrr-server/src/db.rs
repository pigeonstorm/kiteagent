use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex};

const MIGRATIONS: &[&str] = &[
    r#"
    CREATE TABLE IF NOT EXISTS forecast_cache (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        lat         REAL NOT NULL,
        lon         REAL NOT NULL,
        run_date    TEXT NOT NULL,
        run_cycle   INTEGER NOT NULL,
        fetched_at  TEXT NOT NULL,
        valid_from  TEXT NOT NULL,
        valid_to    TEXT NOT NULL,
        raw_json    TEXT NOT NULL,
        UNIQUE(lat, lon, run_date, run_cycle)
    );
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS request_log (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        ts          TEXT NOT NULL,
        ip          TEXT NOT NULL,
        path        TEXT NOT NULL,
        lat         REAL,
        lon         REAL,
        cache_hit   INTEGER NOT NULL,
        status_code INTEGER NOT NULL,
        duration_ms INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_request_log_ip_ts ON request_log(ip, ts);
    CREATE INDEX IF NOT EXISTS idx_request_log_ts    ON request_log(ts);
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS error_log (
        id     INTEGER PRIMARY KEY AUTOINCREMENT,
        ts     TEXT NOT NULL,
        kind   TEXT NOT NULL,
        detail TEXT NOT NULL
    );
    "#,
];

#[derive(Clone)]
pub struct Db(Arc<Mutex<Connection>>);

impl Db {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path).context("failed to open SQLite database")?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")?;
        run_migrations(&conn)?;
        Ok(Self(Arc::new(Mutex::new(conn))))
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().context("failed to open in-memory database")?;
        run_migrations(&conn)?;
        Ok(Self(Arc::new(Mutex::new(conn))))
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.0.lock().unwrap()
    }

    // ── forecast_cache ──────────────────────────────────────────────────

    pub fn get_cached_forecast(
        &self,
        lat: f64,
        lon: f64,
        run_date: &str,
        run_cycle: i32,
    ) -> Result<Option<CachedForecast>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, fetched_at, valid_from, valid_to, raw_json \
             FROM forecast_cache \
             WHERE lat = ?1 AND lon = ?2 AND run_date = ?3 AND run_cycle = ?4 \
             LIMIT 1",
        )?;
        let mut rows = stmt.query(rusqlite::params![lat, lon, run_date, run_cycle])?;
        if let Some(row) = rows.next()? {
            return Ok(Some(CachedForecast {
                id: row.get(0)?,
                fetched_at: row.get(1)?,
                valid_from: row.get(2)?,
                valid_to: row.get(3)?,
                raw_json: row.get(4)?,
            }));
        }
        Ok(None)
    }

    pub fn upsert_forecast_cache(
        &self,
        lat: f64,
        lon: f64,
        run_date: &str,
        run_cycle: i32,
        fetched_at: &str,
        valid_from: &str,
        valid_to: &str,
        raw_json: &str,
    ) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO forecast_cache (lat, lon, run_date, run_cycle, fetched_at, valid_from, valid_to, raw_json) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
             ON CONFLICT(lat, lon, run_date, run_cycle) DO UPDATE SET \
               fetched_at = excluded.fetched_at, \
               valid_from = excluded.valid_from, \
               valid_to   = excluded.valid_to, \
               raw_json   = excluded.raw_json",
            rusqlite::params![lat, lon, run_date, run_cycle, fetched_at, valid_from, valid_to, raw_json],
        )
        .context("upsert forecast cache")?;
        Ok(conn.last_insert_rowid())
    }

    pub fn cache_entry_count(&self) -> Result<i64> {
        let conn = self.conn();
        Ok(conn.query_row("SELECT COUNT(*) FROM forecast_cache", [], |r| r.get(0))?)
    }

    pub fn last_cache_entry(&self) -> Result<Option<CachedForecast>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, fetched_at, valid_from, valid_to, raw_json \
             FROM forecast_cache ORDER BY id DESC LIMIT 1",
        )?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            return Ok(Some(CachedForecast {
                id: row.get(0)?,
                fetched_at: row.get(1)?,
                valid_from: row.get(2)?,
                valid_to: row.get(3)?,
                raw_json: row.get(4)?,
            }));
        }
        Ok(None)
    }

    pub fn prune_old_cache(&self, keep_days: i64) -> Result<usize> {
        let conn = self.conn();
        let deleted = conn.execute(
            "DELETE FROM forecast_cache WHERE fetched_at < datetime('now', ?1)",
            [format!("-{keep_days} days")],
        )?;
        Ok(deleted)
    }

    // ── request_log ─────────────────────────────────────────────────────

    pub fn log_request(
        &self,
        ip: &str,
        path: &str,
        lat: Option<f64>,
        lon: Option<f64>,
        cache_hit: bool,
        status_code: u16,
        duration_ms: u64,
    ) -> Result<()> {
        let conn = self.conn();
        let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        conn.execute(
            "INSERT INTO request_log (ts, ip, path, lat, lon, cache_hit, status_code, duration_ms) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![ts, ip, path, lat, lon, if cache_hit { 1 } else { 0 }, status_code as i32, duration_ms as i64],
        )?;
        Ok(())
    }

    pub fn count_requests_for_ip_since(&self, ip: &str, since: &str) -> Result<i64> {
        let conn = self.conn();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM request_log WHERE ip = ?1 AND ts > ?2",
            rusqlite::params![ip, since],
            |r| r.get(0),
        )?)
    }

    pub fn requests_last_24h(&self) -> Result<i64> {
        let conn = self.conn();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM request_log WHERE ts > datetime('now', '-24 hours')",
            [],
            |r| r.get(0),
        )?)
    }

    pub fn requests_last_1h(&self) -> Result<i64> {
        let conn = self.conn();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM request_log WHERE ts > datetime('now', '-1 hour')",
            [],
            |r| r.get(0),
        )?)
    }

    pub fn cache_hit_rate_24h(&self) -> Result<f64> {
        let conn = self.conn();
        let (total, hits): (i64, i64) = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(cache_hit), 0) FROM request_log \
             WHERE ts > datetime('now', '-24 hours') AND path = '/forecast'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if total == 0 {
            return Ok(0.0);
        }
        Ok(hits as f64 / total as f64 * 100.0)
    }

    pub fn requests_by_hour_24h(&self) -> Result<Vec<i64>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT strftime('%H', ts) AS hr, COUNT(*) \
             FROM request_log WHERE ts > datetime('now', '-24 hours') \
             GROUP BY hr ORDER BY hr",
        )?;
        let mut counts = vec![0i64; 24];
        let rows = stmt.query_map([], |r| {
            let hr: String = r.get(0)?;
            let cnt: i64 = r.get(1)?;
            Ok((hr, cnt))
        })?;
        for row in rows {
            let (hr, cnt) = row?;
            if let Ok(h) = hr.parse::<usize>() {
                if h < 24 {
                    counts[h] = cnt;
                }
            }
        }
        Ok(counts)
    }

    pub fn rate_limited_last_24h(&self) -> Result<i64> {
        let conn = self.conn();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM request_log WHERE ts > datetime('now', '-24 hours') AND status_code = 429",
            [],
            |r| r.get(0),
        )?)
    }

    pub fn top_callers_24h(&self, limit: usize) -> Result<Vec<(String, i64)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT ip, COUNT(*) AS cnt FROM request_log \
             WHERE ts > datetime('now', '-24 hours') \
             GROUP BY ip ORDER BY cnt DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn prune_old_requests(&self, keep_days: i64) -> Result<usize> {
        let conn = self.conn();
        let deleted = conn.execute(
            "DELETE FROM request_log WHERE ts < datetime('now', ?1)",
            [format!("-{keep_days} days")],
        )?;
        Ok(deleted)
    }

    // ── error_log ───────────────────────────────────────────────────────

    pub fn log_error(&self, kind: &str, detail: &str) -> Result<()> {
        let conn = self.conn();
        let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        conn.execute(
            "INSERT INTO error_log (ts, kind, detail) VALUES (?1, ?2, ?3)",
            rusqlite::params![ts, kind, detail],
        )?;
        Ok(())
    }

    pub fn errors_last_24h(&self) -> Result<i64> {
        let conn = self.conn();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM error_log WHERE ts > datetime('now', '-24 hours')",
            [],
            |r| r.get(0),
        )?)
    }

    pub fn recent_errors(&self, limit: usize) -> Result<Vec<ErrorEntry>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT ts, kind, detail FROM error_log ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |r| {
            Ok(ErrorEntry {
                ts: r.get(0)?,
                kind: r.get(1)?,
                detail: r.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn prune_old_errors(&self, keep_days: i64) -> Result<usize> {
        let conn = self.conn();
        let deleted = conn.execute(
            "DELETE FROM error_log WHERE ts < datetime('now', ?1)",
            [format!("-{keep_days} days")],
        )?;
        Ok(deleted)
    }
}

fn run_migrations(conn: &Connection) -> Result<()> {
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        conn.execute_batch(sql)
            .with_context(|| format!("migration {} failed", i))?;
    }
    Ok(())
}

#[derive(Debug)]
pub struct CachedForecast {
    pub id: i64,
    pub fetched_at: String,
    pub valid_from: String,
    pub valid_to: String,
    pub raw_json: String,
}

#[derive(Debug, serde::Serialize)]
pub struct ErrorEntry {
    pub ts: String,
    pub kind: String,
    pub detail: String,
}
