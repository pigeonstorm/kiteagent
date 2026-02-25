# KiteAgent

A Rust-based kitesurf weather agent that monitors conditions at Windy Point, Lake Travis, Austin TX and sends browser push notifications with gear recommendations when it's time to ride.

## Quick Start

```bash
# Build both binaries
cargo build --release

# 1. Start the server (handles Web Push subscriptions + /push endpoint)
./target/release/kiteagent-server &

# 2. Start the agent (fetches forecast hourly, evaluates, sends notifications)
./target/release/kiteagent-agent config.toml
```

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

## Tests

```bash
cargo test --workspace
```
