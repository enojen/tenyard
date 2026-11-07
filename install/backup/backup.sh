#!/bin/sh
# The daily copy off the host (install/README.md, "Backups"), run by the CronJob
# offsite-backup on the control-plane server. Two files go to the bucket, each encrypted with
# age to the key of Tenyard, as both hold every Secret of the cluster in plain text:
#   k3s/k3s-<day>.tar.gz.age        the k3s datastore (an online SQLite backup) and the server
#                                   token, which decrypts the bootstrap data in it
#   server/server-<day>.db.gz.age   the newest daily copy of the tenbox server's database
# The bucket deletes a file after 30 days. On a failure it sends one ntfy notice.
set -eu

apk add --no-cache -q sqlite age curl
day="$(date -u +%Y-%m-%d)"
work="$(mktemp -d)"
step="start"

notify() {
  status=$?
  rm -rf "$work"
  if [ "$status" != 0 ]; then
    curl -sf -d "tenyard · backup · failed · $step → kubectl -n tenbox-server logs job/<name>" \
      "$(cat /ntfy/url)" > /dev/null || true
  fi
  exit "$status"
}
trap notify EXIT

umask 077
printf 'user = "%s:%s"\n' "$(cat /bucket/access_key)" "$(cat /bucket/secret_key)" > "$work/curl.conf"
upload() {
  curl -sSf -K "$work/curl.conf" --aws-sigv4 "aws:amz:$REGION:s3" -T "$1" \
    "https://$ENDPOINT/$BUCKET/$2"
  echo "uploaded $2 ($(wc -c < "$1") bytes)"
}

step="k3s datastore"
mkdir "$work/k3s"
sqlite3 /k3s/db/state.db ".backup '$work/k3s/state.db'"
check="$(sqlite3 "$work/k3s/state.db" 'PRAGMA integrity_check')"
[ "$check" = ok ] || { echo "integrity check of the copy: $check" >&2; exit 1; }
cp /k3s/token "$work/k3s/token"
tar -czf "$work/k3s.tar.gz" -C "$work/k3s" state.db token
age -r "$AGE_RECIPIENT" -o "$work/k3s.tar.gz.age" "$work/k3s.tar.gz"
upload "$work/k3s.tar.gz.age" "k3s/k3s-$day.tar.gz.age"

step="server database"
# The CronJob backup writes it at 03:00; a missing one means that job failed.
copy="/server/backups/server-$day.db"
[ -s "$copy" ] || { echo "no $copy: the backup CronJob did not run today" >&2; exit 1; }
gzip -c "$copy" > "$work/server.db.gz"
age -r "$AGE_RECIPIENT" -o "$work/server.db.gz.age" "$work/server.db.gz"
upload "$work/server.db.gz.age" "server/server-$day.db.gz.age"
