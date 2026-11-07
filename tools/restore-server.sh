#!/usr/bin/env bash
# Puts the tenbox server's database of one day's backup (install/README.md, "Backups") on the
# server's volume of the cluster of KUBECONFIG, in place of the one there. It stops the server,
# writes the file with a helper pod on the volume, and starts the server again. The backup is
# decrypted in a container with no network and goes to the pod through a pipe, never to a disk
# here. Needs docker and the age key of Tenyard (SOPS_AGE_KEY_FILE, default
# ~/.config/tenyard/age.key); the bucket keys come from secrets/. Run tools/install.sh first.
#
# Usage: tools/restore-server.sh YYYY-MM-DD
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
bin="$root/.tools/bin"
export SOPS_AGE_KEY_FILE="${SOPS_AGE_KEY_FILE:-$HOME/.config/tenyard/age.key}"
day="$1"
ns=tenbox-server
# The image and the bucket of install/backup/backup.yaml.
image=alpine:3.22@sha256:bfe7ceb9f09ec0566c48fb93356e45a47a00be5a1dd14fa5b909e7709c66b034
bucket=https://hel1.your-objectstorage.com/tenyard-backups

umask 077
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"; kubectl -n "$ns" delete pod restore-server --ignore-not-found --wait=false > /dev/null' EXIT
key() {
  "$bin/sops" --decrypt --extract "[\"data\"][\"$1\"]" \
    "$root/secrets/tenbox-server/backup-bucket.sops.yaml" | base64 -d
}
printf 'user = "%s:%s"\n' "$(key access_key)" "$(key secret_key)" > "$tmp/curl.conf"
curl -sSf -K "$tmp/curl.conf" --aws-sigv4 "aws:amz:hel1:s3" -o "$tmp/server.db.gz.age" \
  "$bucket/server/server-$day.db.gz.age"
rm "$tmp/curl.conf"
printf 'FROM %s\nRUN apk add --no-cache sqlite age\n' "$image" \
  | docker build -q -t tenyard-backup-check - > /dev/null

kubectl -n "$ns" scale deployment/tenbox-server --replicas=0
kubectl -n "$ns" wait --for=delete pod -l app=tenbox-server --timeout=120s
kubectl apply -f - <<EOF
apiVersion: v1
kind: Pod
metadata: { name: restore-server, namespace: $ns }
spec:
  restartPolicy: Never
  nodeSelector: { node-role.kubernetes.io/control-plane: "true" }
  tolerations: [{ key: node-role.kubernetes.io/control-plane, operator: Exists, effect: NoSchedule }]
  securityContext: { fsGroup: 1000 }
  containers:
    - name: restore
      image: $image
      command: [sleep, "600"]
      securityContext: { runAsUser: 1000 }
      volumeMounts: [{ name: data, mountPath: /data }]
  volumes: [{ name: data, persistentVolumeClaim: { claimName: tenbox-server } }]
EOF
kubectl -n "$ns" wait --for=condition=Ready pod/restore-server --timeout=120s

docker run --rm --network none -v "$tmp:/backups:ro" -v "$SOPS_AGE_KEY_FILE:/key:ro" \
  tenyard-backup-check sh -c 'age -d -i /key /backups/server.db.gz.age | gunzip' \
  | kubectl -n "$ns" exec -i restore-server -- sh -c \
    'cat > /data/server.db.restore && rm -f /data/server.db-wal /data/server.db-shm && mv /data/server.db.restore /data/server.db'
kubectl -n "$ns" exec restore-server -- ls -l /data/server.db

kubectl -n "$ns" delete pod restore-server --wait=true > /dev/null
kubectl -n "$ns" scale deployment/tenbox-server --replicas=1
kubectl -n "$ns" rollout status deployment/tenbox-server --timeout=120s
