#!/usr/bin/env bash
# Bamako sqld — deploy to shared server
#
# Usage:
#   ./deploy.sh                         # deploy to DEPLOY_SERVER
#   ./deploy.sh --setup                 # first-time setup (creates user, dirs, service)
#   ./deploy.sh --domain docs.my.com    # override domain (used during setup)
#
# Requires: DEPLOY_SERVER env var (or set below), SSH key access as root
#
# Server layout mirrors cosmicbizwitch pattern:
#   /opt/bamako/           — app root
#   /opt/bamako/sqld       — sqld binary
#   /opt/bamako/data/      — database files
#   /opt/bamako/backups/   — rolling backups
#   /opt/bamako/.env       — environment config (not deployed, created on server)
#   /opt/bamako/jwt-key    — JWT signing secret (generated once on server)

set -euo pipefail

DEPLOY_SERVER="${DEPLOY_SERVER:-}"
DEPLOY_USER="${DEPLOY_USER:-root}"
REMOTE_DIR="/opt/bamako"
SERVICE_NAME="bamako-sqld"
SQLD_PORT=8092
SQLD_VERSION="${SQLD_VERSION:-latest}"   # pin e.g. "0.24.21" or leave "latest"
SETUP=false
DOMAIN=""

for i in "$@"; do
  case $i in
    --setup)        SETUP=true ;;
    --domain=*)     DOMAIN="${i#*=}" ;;
    --domain)       DOMAIN="$2"; shift ;;
    --server=*)     DEPLOY_SERVER="${i#*=}" ;;
    --server)       DEPLOY_SERVER="$2"; shift ;;
  esac
done

if [ -z "$DEPLOY_SERVER" ]; then
  echo "Set DEPLOY_SERVER env var or pass --server <ip>"
  echo "  Example: DEPLOY_SERVER=159.223.0.1 ./deploy.sh"
  exit 1
fi

SSH="ssh -o ControlMaster=auto -o ControlPath=/tmp/bamako-deploy-%r@%h:%p -o ControlPersist=60 ${DEPLOY_USER}@${DEPLOY_SERVER}"

# ── Resolve latest sqld release ─────────────────────────────────────────────
resolve_sqld_url() {
  if [ "$SQLD_VERSION" = "latest" ]; then
    SQLD_VERSION=$(curl -s "https://api.github.com/repos/tursodatabase/libsql/releases/latest" \
      | grep '"tag_name"' | head -1 \
      | sed 's/.*"tag_name": *"//;s/".*//')
  fi
  # Strip leading 'v' if present, tag format is libsql-server-v0.24.x
  VER="${SQLD_VERSION#libsql-server-}"
  VER="${VER#v}"
  echo "https://github.com/tursodatabase/libsql/releases/download/libsql-server-v${VER}/libsql-server-x86_64-unknown-linux-gnu.tar.xz"
}

# ── First-time setup ─────────────────────────────────────────────────────────
if $SETUP; then
  echo "==> First-time setup on ${DEPLOY_SERVER}..."

  if [ -z "$DOMAIN" ]; then
    echo "Pass --domain your.domain.com for Caddy config"
    exit 1
  fi

  SQLD_URL=$(resolve_sqld_url)
  echo "    sqld release: ${SQLD_URL}"

  $SSH bash -s -- "$REMOTE_DIR" "$SERVICE_NAME" "$SQLD_PORT" "$SQLD_URL" "$DOMAIN" << 'REMOTE'
    set -euo pipefail
    REMOTE_DIR="$1"; SERVICE_NAME="$2"; SQLD_PORT="$3"; SQLD_URL="$4"; DOMAIN="$5"

    # Create user
    if ! id bamako &>/dev/null; then
      useradd --system --no-create-home --shell /usr/sbin/nologin bamako
      echo "Created user: bamako"
    fi

    # Create directories
    mkdir -p "${REMOTE_DIR}/data" "${REMOTE_DIR}/backups" "${REMOTE_DIR}/logs"
    chown -R bamako:bamako "${REMOTE_DIR}"

    # Download sqld binary
    echo "Downloading sqld..."
    TMP=$(mktemp -d)
    curl -fL "$SQLD_URL" -o "${TMP}/sqld.tar.xz"
    tar -xJf "${TMP}/sqld.tar.xz" -C "${TMP}"
    SQLD_BIN=$(find "$TMP" -name "sqld" -type f | head -1)
    if [ -z "$SQLD_BIN" ]; then
      echo "ERROR: sqld binary not found in archive"
      ls -la "$TMP"
      exit 1
    fi
    install -m 755 "$SQLD_BIN" "${REMOTE_DIR}/sqld"
    rm -rf "$TMP"
    echo "Installed: ${REMOTE_DIR}/sqld ($(${REMOTE_DIR}/sqld --version 2>&1 | head -1))"

    # Generate Ed25519 keypair for JWT auth (once)
    if [ ! -f "${REMOTE_DIR}/jwt-key" ]; then
      openssl genpkey -algorithm ed25519 -outform PEM -out "${REMOTE_DIR}/jwt-private.pem"
      # Public key PEM → sqld jwt-key-file
      openssl pkey -in "${REMOTE_DIR}/jwt-private.pem" -pubout -out "${REMOTE_DIR}/jwt-key"
      chmod 600 "${REMOTE_DIR}/jwt-key" "${REMOTE_DIR}/jwt-private.pem"
      chown bamako:bamako "${REMOTE_DIR}/jwt-key" "${REMOTE_DIR}/jwt-private.pem"
      echo "Generated Ed25519 keypair."
    fi

    # Generate admin token for auth service (once)
    ADMIN_TOKEN_FILE="${REMOTE_DIR}/admin-token"
    if [ ! -f "$ADMIN_TOKEN_FILE" ]; then
      openssl rand -hex 32 > "$ADMIN_TOKEN_FILE"
      chmod 600 "$ADMIN_TOKEN_FILE"
      chown bamako:bamako "$ADMIN_TOKEN_FILE"
      echo "Generated admin token."
    fi

    # Extract Ed25519 seed (private key bytes) for auth service env
    # PKCS8 DER is 48 bytes: 16-byte header + 32-byte seed
    PRIV_SEED=$(openssl pkey -in "${REMOTE_DIR}/jwt-private.pem" -outform DER \
      | tail -c 32 | base64 | tr '+/' '-_' | tr -d '=\n')

    # Create auth.env
    if [ ! -f "${REMOTE_DIR}/auth.env" ]; then
      cat > "${REMOTE_DIR}/auth.env" << AUTHENV
JWT_PRIVATE_KEY=${PRIV_SEED}
ADMIN_TOKEN=$(cat $ADMIN_TOKEN_FILE)
ALLOWLIST_PATH=${REMOTE_DIR}/allowlist.json
PORT=8091
AUTHENV
      chown bamako:bamako "${REMOTE_DIR}/auth.env"
      chmod 600 "${REMOTE_DIR}/auth.env"
      echo "Created auth.env"
    fi

    # Create .env if missing
    if [ ! -f "${REMOTE_DIR}/.env" ]; then
      cat > "${REMOTE_DIR}/.env" << ENV
SQLD_PORT=${SQLD_PORT}
SQLD_NODE=primary
ENV
      chown bamako:bamako "${REMOTE_DIR}/.env"
      chmod 600 "${REMOTE_DIR}/.env"
      echo "Created .env"
    fi

    # Caddy: route /auth/* to auth service, rest to sqld
    CADDY_CONF="/etc/caddy/conf.d/bamako.caddy"
    mkdir -p /etc/caddy/conf.d
    if [ ! -f "$CADDY_CONF" ]; then
      cat > "$CADDY_CONF" << CADDY
${DOMAIN} {
    handle /auth/* {
        reverse_proxy localhost:8091
    }
    handle {
        reverse_proxy localhost:${SQLD_PORT}
    }
}
CADDY
      echo "Created Caddy config: ${CADDY_CONF}"
      if systemctl is-active caddy >/dev/null 2>&1; then
        systemctl reload caddy && echo "Caddy reloaded."
      else
        echo "WARNING: Caddy not running — add import manually."
      fi
    else
      echo "Caddy config already exists: ${CADDY_CONF}"
    fi
REMOTE

  # Build auth service binary for linux/amd64
  echo "==> Building auth service..."
  GOOS=linux GOARCH=amd64 go build -o /tmp/bamako-auth ./services/auth/
  scp /tmp/bamako-auth "${DEPLOY_USER}@${DEPLOY_SERVER}:${REMOTE_DIR}/bamako-auth"
  $SSH "chmod +x ${REMOTE_DIR}/bamako-auth && chown bamako:bamako ${REMOTE_DIR}/bamako-auth"
  rm -f /tmp/bamako-auth

  # Install auth service
  scp bamako-auth.service "${DEPLOY_USER}@${DEPLOY_SERVER}:/etc/systemd/system/bamako-auth.service"

  # Copy and install sqld service file
  scp bamako-sqld.service "${DEPLOY_USER}@${DEPLOY_SERVER}:/etc/systemd/system/${SERVICE_NAME}.service"
  $SSH systemctl daemon-reload
  $SSH systemctl enable "${SERVICE_NAME}" bamako-auth
  $SSH systemctl start "${SERVICE_NAME}" bamako-auth
  $SSH systemctl status "${SERVICE_NAME}" bamako-auth --no-pager

  # Generate owner invite (includes admin_token so owner can manage invites from the app)
  ADMIN_TOKEN=$($SSH "cat ${REMOTE_DIR}/admin-token")
  OWNER_INVITE=$(python3 -c "
import base64, json
payload = json.dumps({
  'v': 2,
  'serverUrl': 'https://${DOMAIN}',
  'name': 'shared',
  'auth': 'google',
  'permissionLevel': 'owner',
  'admin_token': '${ADMIN_TOKEN}'
})
print(base64.b64encode(payload.encode()).decode())
")

  echo ""
  echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  echo "  Setup complete!"
  echo "  Server: https://${DOMAIN}"
  echo ""
  echo "  OWNER INVITE (paste into Bamako app → Settings → Paste Invite):"
  echo ""
  echo "  ${OWNER_INVITE}"
  echo ""
  echo "  Add your Google email to the allowlist first:"
  echo "    curl -X POST https://${DOMAIN}/auth/invites \\"
  echo "      -H 'Authorization: Bearer ${ADMIN_TOKEN}' \\"
  echo "      -H 'Content-Type: application/json' \\"
  echo "      -d '{\"email\":\"you@gmail.com\"}'"
  echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  exit 0
fi

# ── Regular deploy (update sqld binary) ─────────────────────────────────────
echo "==> Deploying sqld to ${DEPLOY_SERVER}..."

echo "Step 1: Stop service"
$SSH "systemctl stop ${SERVICE_NAME} || true"

echo "Step 2: Backup data"
$SSH bash -s -- "$REMOTE_DIR" << 'REMOTE'
  set -euo pipefail
  REMOTE_DIR="$1"
  TIMESTAMP=$(date +%Y%m%d_%H%M%S)
  BACKUP_FILE="${REMOTE_DIR}/backups/data_${TIMESTAMP}.tar.gz"
  if [ -d "${REMOTE_DIR}/data" ]; then
    tar -czf "$BACKUP_FILE" -C "${REMOTE_DIR}" data/
    echo "Backup: ${BACKUP_FILE}"
    # Keep last 10 backups
    ls -t "${REMOTE_DIR}/backups"/data_*.tar.gz 2>/dev/null | tail -n +11 | xargs rm -f 2>/dev/null || true
  fi
REMOTE

echo "Step 3: Download and install new sqld binary"
SQLD_URL=$(resolve_sqld_url)
$SSH bash -s -- "$REMOTE_DIR" "$SQLD_URL" << 'REMOTE'
  set -euo pipefail
  REMOTE_DIR="$1"; SQLD_URL="$2"
  TMP=$(mktemp -d)
  curl -fL "$SQLD_URL" -o "${TMP}/sqld.tar.xz"
  tar -xzf "${TMP}/sqld.tar.gz" -C "$TMP"
  SQLD_BIN=$(find "$TMP" -name "sqld" -type f | head -1)
  [ -f "${REMOTE_DIR}/sqld" ] && cp "${REMOTE_DIR}/sqld" "${REMOTE_DIR}/sqld.prev"
  install -m 755 "$SQLD_BIN" "${REMOTE_DIR}/sqld"
  rm -rf "$TMP"
  echo "Installed: $(${REMOTE_DIR}/sqld --version 2>&1 | head -1)"
REMOTE

echo "Step 4: Restart service"
$SSH "systemctl start ${SERVICE_NAME}"
sleep 2
$SSH "systemctl status ${SERVICE_NAME} --no-pager -l"

echo ""
echo "==> Deploy complete."
