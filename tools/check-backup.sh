#!/usr/bin/env bash
# Downloads the backups of one day (default today, UTC) from the bucket and checks that they
# open; it fails when an integrity check of SQLite does. It decrypts them in RAM, in a container
# with no network and a read-only disk, and prints only checks and counts, never a value: the k3s
# datastore holds every Secret of the cluster.
# Needs docker and the age key of Tenyard (SOPS_AGE_KEY_FILE, default ~/.config/tenyard/age.key);
# the bucket keys come from secrets/. Run tools/install.sh first.
#
# Usage: tools/check-backup.sh [YYYY-MM-DD]
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
bin="$root/.tools/bin"
export SOPS_AGE_KEY_FILE="${SOPS_AGE_KEY_FILE:-$HOME/.config/tenyard/age.key}"
day="${1:-$(date -u +%Y-%m-%d)}"
# The image and the bucket of install/backup/backup.yaml.
image=alpine:3.22@sha256:bfe7ceb9f09ec0566c48fb93356e45a47a00be5a1dd14fa5b909e7709c66b034
bucket=https://hel1.your-objectstorage.com/tenyard-backups

umask 077
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
key() {
  "$bin/sops" --decrypt --extract "[\"data\"][\"$1\"]" \
    "$root/secrets/tenbox-server/backup-bucket.sops.yaml" | base64 -d
}
printf 'user = "%s:%s"\n' "$(key access_key)" "$(key secret_key)" > "$tmp/curl.conf"
for object in "k3s/k3s-$day.tar.gz.age" "server/server-$day.db.gz.age"; do
  curl -sSf -K "$tmp/curl.conf" --aws-sigv4 "aws:amz:hel1:s3" -o "$tmp/${object#*/}" "$bucket/$object"
  echo "$object: $(wc -c < "$tmp/${object#*/}") bytes"
done
rm "$tmp/curl.conf"

printf 'FROM %s\nRUN apk add --no-cache sqlite age\n' "$image" \
  | docker build -q -t tenyard-backup-check - > /dev/null
docker run --rm -i --network none --read-only --tmpfs /work -e DAY="$day" \
  -v "$tmp:/backups:ro" -v "$SOPS_AGE_KEY_FILE:/key:ro" tenyard-backup-check sh -eu -s <<'EOF'
cd /work
age -d -i /key "/backups/k3s-$DAY.tar.gz.age" | tar -xz
echo "k3s token: $(wc -c < token) bytes"
check="$(sqlite3 state.db 'PRAGMA integrity_check')"; echo "k3s integrity: $check"; [ "$check" = ok ]
echo "k3s rows: $(sqlite3 state.db 'SELECT count(*) FROM kine')"
echo "k3s secrets: $(sqlite3 state.db "SELECT count(DISTINCT name) FROM kine WHERE name LIKE '/registry/secrets/%'")"
age -d -i /key "/backups/server-$DAY.db.gz.age" | gunzip > server.db
check="$(sqlite3 server.db 'PRAGMA integrity_check')"; echo "server integrity: $check"; [ "$check" = ok ]
echo "server tables: $(sqlite3 server.db "SELECT group_concat(name, ' ') FROM sqlite_master WHERE type = 'table'")"
EOF
