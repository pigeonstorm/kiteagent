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

## Kite Gear WASM (required for `/kite-gear.js`)

The subscription UI loads `kite-gear.js` and WebAssembly from disk at runtime. The server looks for them under **`WorkingDirectory`** (`/opt/kiteagent` in the systemd unit) at `kite-gear/pkg/kite_gear.js` and `kite-gear/pkg/kite_gear_bg.wasm` (the latter is also exposed as `/kite-gear.wasm` and `/kite_gear_bg.wasm`).

Build the pkg on your build machine (needs [`wasm-pack`](https://rustwasm.github.io/wasm-pack/installer/)):

```bash
wasm-pack build --target web kite-gear
```

Deploy the generated files **with** the binary (the `pkg/` directory is gitignored; it is not in the repo):

```bash
scp -r kite-gear/pkg ec2-user@YOUR_INSTANCE:~/kite-gear-pkg
# On the instance, after mkdir /opt/kiteagent (see below):
sudo mkdir -p /opt/kiteagent/kite-gear
sudo mv ~/kite-gear-pkg /opt/kiteagent/kite-gear/pkg
sudo chown -R kiteagent:kiteagent /opt/kiteagent/kite-gear
```

If you skip this step, the browser console shows **404** on `kite-gear.js` and **WASM not available**.

## Deploy to EC2 Graviton 3

```bash
# Copy binaries and config
scp target/aarch64-unknown-linux-gnu/release/kiteagent-agent ec2-user@YOUR_INSTANCE:~/
scp target/aarch64-unknown-linux-gnu/release/kiteagent-server ec2-user@YOUR_INSTANCE:~/
scp target/aarch64-unknown-linux-gnu/release/hrrr-server ec2-user@YOUR_INSTANCE:~/
scp target/aarch64-unknown-linux-gnu/release/live-server ec2-user@YOUR_INSTANCE:~/
scp config.toml ec2-user@YOUR_INSTANCE:~/
scp deploy/hrrr-server.service deploy/live-server.service \
    deploy/kiteagent-server.service deploy/kiteagent-agent.service \
    ec2-user@YOUR_INSTANCE:~/
# Copy WASM/JS (see section above): kite-gear/pkg → /opt/kiteagent/kite-gear/pkg

# On the instance:
sudo mkdir -p /opt/kiteagent
sudo useradd -r -s /bin/false kiteagent 2>/dev/null || true
sudo mv ~/kiteagent-agent ~/kiteagent-server ~/hrrr-server ~/live-server ~/config.toml /opt/kiteagent/
sudo chown -R kiteagent:kiteagent /opt/kiteagent
sudo chmod +x /opt/kiteagent/kiteagent-agent /opt/kiteagent/kiteagent-server /opt/kiteagent/hrrr-server /opt/kiteagent/live-server
```

### Install or refresh systemd units (on the remote machine)

After the four `*.service` files are in your home directory on the instance (via `scp` above, or copy them there some other way), SSH in and run:

```bash
sudo install -m 644 \
  deploy/hrrr-server.service \
  deploy/live-server.service \
  deploy/live-server.service \
  deploy/kiteagent-server.service \
  deploy/kiteagent-agent.service \
  /etc/systemd/system/

sudo systemctl daemon-reload
sudo systemctl enable hrrr-server live-server kiteagent-server kiteagent-agent
sudo systemctl restart hrrr-server live-server kiteagent-server kiteagent-agent
```

To install only updated unit files without re-copying binaries, copy new files to `~` then run the same block (use `restart` so new `ExecStart` / `Environment` lines take effect).

Status and logs:

```bash
systemctl status hrrr-server live-server kiteagent-server kiteagent-agent
journalctl -u kiteagent-server -u kiteagent-agent -f
```

## Nginx TLS (required for Web Push)

Service workers require HTTPS. Put Nginx in front with Let's Encrypt:

```bash
# Install certbot and nginx
sudo yum install -y nginx certbot python3-certbot-nginx

# Get certificate (interactive)
sudo certbot certonly --nginx -d ka.pigeonstorm.com

# Add the config from deploy/nginx.conf to your server block
# Then reload nginx
sudo systemctl reload nginx
```

## Verify

- Server: `curl http://localhost:8080/status`
- Kite Gear assets (after TLS): `curl -sS -o /dev/null -w '%{http_code}\n' https://ka.pigeonstorm.com/kite-gear.js` should print `200`
- Agent: check logs with `journalctl -u kiteagent-agent -f`
