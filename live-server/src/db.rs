use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use std::sync::{Arc, Mutex};

use crate::WeatherReading;

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Db { conn: Arc::new(Mutex::new(conn)) };
        db.init()?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Db { conn: Arc::new(Mutex::new(conn)) };
        db.init()?;
        Ok(db)
    }

    fn init(&self) -> Result<()> {
        self.conn.lock().unwrap().execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS weather_readings (
                id                   INTEGER PRIMARY KEY AUTOINCREMENT,
                scraped_at           TEXT    NOT NULL,
                station_time         TEXT,
                wind_speed_mph       REAL,
                wind_direction       TEXT,
                wind_direction_deg   INTEGER,
                wind_avg_mph         REAL,
                wind_hi_mph          REAL,
                wind_hi_dir_deg      INTEGER,
                wind_rms_mph         REAL,
                wind_vector_avg_mph  REAL,
                wind_vector_dir_deg  INTEGER,
                temperature_f        REAL,
                humidity_pct         REAL,
                barometer_inhg       REAL,
                barometer_trend      REAL,
                rain_in              REAL,
                rain_rate_in_hr      REAL,
                wind_chill_f         REAL,
                heat_index_f         REAL,
                dewpoint_f           REAL
            );
            "#,
        )?;
        Ok(())
    }

    pub fn insert_reading(&self, r: &WeatherReading) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"INSERT INTO weather_readings (
                scraped_at, station_time,
                wind_speed_mph, wind_direction, wind_direction_deg,
                wind_avg_mph, wind_hi_mph, wind_hi_dir_deg,
                wind_rms_mph, wind_vector_avg_mph, wind_vector_dir_deg,
                temperature_f, humidity_pct,
                barometer_inhg, barometer_trend,
                rain_in, rain_rate_in_hr,
                wind_chill_f, heat_index_f, dewpoint_f
            ) VALUES (
                ?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20
            )"#,
            params![
                r.scraped_at.to_rfc3339(),
                r.station_time,
                r.wind_speed_mph,
                r.wind_direction,
                r.wind_direction_deg,
                r.wind_avg_mph,
                r.wind_hi_mph,
                r.wind_hi_dir_deg,
                r.wind_rms_mph,
                r.wind_vector_avg_mph,
                r.wind_vector_dir_deg,
                r.temperature_f,
                r.humidity_pct,
                r.barometer_inhg,
                r.barometer_trend,
                r.rain_in,
                r.rain_rate_in_hr,
                r.wind_chill_f,
                r.heat_index_f,
                r.dewpoint_f,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_latest(&self) -> Result<Option<WeatherReading>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT * FROM weather_readings ORDER BY id DESC LIMIT 1",
        )?;
        let mut rows = stmt.query_map([], row_to_reading)?;
        Ok(rows.next().transpose()?)
    }

    pub fn get_history(&self, limit: i64) -> Result<Vec<WeatherReading>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT * FROM weather_readings ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit], row_to_reading)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(anyhow::Error::from);
        rows
    }

    /// Total number of readings stored.
    pub fn count(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM weather_readings",
            [],
            |r| r.get(0),
        )?)
    }
}

fn row_to_reading(row: &rusqlite::Row) -> rusqlite::Result<WeatherReading> {
    let scraped_at_str: String = row.get(1)?;
    let scraped_at = DateTime::parse_from_rfc3339(&scraped_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    Ok(WeatherReading {
        id: Some(row.get(0)?),
        scraped_at,
        station_time: row.get(2)?,
        wind_speed_mph: row.get(3)?,
        wind_direction: row.get(4)?,
        wind_direction_deg: row.get(5)?,
        wind_avg_mph: row.get(6)?,
        wind_hi_mph: row.get(7)?,
        wind_hi_dir_deg: row.get(8)?,
        wind_rms_mph: row.get(9)?,
        wind_vector_avg_mph: row.get(10)?,
        wind_vector_dir_deg: row.get(11)?,
        temperature_f: row.get(12)?,
        humidity_pct: row.get(13)?,
        barometer_inhg: row.get(14)?,
        barometer_trend: row.get(15)?,
        rain_in: row.get(16)?,
        rain_rate_in_hr: row.get(17)?,
        wind_chill_f: row.get(18)?,
        heat_index_f: row.get(19)?,
        dewpoint_f: row.get(20)?,
    })
}
