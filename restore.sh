#!/usr/bin/env bash
# Bamako sqld — restore data to shared server
#
# Usage:
#   ./restore.sh                            # restore most recent local backup
#   ./restore.sh backup_20240101_120000     # restore specific local backup by name
#   ./restore.sh --from-remote              # restore most recent backup already on server
#   ./restore.sh --from-remote data_20240101_120000.tar.gz  # specific remote backup
#   ./restore.sh --list                     # list available local backups
#   ./restore.sh --list-remote              # list backups on server
#
# Requires: DEPLOY_SERVER env var (or --server), SSH key access

set -euo pipefail

DEPLOY_SERVER="${DEPLOY_SERVER:-}"
DEPLOY_USER="${DEPLOY_USER:-root}"
REMOTE_DIR="/opt/bamako"
SERVICE_NAME="bamako-sqld"
LOCAL_BACKUP_DIR="$HOME/Documents/bamako-backups"
FROM_REMOTE=false
LIST=false
LIST_REMOTE=false
BACKUP_NAME=""

for i in "$@"; do
  case $i in
    --from-remote)  FROM_REMOTE=true ;;
    --list)         LIST=true ;;
    --list-remote)  LIST_REMOTE=true ;;
    --server=*)     DEPLOY_SERVER="${i#*=}" ;;
    --server)       DEPLOY_SERVER="$2"; shift ;;
    --*)            echo "Unknown flag: $i" >&2; exit 1 ;;
    *)              BACKUP_NAME="$i" ;;
  esac
done

if [ -z "$DEPLOY_SERVER" ]; then
  echo "Set DEPLOY_SERVER env var or pass --server <ip>"
  echo "  Example: DEPLOY_SERVER=159.223.0.1 ./restore.sh"
  exit 1
fi

SSH="ssh -o ControlMaster=auto -o ControlPath=/tmp/bamako-deploy-%r@%h:%p -o ControlPersist=60 ${DEPLOY_USER}@${DEPLOY_SERVER}"

# ── List local backups ────────────────────────────────────────────────────────
if $LIST; then
  echo "Local backups in ${LOCAL_BACKUP_DIR}:"
  ls -lht "${LOCAL_BACKUP_DIR}"/data_*.tar.gz 2>/dev/null || echo "  (none)"
  exit 0
fi

# ── List remote backups ───────────────────────────────────────────────────────
if $LIST_REMOTE; then
  echo "Remote backups on ${DEPLOY_SERVER}:${REMOTE_DIR}/backups/"
  $SSH "ls -lht ${REMOTE_DIR}/backups/data_*.tar.gz 2>/dev/null || echo '  (none)'"
  exit 0
fi

# ── Confirm ───────────────────────────────────────────────────────────────────
echo "WARNING: This will stop the service and replace all data on ${DEPLOY_SERVER}."
read -rp "Continue? [y/N] " CONFIRM
[[ "$CONFIRM" =~ ^[Yy]$ ]] || { echo "Aborted."; exit 1; }

# ── Restore from remote backup ────────────────────────────────────────────────
if $FROM_REMOTE; then
  echo "==> Restoring from remote backup on ${DEPLOY_SERVER}..."
  $SSH bash -s -- "$REMOTE_DIR" "$SERVICE_NAME" "$BACKUP_NAME" << 'REMOTE'
    set -euo pipefail
    REMOTE_DIR="$1"; SERVICE_NAME="$2"; BACKUP_NAME="$3"

    if [ -n "$BACKUP_NAME" ]; then
      BACKUP_FILE="${REMOTE_DIR}/backups/${BACKUP_NAME}"
    else
      BACKUP_FILE=$(ls -t "${REMOTE_DIR}/backups"/data_*.tar.gz 2>/dev/null | head -n1 || true)
      if [ -z "$BACKUP_FILE" ]; then
        echo "ERROR: No backups found on server." >&2
        exit 1
      fi
    fi

    if [ ! -f "$BACKUP_FILE" ]; then
      echo "ERROR: Backup not found: ${BACKUP_FILE}" >&2
      exit 1
    fi

    echo "==> Restoring from: ${BACKUP_FILE}"
    systemctl stop "${SERVICE_NAME}" 2>/dev/null || true

    # Preserve a pre-restore snapshot
    TIMESTAMP=$(date +%Y%m%d_%H%M%S)
    PRE="${REMOTE_DIR}/backups/pre_restore_${TIMESTAMP}.tar.gz"
    [ -d "${REMOTE_DIR}/data" ] && tar -czf "$PRE" -C "${REMOTE_DIR}" data/ && echo "    Pre-restore snapshot: ${PRE}"

    rm -rf "${REMOTE_DIR}/data"
    mkdir -p "${REMOTE_DIR}/data"
    tar -xzf "$BACKUP_FILE" -C "${REMOTE_DIR}"
    chown -R bamako:bamako "${REMOTE_DIR}/data"

    systemctl start "${SERVICE_NAME}"
    echo "==> Restore complete."
REMOTE
  exit 0
fi

# ── Restore from local backup (upload then restore) ───────────────────────────
if [ -n "$BACKUP_NAME" ]; then
  if [[ "$BACKUP_NAME" == /* ]]; then
    LOCAL_FILE="$BACKUP_NAME"
  else
    # Allow bare name with or without extension
    if [[ "$BACKUP_NAME" == *.tar.gz ]]; then
      LOCAL_FILE="${LOCAL_BACKUP_DIR}/${BACKUP_NAME}"
    else
      LOCAL_FILE="${LOCAL_BACKUP_DIR}/${BACKUP_NAME}.tar.gz"
    fi
  fi
else
  LOCAL_FILE=$(ls -1t "${LOCAL_BACKUP_DIR}"/data_*.tar.gz 2>/dev/null | head -n1 || true)
  if [ -z "$LOCAL_FILE" ]; then
    echo "ERROR: No local backups found in ${LOCAL_BACKUP_DIR}" >&2
    exit 1
  fi
fi

if [ ! -f "$LOCAL_FILE" ]; then
  echo "ERROR: Backup not found: ${LOCAL_FILE}" >&2
  exit 1
fi

FILENAME=$(basename "$LOCAL_FILE")
echo "==> Uploading ${LOCAL_FILE} to server..."
scp "$LOCAL_FILE" "${DEPLOY_USER}@${DEPLOY_SERVER}:${REMOTE_DIR}/backups/${FILENAME}"

echo "==> Restoring on server..."
$SSH bash -s -- "$REMOTE_DIR" "$SERVICE_NAME" "$FILENAME" << 'REMOTE'
  set -euo pipefail
  REMOTE_DIR="$1"; SERVICE_NAME="$2"; FILENAME="$3"
  BACKUP_FILE="${REMOTE_DIR}/backups/${FILENAME}"

  systemctl stop "${SERVICE_NAME}" 2>/dev/null || true

  TIMESTAMP=$(date +%Y%m%d_%H%M%S)
  PRE="${REMOTE_DIR}/backups/pre_restore_${TIMESTAMP}.tar.gz"
  [ -d "${REMOTE_DIR}/data" ] && tar -czf "$PRE" -C "${REMOTE_DIR}" data/ && echo "    Pre-restore snapshot: ${PRE}"

  rm -rf "${REMOTE_DIR}/data"
  mkdir -p "${REMOTE_DIR}/data"
  tar -xzf "$BACKUP_FILE" -C "${REMOTE_DIR}"
  chown -R bamako:bamako "${REMOTE_DIR}/data"

  systemctl start "${SERVICE_NAME}"
  echo "==> Restore complete."
REMOTE
