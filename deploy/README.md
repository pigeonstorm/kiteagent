# KiteAgent Deployment — ARM64 Graviton 3

## Cross-compile from macOS

```bash
# Install target
rustup target add aarch64-unknown-linux-gnu

# On macOS you may need a linker for the target
# brew install FiloSottile/musl-cross/musl-cross

# Build both binaries
cargo build --release --target aarch64-unknown-linux-gnu

# Binaries will be at:
# target/aarch64-unknown-linux-gnu/release/kiteagent-agent
# target/aarch64-unknown-linux-gnu/release/kiteagent-server
```

## Deploy to EC2 Graviton 3

```bash
# Copy binaries and config
scp target/aarch64-unknown-linux-gnu/release/kiteagent-agent ec2-user@YOUR_INSTANCE:~/
scp target/aarch64-unknown-linux-gnu/release/kiteagent-server ec2-user@YOUR_INSTANCE:~/
scp config.toml ec2-user@YOUR_INSTANCE:~/

# On the instance:
sudo mkdir -p /opt/kiteagent
sudo useradd -r -s /bin/false kiteagent 2>/dev/null || true
sudo mv ~/kiteagent-agent ~/kiteagent-server ~/config.toml /opt/kiteagent/
sudo chown -R kiteagent:kiteagent /opt/kiteagent
sudo chmod +x /opt/kiteagent/kiteagent-agent /opt/kiteagent/kiteagent-server

# Install systemd units (run from project root)
sudo cp deploy/kiteagent-server.service deploy/kiteagent-agent.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable kiteagent-server kiteagent-agent
sudo systemctl start kiteagent-server kiteagent-agent
```

## Nginx TLS (required for Web Push)

Service workers require HTTPS. Put Nginx in front with Let's Encrypt:

```bash
# Install certbot and nginx
sudo yum install -y nginx certbot python3-certbot-nginx

# Get certificate (interactive)
sudo certbot certonly --nginx -d kite.yourdomain.com

# Add the config from deploy/nginx.conf to your server block
# Then reload nginx
sudo systemctl reload nginx
```

## Verify

- Server: `curl http://localhost:8080/status`
- Agent: check logs with `journalctl -u kiteagent-agent -f`
