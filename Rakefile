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
  pids
end

def spawn_all_prod
  FileUtils.mkdir_p(LOG_DIR)
  pids = []
  pids << spawn(SERVER_BIN, out: ["#{LOG_DIR}/server.log", "a"], err: [:child, :out])
  pids << spawn(HRRR_BIN, out: ["#{LOG_DIR}/hrrr.log", "a"], err: [:child, :out])
  pids << spawn(LIVE_BIN, out: ["#{LOG_DIR}/live.log", "a"], err: [:child, :out])
  pids
end

def kill_pids(pids)
  pids.each { |pid| Process.kill("TERM", pid) rescue nil }
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
    rake dev:run     Run all: server, hrrr, live (bg) + agent (fg)
    rake dev:watch   Auto-reload server + agent, opens browser

  prod:
    rake prod:build  Build release (cargo build --release)
    rake prod:run    Run all release binaries

  SINGLE-SERVICE

    rake server      Start push server only
    rake agent       Start agent only (needs server, hrrr running)
    rake hrrr        Start HRRR API only
    rake live        Start live-server only

  CONVENIENCE

    rake run         Alias for dev:run
    rake build       Release build
    rake hrrr:dev   HRRR with cargo-watch
    rake live:dev    Live-server with cargo-watch

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

  desc "Run all services (server, hrrr, live in bg; agent in fg)"
  task run: [:build] do
    pids = spawn_all_dev
    at_exit { kill_pids(pids) }
    sleep 4
    sh "cargo run -p kiteagent-agent -- #{CONFIG}"
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
    sleep 2
    sh "#{AGENT_BIN} #{CONFIG}"
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
