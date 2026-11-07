#!/usr/bin/env bash
# Checks every Kubernetes manifest in git: kubeconform against the API schemas of the cluster's
# version (strict: an unknown field fails), then kube-linter with its default checks. An object
# that breaks a kube-linter check on purpose says why in an `ignore-check.kube-linter.io/<check>`
# annotation. Run tools/install.sh first.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
bin="$root/.tools/bin"
cd "$root"

# The Kubernetes version of the cluster (install/README.md, "Versions").
kubernetes_version=1.36.5

# Not manifests: the tenbox manifest, the worker cloud-init, kustomizations, strategic merge
# patches (the CSI controller, the services of a task) and the resource classes.
not_manifest='^tenbox\.yaml$|^install/autoscaler/worker-cloud-init\.yaml$|(^|/)kustomization\.yaml$|\.patch\.yaml$|^task/services/|^task/classes\.yaml$'

files=()
while IFS= read -r file; do files+=("$file"); done < <(git ls-files '*.yaml' | grep -Ev "$not_manifest")

"$bin/kubeconform" -kubernetes-version "$kubernetes_version" -strict -summary "${files[@]}"
"$bin/kube-linter" lint "${files[@]}"
