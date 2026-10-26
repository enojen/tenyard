#!/usr/bin/env bash
# Writes the three shared Secrets a task Job reads, from the files on this computer. Run it
# again after a login or a token change: each Secret is replaced. Needs KUBECONFIG and kubectl.
# The tenbox login sync writes the last two itself. The tracker key and the ntfy URL are per
# project, in `project-<project id>`: `tenbox machine run` writes it before each Job.
#
#   ghcr               the pull secret of the private image (GitHub token with read:packages)
#   github-token       token: gh and git push in the box
#   claude-credentials credentials.json: ~/.claude/.credentials.json
#
# Env: NAMESPACE (tenbox), GITHUB_USER (enojen), GITHUB_TOKEN_FILE (~/.config/tenbox/github-token),
# CLAUDE_CREDENTIALS_FILE (~/.claude/.credentials.json).
set -euo pipefail
ns="${NAMESPACE:-tenbox}"
user="${GITHUB_USER:-enojen}"
token_file="${GITHUB_TOKEN_FILE:-$HOME/.config/tenbox/github-token}"
login_file="${CLAUDE_CREDENTIALS_FILE:-$HOME/.claude/.credentials.json}"
[ -s "$token_file" ] || { echo "no token at $token_file" >&2; exit 1; }
[ -s "$login_file" ] || { echo "no login at $login_file" >&2; exit 1; }

# `apply` replaces an existing Secret; nothing goes on a command line.
apply() { kubectl -n "$ns" create secret "$@" --dry-run=client -o yaml | kubectl -n "$ns" apply -f - ; }

umask 077
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
jq -n --rawfile t "$token_file" --arg u "$user" \
  '($t | gsub("\\s"; "")) as $tok | { auths: { "ghcr.io": { username: $u, password: $tok, auth: (($u + ":" + $tok) | @base64) } } }' \
  > "$tmp/dockerconfig.json"
apply generic ghcr --type=kubernetes.io/dockerconfigjson --from-file=.dockerconfigjson="$tmp/dockerconfig.json"
tr -d '[:space:]' < "$token_file" > "$tmp/token"
apply generic github-token --from-file=token="$tmp/token"
apply generic claude-credentials --from-file=credentials.json="$login_file"
echo "login refresh expiry: $(jq -r '.claudeAiOauth.refreshTokenExpiresAt / 1000 | todate' "$login_file")"
kubectl -n "$ns" get secrets
