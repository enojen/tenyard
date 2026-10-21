#!/usr/bin/env bash
# Runs one pod that writes to a new volume, reaches the internet and prints its cores,
# then removes the pod and the volume. Needs KUBECONFIG and kubectl.
set -euo pipefail

ns="${1:-tenbox}"

kubectl -n "$ns" apply -f - <<'EOF'
apiVersion: v1
kind: PersistentVolumeClaim
metadata: { name: tenyard-check }
spec: { accessModes: [ReadWriteOnce], resources: { requests: { storage: 10Gi } } }
---
apiVersion: v1
kind: Pod
metadata: { name: tenyard-check }
spec:
  restartPolicy: Never
  containers:
  - name: check
    image: alpine:3.20
    command: ["sh", "-c", "echo hello > /data/hello && cat /data/hello && wget -qO- https://api.github.com/zen && echo && nproc && cat /sys/fs/cgroup/cpu.max"]
    volumeMounts: [{ name: data, mountPath: /data }]
  volumes: [{ name: data, persistentVolumeClaim: { claimName: tenyard-check } }]
EOF

phase=""
for _ in $(seq 1 60); do
  phase="$(kubectl -n "$ns" get pod tenyard-check -o jsonpath='{.status.phase}')"
  if [ "$phase" = Succeeded ] || [ "$phase" = Failed ]; then break; fi
  sleep 3
done
echo "pod: $phase"
kubectl -n "$ns" logs tenyard-check
kubectl -n "$ns" delete pod/tenyard-check pvc/tenyard-check --wait=true
[ "$phase" = Succeeded ]
