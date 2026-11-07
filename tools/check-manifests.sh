#!/usr/bin/env bash
# Checks the Kubernetes manifests: the build of clusters/tenyard (every object of the cluster
# that lives in git) and the templates of a task Job in task/. kubeconform checks them against
# the API schemas of the cluster's version (strict: an unknown field fails), then kube-linter
# with its default checks. An object that breaks a kube-linter check on purpose says why in an
# `ignore-check.kube-linter.io/<check>` annotation. Run tools/install.sh first.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
bin="$root/.tools/bin"
cd "$root"

# The Kubernetes version of the cluster (install/README.md, "Versions").
kubernetes_version=1.36.5

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
"$bin/kustomize" build clusters/tenyard > "$tmp/cluster.yaml"
# The services of a task are strategic merge patches of the Job, not objects of their own.
templates=(task/job.yaml task/pvc.yaml)

"$bin/kubeconform" -kubernetes-version "$kubernetes_version" -strict -summary "$tmp/cluster.yaml" "${templates[@]}"
"$bin/kube-linter" lint "$tmp/cluster.yaml" "${templates[@]}"
