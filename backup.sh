#!/usr/bin/env bash
# Bamako sqld — backup data from shared server
#
# Usage:
#   ./backup.sh                        # create remote backup, pull to local
#   ./backup.sh --remote-only          # create backup on server only (no pull)
#   ./backup.sh --list                 # list backups on server
#
# Requires: DEPLOY_SERVER env var (or --server), SSH key access

set -euo pipefail

DEPLOY_SERVER="${DEPLOY_SERVER:-}"
DEPLOY_USER="${DEPLOY_USER:-root}"
REMOTE_DIR="/opt/bamako"
SERVICE_NAME="bamako-sqld"
LOCAL_BACKUP_DIR="$HOME/Documents/bamako-backups"
REMOTE_ONLY=false
LIST=false

for i in "$@"; do
  case $i in
    --remote-only)  REMOTE_ONLY=true ;;
    --list)         LIST=true ;;
    --server=*)     DEPLOY_SERVER="${i#*=}" ;;
    --server)       DEPLOY_SERVER="$2"; shift ;;
  esac
done

if [ -z "$DEPLOY_SERVER" ]; then
  echo "Set DEPLOY_SERVER env var or pass --server <ip>"
  echo "  Example: DEPLOY_SERVER=159.223.0.1 ./backup.sh"
  exit 1
fi

SSH="ssh -o ControlMaster=auto -o ControlPath=/tmp/bamako-deploy-%r@%h:%p -o ControlPersist=60 ${DEPLOY_USER}@${DEPLOY_SERVER}"

# ── List backups ──────────────────────────────────────────────────────────────
if $LIST; then
  echo "Backups on ${DEPLOY_SERVER}:${REMOTE_DIR}/backups/"
  $SSH "ls -lht ${REMOTE_DIR}/backups/data_*.tar.gz 2>/dev/null || echo '  (none)'"
  exit 0
fi

# ── Create backup on server ───────────────────────────────────────────────────
echo "==> Creating backup on ${DEPLOY_SERVER}..."
BACKUP_FILE=$($SSH bash -s -- "$REMOTE_DIR" "$SERVICE_NAME" << 'REMOTE'
  set -euo pipefail
  REMOTE_DIR="$1"; SERVICE_NAME="$2"
  TIMESTAMP=$(date +%Y%m%d_%H%M%S)
  BACKUP_FILE="${REMOTE_DIR}/backups/data_${TIMESTAMP}.tar.gz"
  mkdir -p "${REMOTE_DIR}/backups"
  # Pause writes with a brief service stop for a clean snapshot
  systemctl stop "${SERVICE_NAME}" 2>/dev/null || true
  tar -czf "$BACKUP_FILE" -C "${REMOTE_DIR}" data/
  systemctl start "${SERVICE_NAME}" 2>/dev/null || true
  # Keep last 20 backups on server
  ls -t "${REMOTE_DIR}/backups"/data_*.tar.gz 2>/dev/null | tail -n +21 | xargs rm -f 2>/dev/null || true
  echo "$BACKUP_FILE"
REMOTE
)

echo "    Remote backup: ${BACKUP_FILE}"

if $REMOTE_ONLY; then
  echo "==> Done (remote-only)."
  exit 0
fi

# ── Pull backup to local ──────────────────────────────────────────────────────
mkdir -p "$LOCAL_BACKUP_DIR"
FILENAME=$(basename "$BACKUP_FILE")
LOCAL_FILE="${LOCAL_BACKUP_DIR}/${FILENAME}"

echo "==> Pulling backup to ${LOCAL_FILE}..."
scp "${DEPLOY_USER}@${DEPLOY_SERVER}:${BACKUP_FILE}" "$LOCAL_FILE"

SIZE=$(du -sh "$LOCAL_FILE" | cut -f1)
echo "    Downloaded: ${LOCAL_FILE} (${SIZE})"

# Keep last 10 local backups
BACKUP_COUNT=$(ls -1 "${LOCAL_BACKUP_DIR}"/data_*.tar.gz 2>/dev/null | wc -l | tr -d ' ')
if [ "$BACKUP_COUNT" -gt 10 ]; then
  echo "==> Pruning old local backups (keeping 10 most recent)..."
  ls -1t "${LOCAL_BACKUP_DIR}"/data_*.tar.gz | tail -n +11 | xargs rm -f
fi

echo ""
echo "==> Backup complete."
echo "    Local:  ${LOCAL_FILE}"
echo "    Remote: ${BACKUP_FILE}"
