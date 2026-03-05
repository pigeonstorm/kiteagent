require "rake"

BIN_DIR = "target/release"
AGENT_BIN = "#{BIN_DIR}/kiteagent-agent"
SERVER_BIN = "#{BIN_DIR}/kiteagent-server"
HRRR_BIN  = "#{BIN_DIR}/hrrr-server"
LIVE_BIN  = "#{BIN_DIR}/live-server"
CONFIG = "config.toml"

desc "Build both binaries (release)"
task :build do
  sh "cargo build --release"
end

desc "Start the push server (foreground, dev mode via cargo run)"
task :server do
  sh "cargo run -p kiteagent-server"
end

desc "Start the weather agent (foreground, dev mode via cargo run)"
task :agent do
  sh "cargo run -p kiteagent-agent -- #{CONFIG}"
end

desc "Build release binaries then start both (server bg, agent fg)"
task :run => [:build] do
  pid = spawn(SERVER_BIN)
  at_exit { Process.kill("TERM", pid) rescue nil }
  sleep 1
  sh "#{AGENT_BIN} #{CONFIG}"
ensure
  Process.kill("TERM", pid) rescue nil
end

desc "Dev mode: auto-reload both on source changes (cargo-watch)"
task :dev do
  server_pid = spawn("cargo watch -w server/src -w shared/src -w server/static -x 'run -p kiteagent-server'")
  at_exit { Process.kill("TERM", server_pid) rescue nil }
  sleep 6
  system %(open "cursor://vscode.simple-browser/show?url=http://localhost:8080?user=victor")
  sh "cargo watch -w agent/src -w shared/src -x 'run -p kiteagent-agent -- #{CONFIG}'"
ensure
  Process.kill("TERM", server_pid) rescue nil
end

desc "Start the HRRR API server (foreground, dev mode)"
task :hrrr do
  sh "cargo run -p hrrr-server"
end

desc "Dev mode for HRRR server with auto-reload"
task "hrrr:dev" do
  sh "cargo watch -w hrrr-server/src -w hrrr-server/static -x 'run -p hrrr-server'"
end

desc "Start the live weather scraper/API server (foreground, dev mode)"
task :live do
  sh "cargo run -p live-server"
end

desc "Dev mode for live-server with auto-reload"
task "live:dev" do
  sh "cargo watch -w live-server/src -w live-server/static -x 'run -p live-server'"
end
