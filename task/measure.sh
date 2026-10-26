#!/usr/bin/env bash
# Prints the start timeline of one task Job: each stage as seconds after the Job was created,
# from the pod events (kept about an hour) and the box log. While the pod runs, also the CPU
# throttle ratio of its cgroup. Needs KUBECONFIG, kubectl and jq.
#
#   measure.sh                   the newest Job of the namespace
#   measure.sh task-demo-1-1026-204413
#
# Env: NAMESPACE (tenbox).
set -euo pipefail
ns="${NAMESPACE:-tenbox}"
job="${1:-$(kubectl -n "$ns" get jobs --sort-by=.metadata.creationTimestamp -o jsonpath='{.items[-1].metadata.name}')}"
[ -n "$job" ] || { echo "no Job in $ns" >&2; exit 1; }
created="$(kubectl -n "$ns" get "job/$job" -o jsonpath='{.metadata.creationTimestamp}')"
pod="$(kubectl -n "$ns" get pods -l "job-name=$job" -o jsonpath='{.items[0].metadata.name}')"
t0="$(date -d "$created" +%s)"
after() { echo "+$(( $(date -d "$1" +%s) - t0 )) s"; }

echo "job $job created $created, pod ${pod:-?}"
# An event carries eventTime or lastTimestamp; the pull event says how long the pull took.
kubectl -n "$ns" get events --field-selector "involvedObject.name=$pod" -o json \
  | jq -r '.items[] | select(.reason | IN("Scheduled","SuccessfulAttachVolume","Pulling","Pulled","Started"))
           | "\(.eventTime // .lastTimestamp) \(.reason) \(.message)"' \
  | sort | while read -r at reason message; do
      printf '  %-10s %-24s %s\n' "$(after "$at")" "$reason" "$(printf '%s' "$message" | grep -o 'in [0-9.]*[a-z]*s' | head -1)"
    done

# The box log with kubelet timestamps: the lines boot.sh writes around the command.
kubectl -n "$ns" logs "job/$job" --timestamps 2> /dev/null \
  | grep -E 'tenbox-boot: (ready at|command ended|no session|sessions idle|memory peak)' \
  | while read -r at line; do printf '  %-10s %s\n' "$(after "$at")" "${line#tenbox-boot: }"; done

phase="$(kubectl -n "$ns" get "pod/$pod" -o jsonpath='{.status.phase}')"
if [ "$phase" = Running ]; then
  (kubectl -n "$ns" exec "$pod" -- cat /sys/fs/cgroup/cpu.stat 2> /dev/null || true) \
    | awk '/^nr_periods/ {p=$2} /^nr_throttled/ {t=$2} END { if (p > 0) printf "  cpu throttled: %d of %d periods (%.1f%%)\n", t, p, 100*t/p }'
fi
