#!/usr/bin/env bash
# Dumps the self-hosted Postgres database, uploads it to a private R2
# bucket (separate from the public bikepackid-media bucket), and prunes
# backups older than $RETENTION_DAYS. Meant to run daily via cron on the
# EC2 instance — see backend/DEPLOY_AWS.md "Backup database" for setup.
#
# Only relevant for the self-hosted Postgres deploy (docker-compose.postgres.yml).
# Not needed if DATABASE_URL still points at a managed provider (Supabase/RDS/etc)
# — those handle backups themselves.
set -euo pipefail

cd "$(dirname "$0")/.."

: "${R2_BACKUP_BUCKET:?Set R2_BACKUP_BUCKET (e.g. bikepackid-backups)}"
: "${R2_BACKUP_ENDPOINT:?Set R2_BACKUP_ENDPOINT (e.g. https://<account_id>.r2.cloudflarestorage.com)}"
: "${AWS_ACCESS_KEY_ID:?Set AWS_ACCESS_KEY_ID to the R2 token's access key}"
: "${AWS_SECRET_ACCESS_KEY:?Set AWS_SECRET_ACCESS_KEY to the R2 token's secret key}"

RETENTION_DAYS="${RETENTION_DAYS:-30}"
TIMESTAMP=$(date -u +%Y%m%d-%H%M%S)
BACKUP_FILE="/tmp/bikepackid-${TIMESTAMP}.sql.gz"

cleanup() {
  rm -f "$BACKUP_FILE"
}
trap cleanup EXIT

echo "[$(date -u +%FT%TZ)] Dumping database..."
docker compose -f docker-compose.yml -f docker-compose.postgres.yml exec -T postgres \
  pg_dump -U bikepackid -d bikepackid | gzip > "$BACKUP_FILE"

echo "[$(date -u +%FT%TZ)] Uploading to R2..."
aws s3 cp "$BACKUP_FILE" "s3://${R2_BACKUP_BUCKET}/${TIMESTAMP}.sql.gz" \
  --endpoint-url "$R2_BACKUP_ENDPOINT" --region auto

echo "[$(date -u +%FT%TZ)] Pruning backups older than ${RETENTION_DAYS} days..."
CUTOFF=$(date -u -d "-${RETENTION_DAYS} days" +%s)
aws s3api list-objects-v2 --bucket "$R2_BACKUP_BUCKET" --endpoint-url "$R2_BACKUP_ENDPOINT" --region auto \
  --query 'Contents[].Key' --output text | tr '\t' '\n' | while read -r key; do
  [ -z "$key" ] && continue
  key_date=$(echo "$key" | grep -oE '^[0-9]{8}' || true)
  [ -z "$key_date" ] && continue
  key_epoch=$(date -u -d "$key_date" +%s)
  if [ "$key_epoch" -lt "$CUTOFF" ]; then
    echo "  deleting $key"
    aws s3 rm "s3://${R2_BACKUP_BUCKET}/${key}" --endpoint-url "$R2_BACKUP_ENDPOINT" --region auto
  fi
done

echo "[$(date -u +%FT%TZ)] Backup done: ${TIMESTAMP}.sql.gz"

# Optional dead-man's-switch ping — alerts (email/etc, configured on
# healthchecks.io) if this script doesn't run/complete within the expected
# window. No-op if HEALTHCHECK_PING_URL isn't set.
if [ -n "${HEALTHCHECK_PING_URL:-}" ]; then
  curl -fsS -m 10 --retry 3 "$HEALTHCHECK_PING_URL" >/dev/null || true
fi
