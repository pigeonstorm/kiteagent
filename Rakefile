require "rake"

BIN_DIR = "target/release"
AGENT_BIN = "#{BIN_DIR}/kiteagent-agent"
SERVER_BIN = "#{BIN_DIR}/kiteagent-server"
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
