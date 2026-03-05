# KiteAgent

A Rust-based kitesurf weather agent that monitors conditions at Windy Point, Lake Travis, Austin TX and sends browser push notifications with gear recommendations when it's time to ride.

## Quick Start

The easiest way to run both binaries together is via the Rake tasks:

```bash
# Dev mode — builds on every change, server in background, agent in foreground
rake dev

# Release mode — builds optimised binaries once, then runs both
rake run

# Or start each process individually (dev builds)
rake server   # Axum HTTP server only
rake agent    # Weather agent only
```

Manual invocation without Rake:

```bash
cargo build --release

./target/release/kiteagent-server &
./target/release/kiteagent-agent config.toml
```

## Crates

| Crate | Binary | Intent |
|---|---|---|
| `shared` | — | Internal library shared by `agent` and `server`. Holds config types (parsed from `config.toml`), the SQLite `Db` wrapper, and schema migrations. |
| `agent` | `kiteagent-agent` | Weather daemon. Fetches Open-Meteo HRRR forecasts on a cron schedule, evaluates rideable wind windows against the rider's gear inventory, and POSTs Web Push notifications to `kiteagent-server`. |
| `server` | `kiteagent-server` | Axum HTTP server. Manages Web Push subscriptions, generates VAPID keys on first run, and exposes `/subscribe`, `/push`, `/status`, and the PWA subscription page. |
| `hrrr-server` | `hrrr-server` | Standalone forecast cache. Downloads NOAA HRRR GRIB2 files, parses them with the `grib` crate, stores results in SQLite, and serves an HTTP API plus a simple dashboard for raw forecast inspection. |
| `live-server` | `live-server` | Real-time conditions server. Scrapes live wind and atmosphere readings from the ARL:UT Lake Travis station, stores them in SQLite, and exposes both a REST API (Axum) and a gRPC service (tonic/prost, defined in `proto/live.proto`) for querying the latest and historical readings. |

## Components

- **kiteagent-server** — Axum HTTP server with `/subscribe`, `/push`, `/status`, and subscription page. Generates VAPID keys on first run.
- **kiteagent-agent** — Weather daemon that fetches Open-Meteo HRRR, evaluates rideable windows, and POSTs notifications to the server.

## Configuration

Edit `config.toml` to set:

- Location (lat/lon for Windy Point)
- User weight (84 kg default) and gear inventory (kite sizes, sail sizes)
- Push server URL and secret
- Schedule (fetch interval, morning digest hour, cooldowns)

## Setup on Phone

1. Serve the app over HTTPS (required for Web Push). See `deploy/README.md`.
2. Open `https://your-domain/` in your browser.
3. Tap "Enable Kite Alerts" and allow notifications.
4. On iOS: add the page to Home Screen first (PWA), then enable notifications.

## Deploy (Graviton 3 / ARM64)

See [deploy/README.md](deploy/README.md) for cross-compilation, systemd units, and Nginx TLS.

## Inspecting the Database

The app uses SQLite. The database file is `kiteagent.db` in the project root (configurable via `config.toml` → `storage.db_path`).

### Option 1 — `sqlite3` CLI (quickest)

```bash
sqlite3 kiteagent.db
```

Useful commands once inside the shell:

```sql
.tables                -- list all tables
.mode column
.headers on

SELECT * FROM forecasts          ORDER BY id DESC LIMIT 5;
SELECT * FROM analysis_runs      ORDER BY id DESC LIMIT 5;
SELECT * FROM errors             ORDER BY id DESC LIMIT 20;
SELECT * FROM notifications_sent ORDER BY id DESC LIMIT 10;
SELECT * FROM push_subscriptions;
SELECT * FROM profile;
SELECT * FROM spots;
SELECT * FROM gear_items;
SELECT * FROM gear_wind_ranges;
```

Pretty-print a JSON column:

```bash
sqlite3 kiteagent.db "SELECT json_pretty(raw_json) FROM forecasts ORDER BY id DESC LIMIT 1;"
```

Type `.quit` to exit.

### Option 2 — DB Browser for SQLite (GUI)

```bash
brew install --cask db-browser-for-sqlite
```

Open the app, then **File → Open Database → `kiteagent.db`**.

### Option 3 — VS Code / Cursor extension

Install the **SQLite Viewer** extension (by Florian Klampfer) and click on `kiteagent.db` directly in the file explorer.

## Tests

```bash
cargo test --workspace
```
