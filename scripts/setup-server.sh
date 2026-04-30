#!/usr/bin/env bash
# Bamako server setup — generates auth tokens, creates config, starts services.
# Run once on your server after cloning this repo.
#
# Usage:
#   ./scripts/setup-server.sh --domain docs.example.com
#   ./scripts/setup-server.sh --domain docs.example.com --email admin@example.com
#
# Requirements: docker, docker compose, openssl, python3 OR node

set -euo pipefail

DOMAIN=""
EMAIL="admin@example.com"
SPACES=("shared")  # default namespaces to create

for i in "$@"; do
  case $i in
    --domain=*) DOMAIN="${i#*=}" ;;
    --domain)   DOMAIN="$2"; shift ;;
    --email=*)  EMAIL="${i#*=}" ;;
    --email)    EMAIL="$2"; shift ;;
  esac
done

if [ -z "$DOMAIN" ]; then
  echo "Usage: $0 --domain your.domain.com [--email admin@example.com]"
  exit 1
fi

mkdir -p config

# ── Generate JWT signing key ────────────────────────────────────────────────
if [ ! -f config/jwt-key ]; then
  openssl rand -hex 32 > config/jwt-key
  echo "Generated JWT signing key."
fi
JWT_SECRET=$(cat config/jwt-key)

# ── Generate admin JWT token (long-lived) ──────────────────────────────────
# HS256 JWT: header.payload.signature — pure bash/openssl, no external JWT libs
b64url() { openssl base64 -A | tr '+/' '-_' | tr -d '='; }

HEADER=$(echo -n '{"alg":"HS256","typ":"JWT"}' | b64url)
# exp: year 2099
PAYLOAD=$(echo -n '{"sub":"admin","iat":1700000000,"exp":4070908800}' | b64url)
SIG=$(echo -n "${HEADER}.${PAYLOAD}" \
  | openssl dgst -sha256 -hmac "$JWT_SECRET" -binary \
  | b64url)
ADMIN_TOKEN="${HEADER}.${PAYLOAD}.${SIG}"

# ── Write Caddyfile ─────────────────────────────────────────────────────────
cat > config/Caddyfile << EOF
${DOMAIN} {
    tls ${EMAIL}
    reverse_proxy sqld:8080
}
EOF
echo "Wrote config/Caddyfile for ${DOMAIN}."

# ── Save connection info ────────────────────────────────────────────────────
cat > config/connection-info.txt << EOF
Bamako Server Connection Info
Generated: $(date)

Server URL:  https://${DOMAIN}
Admin Token: ${ADMIN_TOKEN}

Invite Links (base64 — paste into Bamako Settings → Connect Server → Paste Invite):
EOF

for SPACE in "${SPACES[@]}"; do
  INVITE=$(echo -n "{\"v\":1,\"serverUrl\":\"https://${DOMAIN}\",\"name\":\"${SPACE}\",\"token\":\"${ADMIN_TOKEN}\",\"permissionLevel\":\"write\"}" | openssl base64 -A | tr '+/' '-_' | tr -d '=')
  echo "  ${SPACE}: ${INVITE}" >> config/connection-info.txt
done

echo "" >> config/connection-info.txt
echo "Keep this file secure — the admin token grants full access." >> config/connection-info.txt

# ── Start services ──────────────────────────────────────────────────────────
echo ""
echo "Starting services..."
docker compose -f docker-compose.server.yml up -d

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Bamako server running at https://${DOMAIN}"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
cat config/connection-info.txt
echo ""
echo "Full details saved to: config/connection-info.txt"
