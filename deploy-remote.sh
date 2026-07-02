#!/usr/bin/env bash
# deploy-remote.sh — Cross-compile and deploy proxy-pool-rust to a remote Linux server
#
# Usage:
#   ./deploy-remote.sh                          # Build + deploy with defaults
#   TARGET_HOST=user@1.2.3.4 ./deploy-remote.sh # Specify target host
#   SKIP_BUILD=1 ./deploy-remote.sh             # Skip build, only deploy existing binary
#
# Prerequisites:
#   - cargo-cross installed: cargo install cross --git https://github.com/cross-rs/cross
#   - Docker running (cross uses Docker containers for compilation)
#   - SSH access to target server
#   - Target server has Redis running

set -euo pipefail

# ── Configuration ──────────────────────────────────────
TARGET_HOST="${TARGET_HOST:-}"
TARGET_PORT="${TARGET_PORT:-22}"
REMOTE_DIR="${REMOTE_DIR:-/opt/proxy-pool}"
BINARY_NAME="proxy-server"
CONFIG_FILE="${CONFIG_FILE:-config/settings.yaml}"

# ── Colors ─────────────────────────────────────────────
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

info()  { echo -e "${GREEN}[INFO]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*"; exit 1; }

# ── Pre-flight checks ─────────────────────────────────
if [ -z "$TARGET_HOST" ]; then
    error "TARGET_HOST not set. Usage: TARGET_HOST=user@1.2.3.4 ./deploy-remote.sh"
fi

command -v cross >/dev/null 2>&1 || error "cargo-cross not installed. Run: cargo install cross --git https://github.com/cross-rs/cross"
command -v ssh >/dev/null 2>&1 || error "ssh not found"
command -v scp >/dev/null 2>&1 || error "scp not found"

# ── Step 1: Cross-compile ─────────────────────────────
if [ "${SKIP_BUILD:-0}" != "1" ]; then
    info "Cross-compiling for x86_64-unknown-linux-gnu..."
    cross build --release -p proxy-server --target x86_64-unknown-linux-gnu
    info "Build complete: target/x86_64-unknown-linux-gnu/release/${BINARY_NAME}"
else
    warn "Skipping build (SKIP_BUILD=1)"
fi

BINARY_PATH="target/x86_64-unknown-linux-gnu/release/${BINARY_NAME}"
if [ ! -f "$BINARY_PATH" ]; then
    error "Binary not found at $BINARY_PATH. Run without SKIP_BUILD first."
fi

BINARY_SIZE=$(du -h "$BINARY_PATH" | cut -f1)
info "Binary size: ${BINARY_SIZE}"

# ── Step 2: Upload binary ─────────────────────────────
info "Uploading binary to ${TARGET_HOST}:${REMOTE_DIR}..."
ssh -p "$TARGET_PORT" "$TARGET_HOST" "mkdir -p ${REMOTE_DIR}"
scp -P "$TARGET_PORT" "$BINARY_PATH" "${TARGET_HOST}:${REMOTE_DIR}/${BINARY_NAME}"

# Upload config if it doesn't exist on remote
ssh -p "$TARGET_PORT" "$TARGET_HOST" "test -f ${REMOTE_DIR}/settings.yaml || echo 'Config not found on remote'"
if [ -f "$CONFIG_FILE" ]; then
    warn "Local config file found at ${CONFIG_FILE}, but not auto-uploaded to avoid overwriting server config."
    warn "To upload manually: scp -P ${TARGET_PORT} ${CONFIG_FILE} ${TARGET_HOST}:${REMOTE_DIR}/settings.yaml"
fi

# ── Step 3: Restart service ───────────────────────────
info "Restarting proxy-pool service on remote..."
ssh -p "$TARGET_PORT" "$TARGET_HOST" bash -s << 'REMOTE_SCRIPT'
set -e
BINARY="/opt/proxy-pool/proxy-server"
CONFIG="/opt/proxy-pool/settings.yaml"

chmod +x "$BINARY"

# Try systemd first, fall back to direct restart
if systemctl is-active proxy-pool >/dev/null 2>&1; then
    echo "Restarting via systemctl..."
    sudo systemctl restart proxy-pool
    sleep 2
    sudo systemctl status proxy-pool --no-pager -l
elif pgrep -f "$BINARY" >/dev/null 2>&1; then
    echo "Killing existing process..."
    pkill -f "$BINARY" || true
    sleep 1
    echo "Starting proxy-server..."
    nohup "$BINARY" "$CONFIG" > /opt/proxy-pool/proxy-pool.log 2>&1 &
    echo "Started with PID: $!"
    sleep 2
    tail -5 /opt/proxy-pool/proxy-pool.log
else
    echo "No existing process found. Starting proxy-server..."
    nohup "$BINARY" "$CONFIG" > /opt/proxy-pool/proxy-pool.log 2>&1 &
    echo "Started with PID: $!"
    sleep 2
    tail -5 /opt/proxy-pool/proxy-pool.log
fi
REMOTE_SCRIPT

info "Deployment complete!"
info "Verify: curl -s http://<server-ip>:8000/api/status"
