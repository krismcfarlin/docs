#!/usr/bin/env bash
# Generate a Bamako invite link for an existing server.
# Reads /opt/bamako/jwt-key from the remote server (or local path via --key-file).
#
# Usage:
#   ./scripts/generate-invite.sh --server 159.223.0.1 --domain docs.example.com
#   ./scripts/generate-invite.sh --server 159.223.0.1 --domain docs.example.com --space shared --permission write
#   ./scripts/generate-invite.sh --key-file /path/to/jwt-key --domain docs.example.com

set -euo pipefail

SPACE="shared"
PERMISSION="write"
DOMAIN=""
DEPLOY_SERVER="${DEPLOY_SERVER:-}"
DEPLOY_USER="${DEPLOY_USER:-root}"
KEY_FILE=""
REMOTE_KEY="/opt/bamako/jwt-key"

while [[ $# -gt 0 ]]; do
  case $1 in
    --space=*)      SPACE="${1#*=}" ;;
    --space)        SPACE="$2"; shift ;;
    --permission=*) PERMISSION="${1#*=}" ;;
    --permission)   PERMISSION="$2"; shift ;;
    --domain=*)     DOMAIN="${1#*=}" ;;
    --domain)       DOMAIN="$2"; shift ;;
    --server=*)     DEPLOY_SERVER="${1#*=}" ;;
    --server)       DEPLOY_SERVER="$2"; shift ;;
    --key-file=*)   KEY_FILE="${1#*=}" ;;
    --key-file)     KEY_FILE="$2"; shift ;;
  esac
  shift
done

if [ -n "$KEY_FILE" ]; then
  if [ ! -f "$KEY_FILE" ]; then
    echo "Key file not found: $KEY_FILE"
    exit 1
  fi
  JWT_SECRET=$(cat "$KEY_FILE")
elif [ -n "$DEPLOY_SERVER" ]; then
  JWT_SECRET=$(ssh "${DEPLOY_USER}@${DEPLOY_SERVER}" "cat ${REMOTE_KEY}")
else
  echo "Pass --server <ip> or --key-file <path>"
  echo "  Example: DEPLOY_SERVER=159.223.0.1 ./scripts/generate-invite.sh --domain docs.example.com"
  exit 1
fi

if [ -z "$DOMAIN" ]; then
  echo "Could not detect domain. Pass --domain your.domain.com"
  exit 1
fi

# JWT key is stored as base64url on server; sqld decodes to raw bytes for HMAC.
# Use Python for signing to ensure binary key handling works on macOS + Linux.
TOKEN=$(python3 - "$JWT_SECRET" << 'PYEOF'
import sys, hmac, hashlib, base64, json

secret_b64url = sys.argv[1]
pad = 4 - len(secret_b64url) % 4
if pad == 4: pad = 0
key = base64.b64decode(secret_b64url.replace('-', '+').replace('_', '/') + '=' * pad)

def b64url(data):
    if isinstance(data, str): data = data.encode()
    return base64.urlsafe_b64encode(data).rstrip(b'=').decode()

header  = b64url(b'{"alg":"HS256","typ":"JWT"}')
payload = b64url(b'{"sub":"admin","iat":1700000000,"exp":4070908800}')
sig     = b64url(hmac.new(key, f"{header}.{payload}".encode(), hashlib.sha256).digest())
print(f"{header}.{payload}.{sig}")
PYEOF
)

# Envelope uses standard base64 (not base64url) so browser atob() can decode it
INVITE=$(echo -n "{\"v\":1,\"serverUrl\":\"https://${DOMAIN}\",\"name\":\"${SPACE}\",\"token\":\"${TOKEN}\",\"permissionLevel\":\"${PERMISSION}\"}" \
  | openssl base64 -A)

echo ""
echo "Invite link for space '${SPACE}' (${PERMISSION}):"
echo ""
echo "  ${INVITE}"
echo ""
echo "Share this with your team. They paste it into:"
echo "  Bamako Settings → Connect Server → Paste Invite"
