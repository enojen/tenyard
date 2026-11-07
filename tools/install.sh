#!/usr/bin/env bash
# Downloads the tools of the checks and of the Secrets into .tools/bin, each one pinned by version and sha256. A second run
# keeps a tool that is already there at its version. Linux and macOS, amd64 and arm64.
#
# A new version: change it below, download the four archives, put their sha256 here.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
bin="$root/.tools/bin"
mkdir -p "$bin"

kustomize_version=v5.8.3
sops_version=v3.13.3
kubeconform_version=v0.8.0
kube_linter_version=v0.8.3
shellcheck_version=v0.11.0

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)
    sops_asset=linux.amd64 sops_sum=e5bec3346a873ae91d871550f3e698c1aad962aff462a080e40f25fde17fef6b
    kustomize_asset=linux_amd64 kustomize_sum=cb9e31198d3f63b44848bd0afc4c8efc56e6cfef9c93a4a39a211940e6decd61
    kubeconform_asset=linux-amd64 kubeconform_sum=9bc2bffbf71f261128533edaf912153948b7ff238f9a531ae6d34466ec287883
    kube_linter_asset=linux kube_linter_sum=1a6d8419b11971372971fdbc22682b684ebfb7cf1c39591662d1b6ca736c41df
    shellcheck_asset=linux.x86_64 shellcheck_sum=b7af85e41cc99489dcc21d66c6d5f3685138f06d34651e6d34b42ec6d54fe6f6 ;;
  Linux-aarch64 | Linux-arm64)
    sops_asset=linux.arm64 sops_sum=53b0abacd38ef1b12a66d6c100956691b9cefce018d91f81e73ddf7438b94d77
    kustomize_asset=linux_arm64 kustomize_sum=9867b76482cfc6d25b546abc1d2b5a32bd137505aba640649b4b378f64f6d68f
    kubeconform_asset=linux-arm64 kubeconform_sum=1f53fc8e81258197a35e8603054162a5af1de8c5af13746c71ab680d9534ed87
    kube_linter_asset=linux_arm64 kube_linter_sum=802e1b09eabd08f6f0a060a6b8ab2bf7bc7e6bf4f673bb2692303704c84b3e22
    shellcheck_asset=linux.aarch64 shellcheck_sum=68a8133197a50beb8803f8d42f9908d1af1c5540d4bb05fdfca8c1fa47decefc ;;
  Darwin-x86_64)
    sops_asset=darwin.amd64 sops_sum=42162d5cef10b74fcf80a045a70e658d7ce6e63d6ea1be6f347e44015714468d
    kustomize_asset=darwin_amd64 kustomize_sum=d08fb85846c3d977f0e2a66e7ff45f1b5264c294685e5e726e1a8bdfe0da8dee
    kubeconform_asset=darwin-amd64 kubeconform_sum=71dbc87ac9f24099a62b93570e65aa06312ba6ac8aea63b7f86e9d999edf5a92
    kube_linter_asset=darwin kube_linter_sum=c62e8af3c9df2557c7a3922119ea1b35597794737d1ccad493f63a0d66e7b8fc
    shellcheck_asset=darwin.x86_64 shellcheck_sum=c2c15e08df0e8fbc374c335b230a7ee958c313fa5714817a59aa59f1aa594f51 ;;
  Darwin-arm64)
    sops_asset=darwin.arm64 sops_sum=b97c0d434aab577dc40310e8d22ff9e45eef4c80638ab978daae9b4681c59286
    kustomize_asset=darwin_arm64 kustomize_sum=4cbb08e209d8fff47cd60bfb6407daaf5ee699f9b9295849a1065024fac2d1ef
    kubeconform_asset=darwin-arm64 kubeconform_sum=f84f4dfbebf4a6b0b230385fa065a39ea35e02608c2b50d025dcf64775a69d67
    kube_linter_asset=darwin_arm64 kube_linter_sum=6e3443a8ff8625a9fc31a38682c783988d7559018f7ff707a4f8c77c18c92f14
    shellcheck_asset=darwin.aarch64 shellcheck_sum=339b930feb1ea764467013cc1f72d09cd6b869ebf1013296ba9055ab2ffbd26f ;;
  *) echo "no check tools for $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac

sha256() {
  if command -v sha256sum > /dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d ' ' -f 1
}

# fetch <name> <version> <url> <sha256> [path of the binary in the .tar.gz; none: the binary]
fetch() {
  local name="$1" version="$2" url="$3" sum="$4" member="${5:-}" tmp
  if [ -x "$bin/$name" ] && [ "$(cat "$bin/.$name.version" 2> /dev/null)" = "$version" ]; then return 0; fi
  tmp="$(mktemp -d)"
  curl -fsSL -o "$tmp/download" "$url"
  if [ "$(sha256 "$tmp/download")" != "$sum" ]; then
    echo "$name $version: the download does not match its sha256" >&2
    rm -rf "$tmp"
    exit 1
  fi
  if [ -n "$member" ]; then
    tar -xzf "$tmp/download" -C "$tmp" "$member"
    mv "$tmp/$member" "$bin/$name"
  else
    mv "$tmp/download" "$bin/$name"
  fi
  chmod +x "$bin/$name"
  printf '%s' "$version" > "$bin/.$name.version"
  rm -rf "$tmp"
  echo "$name $version" >&2
}

fetch kustomize "$kustomize_version" \
  "https://github.com/kubernetes-sigs/kustomize/releases/download/kustomize%2F$kustomize_version/kustomize_${kustomize_version}_$kustomize_asset.tar.gz" \
  "$kustomize_sum" kustomize
fetch kubeconform "$kubeconform_version" \
  "https://github.com/yannh/kubeconform/releases/download/$kubeconform_version/kubeconform-$kubeconform_asset.tar.gz" \
  "$kubeconform_sum" kubeconform
fetch kube-linter "$kube_linter_version" \
  "https://github.com/stackrox/kube-linter/releases/download/$kube_linter_version/kube-linter-$kube_linter_asset.tar.gz" \
  "$kube_linter_sum" kube-linter
fetch shellcheck "$shellcheck_version" \
  "https://github.com/koalaman/shellcheck/releases/download/$shellcheck_version/shellcheck-$shellcheck_version.$shellcheck_asset.tar.gz" \
  "$shellcheck_sum" "shellcheck-$shellcheck_version/shellcheck"
fetch sops "$sops_version" \
  "https://github.com/getsops/sops/releases/download/$sops_version/sops-$sops_version.$sops_asset" \
  "$sops_sum"
