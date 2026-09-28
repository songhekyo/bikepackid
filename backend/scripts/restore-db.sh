#!/usr/bin/env bash
# Restores a backup produced by backup-db.sh. Use this to actually TEST a
# backup periodically (restore to a throwaway database, not production) —
# an untested backup is an assumption, not a guarantee.
#
# Usage: ./restore-db.sh <backup-key> [target-db-name]
#   e.g. ./restore-db.sh 20261028-030000.sql.gz bikepackid_restore_test
set -euo pipefail

cd "$(dirname "$0")/.."

BACKUP_KEY="${1:?Usage: restore-db.sh <backup-key> [target-db-name]}"
TARGET_DB="${2:-bikepackid_restore_test}"

: "${R2_BACKUP_BUCKET:?Set R2_BACKUP_BUCKET}"
: "${R2_BACKUP_ENDPOINT:?Set R2_BACKUP_ENDPOINT}"
: "${AWS_ACCESS_KEY_ID:?Set AWS_ACCESS_KEY_ID}"
: "${AWS_SECRET_ACCESS_KEY:?Set AWS_SECRET_ACCESS_KEY}"

TMP_FILE="/tmp/restore-${BACKUP_KEY}"
cleanup() { rm -f "$TMP_FILE" "${TMP_FILE%.gz}"; }
trap cleanup EXIT

echo "Downloading ${BACKUP_KEY} from R2..."
aws s3 cp "s3://${R2_BACKUP_BUCKET}/${BACKUP_KEY}" "$TMP_FILE" \
  --endpoint-url "$R2_BACKUP_ENDPOINT" --region auto
gunzip "$TMP_FILE"

echo "Creating throwaway database '${TARGET_DB}'..."
docker compose -f docker-compose.yml -f docker-compose.postgres.yml exec postgres \
  psql -U bikepackid -d postgres -c "DROP DATABASE IF EXISTS ${TARGET_DB};"
docker compose -f docker-compose.yml -f docker-compose.postgres.yml exec postgres \
  psql -U bikepackid -d postgres -c "CREATE DATABASE ${TARGET_DB} OWNER bikepackid;"

echo "Restoring into '${TARGET_DB}'..."
docker compose -f docker-compose.yml -f docker-compose.postgres.yml exec -T postgres \
  psql -U bikepackid -d "$TARGET_DB" < "${TMP_FILE%.gz}"

echo "Row counts in restored '${TARGET_DB}':"
docker compose -f docker-compose.yml -f docker-compose.postgres.yml exec postgres \
  psql -U bikepackid -d "$TARGET_DB" -c \
  "SELECT 'users' AS t, count(*) FROM users UNION ALL SELECT 'journeys', count(*) FROM journeys;"

echo
echo "Compare these counts against the live 'bikepackid' database. When done inspecting, drop the throwaway DB:"
echo "  docker compose -f docker-compose.yml -f docker-compose.postgres.yml exec postgres psql -U bikepackid -d postgres -c \"DROP DATABASE ${TARGET_DB};\""
