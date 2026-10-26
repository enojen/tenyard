#!/usr/bin/env bash
# Runs one tenbox command of one task on the cluster: applies the task's PVC (kept when it
# exists) and one Job from the templates next to this script, then follows the log until the
# pod ends. Needs KUBECONFIG and kubectl. Exits with 0 when the Job completed.
#
#   run.sh TEN-451 "tenbox start TEN-451"
#   PROJECT=demo REPO=https://github.com/me/demo.git run.sh DEMO-1 "tenbox build DEMO-1"
#
# Env: PROJECT (tenbox), PROJECT_ID (xathah, the id in its tenbox.yaml), REPO
# (https://github.com/enojen/tenbox.git), IMAGE (ghcr.io/enojen/tenbox-box:latest), NAMESPACE
# (tenbox). run.sh does not write the project Secret: a Job before it from `tenbox machine run`
# did, or the box runs without a tracker key and ntfy URL.
set -euo pipefail
# bash 5.2 reads `&` in a `${x//a/b}` replacement as the match; a command may hold one.
shopt -u patsub_replacement 2> /dev/null || true
task="${1:?task id}"
cmd="${2:?tenbox command}"
project="${PROJECT:-tenbox}"
project_id="${PROJECT_ID:-xathah}"
repo="${REPO:-https://github.com/enojen/tenbox.git}"
image="${IMAGE:-ghcr.io/enojen/tenbox-box:latest}"
ns="${NAMESPACE:-tenbox}"
here="$(cd "$(dirname "$0")" && pwd)"
name="$(printf '%s' "$task" | tr '[:upper:]_' '[:lower:]-')"
run="$(date -u +%m%d-%H%M%S)"
job="task-$name-$run"

fill() {
  local text; text="$(< "$here/$1")"
  text="${text//__NAME__/$name}"; text="${text//__TASK__/$task}"; text="${text//__RUN__/$run}"
  text="${text//__PROJECT__/$project}"; text="${text//__PROJECT_ID__/$project_id}"
  text="${text//__REPO__/$repo}"; text="${text//__IMAGE__/$image}"
  printf '%s\n' "${text//__CMD__/$cmd}"
}

active="$(kubectl -n "$ns" get jobs -l "task=$task" -o jsonpath='{range .items[?(@.status.active>0)]}{.metadata.name}{"\n"}{end}')"
if [ -n "$active" ]; then
  echo "task $task still runs: $active" >&2
  exit 1
fi
fill pvc.yaml | kubectl -n "$ns" apply -f -
fill job.yaml | kubectl -n "$ns" apply -f -

echo "job $job: waiting for the pod" >&2
kubectl -n "$ns" wait pod -l "job-name=$job" --for=jsonpath='{.status.phase}'=Running --timeout=600s > /dev/null \
  || kubectl -n "$ns" get pod -l "job-name=$job" >&2
kubectl -n "$ns" logs -f "job/$job" || true
kubectl -n "$ns" wait "job/$job" --for=condition=complete --timeout=10s > /dev/null 2>&1 \
  && { echo "job $job: completed" >&2; exit 0; }
echo "job $job: failed" >&2
kubectl -n "$ns" get "job/$job" -o jsonpath='{.status.conditions[*].message}' >&2; echo >&2
exit 1
