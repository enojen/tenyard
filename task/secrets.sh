#!/usr/bin/env bash
# Writes the three shared Secrets a task Job reads, from the files on this computer. Run it
# again after a login or a token change: each Secret is replaced. Needs KUBECONFIG and kubectl.
# The tenbox login sync writes the last two itself. The tracker key and the ntfy URL are per
# project, in `project-<project id>`: `tenbox machine run` writes it before each Job.
#
#   ghcr               the pull secret of the private image: a GitHub token with
#                      read:packages, e.g. `gh auth token` (a fine-grained token cannot)
#   github-token       token: gh and git push in the box. A fine-grained token for the repos
#                      the boxes work on only: contents and pull requests read and write,
#                      metadata read; no packages, no administration, so a box cannot create a
#                      repo or reach another one (TEN-521). Without its file, the pull token.
#   claude-credentials credentials.json: ~/.claude/.credentials.json
#
# Env: NAMESPACE (tenbox), GITHUB_USER (enojen), GHCR_TOKEN_FILE (~/.config/tenbox/ghcr-token),
# GITHUB_TOKEN_FILE (~/.config/tenbox/github-token), GITHUB_BOX_TOKEN_FILE
# (~/.config/tenbox/github-box-token), CLAUDE_CREDENTIALS_FILE (~/.claude/.credentials.json).
set -euo pipefail
ns="${NAMESPACE:-tenbox}"
user="${GITHUB_USER:-enojen}"
ghcr_token_file="${GHCR_TOKEN_FILE:-$HOME/.config/tenbox/ghcr-token}"
token_file="${GITHUB_TOKEN_FILE:-$HOME/.config/tenbox/github-token}"
box_token_file="${GITHUB_BOX_TOKEN_FILE:-$HOME/.config/tenbox/github-box-token}"
[ -s "$box_token_file" ] || box_token_file="$token_file"
login_file="${CLAUDE_CREDENTIALS_FILE:-$HOME/.claude/.credentials.json}"
[ -s "$ghcr_token_file" ] || { echo "no pull token at $ghcr_token_file" >&2; exit 1; }
[ -s "$token_file" ] || { echo "no token at $token_file" >&2; exit 1; }
[ -s "$login_file" ] || { echo "no login at $login_file" >&2; exit 1; }

# `apply` replaces an existing Secret; nothing goes on a command line.
apply() { kubectl -n "$ns" create secret "$@" --dry-run=client -o yaml | kubectl -n "$ns" apply -f - ; }

umask 077
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
jq -n --rawfile t "$ghcr_token_file" --arg u "$user" \
  '($t | gsub("\\s"; "")) as $tok | { auths: { "ghcr.io": { username: $u, password: $tok, auth: (($u + ":" + $tok) | @base64) } } }' \
  > "$tmp/dockerconfig.json"
apply generic ghcr --type=kubernetes.io/dockerconfigjson --from-file=.dockerconfigjson="$tmp/dockerconfig.json"
tr -d '[:space:]' < "$box_token_file" > "$tmp/token"
apply generic github-token --from-file=token="$tmp/token"
echo "box token: $box_token_file"
apply generic claude-credentials --from-file=credentials.json="$login_file"
echo "login refresh expiry: $(jq -r '.claudeAiOauth.refreshTokenExpiresAt / 1000 | todate' "$login_file")"
kubectl -n "$ns" get secrets
