require "rake"
require "fileutils"

BIN_DIR = "target/release"
DEV_BIN = "target/debug"
CONFIG = "config.toml"

# Binaries
AGENT_BIN = "#{BIN_DIR}/kiteagent-agent"
SERVER_BIN = "#{BIN_DIR}/kiteagent-server"
HRRR_BIN  = "#{BIN_DIR}/hrrr-server"
LIVE_BIN  = "#{BIN_DIR}/live-server"

AGENT_DEV = "#{DEV_BIN}/kiteagent-agent"
SERVER_DEV = "#{DEV_BIN}/kiteagent-server"
HRRR_DEV  = "#{DEV_BIN}/hrrr-server"
LIVE_DEV  = "#{DEV_BIN}/live-server"

LOG_DIR = "logs"

def spawn_all_dev
  FileUtils.mkdir_p(LOG_DIR)
  pids = []
  pids << spawn("cargo run -p kiteagent-server", out: ["#{LOG_DIR}/server.log", "a"], err: [:child, :out])
  pids << spawn("cargo run -p hrrr-server", out: ["#{LOG_DIR}/hrrr.log", "a"], err: [:child, :out])
  pids << spawn("cargo run -p live-server", out: ["#{LOG_DIR}/live.log", "a"], err: [:child, :out])
  pids << spawn("cargo run -p kiteagent-agent -- #{CONFIG}", out: ["#{LOG_DIR}/agent.log", "a"], err: [:child, :out])
  pids
end

def spawn_all_prod
  FileUtils.mkdir_p(LOG_DIR)
  pids = []
  pids << spawn(SERVER_BIN, out: ["#{LOG_DIR}/server.log", "a"], err: [:child, :out])
  pids << spawn(HRRR_BIN, out: ["#{LOG_DIR}/hrrr.log", "a"], err: [:child, :out])
  pids << spawn(LIVE_BIN, out: ["#{LOG_DIR}/live.log", "a"], err: [:child, :out])
  pids << spawn("#{AGENT_BIN} #{CONFIG}", out: ["#{LOG_DIR}/agent.log", "a"], err: [:child, :out])
  pids
end

def kill_pids(pids)
  pids.each { |pid| Process.kill("TERM", pid) rescue nil }
end

# Prime local SQLite before services start (Windy Point HRRR cache + one live scrape).
def dev_initial_pulls
  sh HRRR_DEV, "pull"
  sh LIVE_DEV, "pull"
end

# HTTP port from [server] bind in config.toml (e.g. 0.0.0.0:8080 → 8080).
def dev_kiteagent_http_port
  return "8080" unless File.exist?(CONFIG)

  in_server = false
  File.foreach(CONFIG) do |line|
    s = line.strip
    if s == "[server]"
      in_server = true
      next
    end
    if s.start_with?("[") && s != "[server]"
      in_server = false
    end
    next unless in_server

    if (m = line[/^\s*bind\s*=\s*"[^"]*:(\d+)"/, 1])
      return m
    end
  end
  "8080"
end

def dev_print_service_urls
  dash = dev_kiteagent_http_port
  # spawn does not set BIND / GRPC_BIND; binaries use defaults unless you export them before rake.
  hrrr_port = "8081"
  live_http_port = "8082"
  live_grpc = ENV["GRPC_BIND"]&.split(":")&.last || "50051"

  puts ""
  puts "─" * 60
  puts "Dev services starting (logs in #{LOG_DIR}/; first boot may take a few seconds). Open:"
  puts "  kiteagent-server   http://localhost:#{dash}/"
  puts "  hrrr-server        http://localhost:#{hrrr_port}/"
  puts "  live-server HTTP   http://localhost:#{live_http_port}/"
  puts "  live-server gRPC   localhost:#{live_grpc}"
  puts "  kiteagent-agent    (no HTTP; uses #{CONFIG})"
  puts "─" * 60
  puts ""
end

# ═══════════════════════════════════════════════════════════════════════════════
# Help
# ═══════════════════════════════════════════════════════════════════════════════

HELP_TEXT = <<~HELP
  Kiteagent Rakefile — build and run services

  SERVICES (ports):
    server  8080  Push server, WebPush, dashboard
    hrrr    8081  HRRR forecast API (NOAA NOMADS)
    live    8082  Live wind scraper (ARL:UT Lake Travis), gRPC :50051
    agent   —     Weather agent (uses config.toml, connects to above)

  NAMESPACES

  dev:
    rake dev:build   Build debug (cargo build)
    rake dev:run     pull + spawn all; prints local URLs for server, hrrr, live, agent note (bg)
    rake dev:watch   Auto-reload server + agent, opens browser

  prod:
    rake prod:build  Build release (cargo build --release)
    rake prod:run    Run all release binaries

  SINGLE-SERVICE

    rake server      Start push server only
    rake agent       Start agent only (needs server, hrrr running)
    rake hrrr        Start HRRR API only
    rake live        Start live-server only

  CLI (after cargo build):
    #{HRRR_DEV} pull   Fetch/cache Windy Point HRRR (default db hrrr.db)
    #{LIVE_DEV} pull   Scrape ARL station once (default db live.db)

  CONVENIENCE

    rake run         Alias for dev:run
    rake build       Release build
    rake kite-gear   Build kite-gear WASM (wasm-pack → kite-gear/pkg/; needed for /kite-gear.js)
    rake wasm        Same as kite-gear
    rake hrrr:dev    HRRR with cargo-watch
    rake live:dev    Live-server with cargo-watch
    rake agent:dev   Agent with cargo-watch

  DIAGNOSTICS

    rake diagnose:push  Check push subscriptions, VAPID key
                        consistency, and server reachability

  LIST TASKS

    rake -T          Show all tasks with descriptions
HELP

desc "Show usage and available tasks"
task :help do
  puts HELP_TEXT
end

task default: :help

# ═══════════════════════════════════════════════════════════════════════════════
# Dev namespace: debug build, cargo run
# ═══════════════════════════════════════════════════════════════════════════════

namespace :dev do
  desc "Build all crates (debug)"
  task :build do
    sh "cargo build"
  end

  desc "Run all services (server, hrrr, live, agent in bg)"
  task run: [:build] do
    dev_initial_pulls
    pids = spawn_all_dev
    dev_print_service_urls
    at_exit { kill_pids(pids) }
    sleep
  ensure
    kill_pids(pids)
  end

  desc "Dev mode: auto-reload server + agent (original dev flow)"
  task :watch do
    server_pid = spawn("cargo watch -w server/src -w shared/src -w server/static -x 'run -p kiteagent-server'")
    at_exit { Process.kill("TERM", server_pid) rescue nil }
    sleep 6
    system %(open "cursor://vscode.simple-browser/show?url=http://localhost:8080?user=victor")
    sh "cargo watch -w agent/src -w shared/src -x 'run -p kiteagent-agent -- #{CONFIG}'"
  ensure
    Process.kill("TERM", server_pid) rescue nil
  end
end

# ═══════════════════════════════════════════════════════════════════════════════
# Prod namespace: release build, run binaries
# ═══════════════════════════════════════════════════════════════════════════════

namespace :prod do
  desc "Build all crates (release)"
  task :build do
    sh "cargo build --release"
  end

  desc "Run all services (release binaries)"
  task run: [:build] do
    pids = spawn_all_prod
    at_exit { kill_pids(pids) }
    sleep
  ensure
    kill_pids(pids)
  end
end

# ═══════════════════════════════════════════════════════════════════════════════
# Single-service tasks (convenience)
# ═══════════════════════════════════════════════════════════════════════════════

desc "Build release binaries"
task :build do
  sh "cargo build --release"
end

desc "Build kite-gear WASM for the server UI (wasm-pack → kite-gear/pkg/)"
task :"kite-gear" do
  sh "wasm-pack build --target web kite-gear"
end

desc "Alias for rake kite-gear"
task wasm: :"kite-gear"

desc "Run all services (dev mode). Use: rake dev:run"
task run: "dev:run"

desc "Start the push server (foreground, dev mode)"
task :server do
  sh "cargo run -p kiteagent-server"
end

desc "Start the weather agent (foreground, dev mode)"
task :agent do
  sh "cargo run -p kiteagent-agent -- #{CONFIG}"
end

desc "Start the HRRR API server (foreground, dev mode)"
task :hrrr do
  sh "cargo run -p hrrr-server"
end

desc "Start the live weather scraper/API server (foreground, dev mode)"
task :live do
  sh "cargo run -p live-server"
end

desc "Dev mode for HRRR server with auto-reload"
task "hrrr:dev" do
  sh "cargo watch -w hrrr-server/src -w hrrr-server/static -x 'run -p hrrr-server'"
end

desc "Dev mode for live-server with auto-reload"
task "live:dev" do
  sh "cargo watch -w live-server/src -w live-server/static -x 'run -p live-server'"
end

desc "Dev mode for agent with auto-reload"
task "agent:dev" do
  sh "cargo watch -w agent/src -w shared/src -x 'run -p kiteagent-agent -- #{CONFIG}'"
end

# ═══════════════════════════════════════════════════════════════════════════════
# Push notification diagnostics
# ═══════════════════════════════════════════════════════════════════════════════

namespace :diagnose do
  desc "Check push subscriptions and VAPID key consistency"
  task :push do
    require "toml-rb" if Gem.loaded_specs["toml-rb"]
    require "json"

    db_path = nil
    push_secret = nil
    server_url = nil

    # Parse config.toml for db_path, push_secret, server_url
    if File.exist?(CONFIG)
      cfg = File.read(CONFIG)
      db_path      = cfg[/^\s*db_path\s*=\s*"([^"]+)"/, 1] || "kiteagent.db"
      push_secret  = cfg[/^\s*push_secret\s*=\s*"([^"]+)"/, 1]
      server_url   = cfg[/^\s*server_url\s*=\s*"([^"]+)"/, 1] || "http://localhost:8080"
    else
      abort "#{CONFIG} not found"
    end

    puts "━━━ Push Notification Diagnostics ━━━"
    puts

    # ── #6: Check DB exists and has subscribers ────────────────────────────────
    puts "▶ Database: #{db_path}"
    unless File.exist?(db_path)
      puts "  ✗ Database file not found! The server has never run from this directory,"
      puts "    or db_path in config.toml is wrong."
      puts "    Hint: check the working directory of your server process."
      puts
    else
      size = (File.size(db_path) / 1024.0).round(1)
      puts "  ✓ Exists (#{size} KB)"

      sub_count = `sqlite3 "#{db_path}" "SELECT COUNT(*) FROM push_subscriptions;" 2>/dev/null`.strip
      if sub_count.empty?
        puts "  ✗ Could not query push_subscriptions (is sqlite3 installed?)"
      elsif sub_count == "0"
        puts "  ✗ No push subscriptions! Open the PWA and subscribe first."
      else
        puts "  ✓ #{sub_count} subscriber(s)"

        # List endpoints (truncated)
        endpoints = `sqlite3 "#{db_path}" "SELECT endpoint FROM push_subscriptions;" 2>/dev/null`.strip.split("\n")
        endpoints.each do |ep|
          domain = ep[%r{https?://([^/]+)}, 1] || ep[0..60]
          apple  = ep.include?("push.apple.com") ? " (Apple)" : ""
          puts "    • #{domain}#{apple}"
        end
      end
      puts

      # Check for recent push errors in server logs
      notif_count = `sqlite3 "#{db_path}" "SELECT COUNT(*) FROM notifications_sent;" 2>/dev/null`.strip
      last_notif  = `sqlite3 "#{db_path}" "SELECT sent_at FROM notifications_sent ORDER BY id DESC LIMIT 1;" 2>/dev/null`.strip
      puts "  Notifications sent: #{notif_count} total"
      puts "  Last notification:  #{last_notif.empty? ? '(never)' : last_notif}"
      puts
    end

    # ── #5: Check VAPID key consistency ────────────────────────────────────────
    vapid_file = "vapid_keys.json"
    puts "▶ VAPID keys: #{vapid_file}"
    if File.exist?(vapid_file)
      mtime = File.mtime(vapid_file).strftime("%Y-%m-%d %H:%M:%S")
      puts "  ✓ Exists (last modified: #{mtime})"

      # Warn if VAPID keys were regenerated after existing subscriptions
      if File.exist?(db_path)
        oldest_sub = `sqlite3 "#{db_path}" "SELECT created_at FROM push_subscriptions ORDER BY id ASC LIMIT 1;" 2>/dev/null`.strip
        unless oldest_sub.empty?
          sub_time = Time.parse(oldest_sub) rescue nil
          if sub_time && File.mtime(vapid_file) > sub_time
            puts "  ⚠ VAPID keys were modified AFTER the oldest subscription!"
            puts "    Subscriptions created before #{mtime} are now STALE."
            puts "    Fix: have all users unsubscribe and re-subscribe."
          else
            puts "  ✓ VAPID keys are older than all subscriptions (consistent)"
          end
        end
      end
    else
      puts "  ✗ #{vapid_file} not found — the server will generate new keys on next start."
      puts "    Any existing subscriptions will become invalid."
    end
    puts

    # ── Live server check ──────────────────────────────────────────────────────
    puts "▶ Server reachability: #{server_url}"
    begin
      status_json = `curl -sf "#{server_url}/status" 2>/dev/null`
      if status_json.empty?
        puts "  ✗ Server not responding at #{server_url}/status"
      else
        status = JSON.parse(status_json)
        puts "  ✓ Running (v#{status['version']}, #{status['subscribers']} subscriber(s))"
        if status["errors_last_24h"].to_i > 0
          puts "  ⚠ #{status['errors_last_24h']} error(s) in the last 24 hours"
        end
      end
    rescue => e
      puts "  ✗ Could not reach server: #{e.message}"
    end
    puts

    puts "━━━ Done ━━━"
  end
end
