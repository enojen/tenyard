#!/usr/bin/env bash
# Writes the Secret `tenyard-autoscaler` of the cluster autoscaler: the pool config
# (cluster-config.json: the image and the worker cloud-init) and a bootstrap token that joins
# workers only. A second run keeps the token and replaces the config. Then it applies the
# Deployment (kustomization.yaml here) and restarts it, as the autoscaler reads the Secret at
# start. Needs KUBECONFIG, kubectl, jq and ssh to the server as root (the token is made with
# `k3s token create`).
#
# Usage: autoscaler.sh <server ssh host> <server ip on tenyard-net>
# How long a worker stays unneeded before it goes is `--scale-down-unneeded-time` in
# autoscaler.yaml.
set -euo pipefail
host="$1"
server_ip="$2"
dir="$(cd "$(dirname "$0")" && pwd)"
ns=kube-system

token="$(kubectl -n "$ns" get secret tenyard-autoscaler -o jsonpath='{.data.token}' 2> /dev/null | base64 -d || true)"
if [ -z "$token" ]; then
  token="$(ssh "root@$host" k3s token create --ttl 0 --description tenyard-autoscaler)"
fi

umask 077
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
sed -e "s|__SERVER_IP__|$server_ip|" -e "s|__TOKEN__|$token|" "$dir/worker-cloud-init.yaml" > "$tmp/cloud-init.yaml"
jq -n --rawfile init "$tmp/cloud-init.yaml" \
  '{ imagesForArch: { amd64: "ubuntu-24.04" },
     nodeConfigs: ([ "box-cx43", "box-cpx32", "box-cpx42" ] | map({ key: ., value: { cloudInit: $init } }) | from_entries) }' \
  > "$tmp/cluster-config.json"
printf %s "$token" > "$tmp/token"
kubectl -n "$ns" create secret generic tenyard-autoscaler \
  --from-file=cluster-config.json="$tmp/cluster-config.json" --from-file=token="$tmp/token" \
  --dry-run=client -o yaml | kubectl -n "$ns" apply -f -
kubectl apply -k "$dir"
kubectl -n "$ns" rollout restart deployment/cluster-autoscaler
kubectl -n "$ns" rollout status deployment/cluster-autoscaler --timeout=120s
