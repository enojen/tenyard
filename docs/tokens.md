# Tokens

Every secret the cluster holds: what it can do, where it is made, where it lives. On a leak,
make a new one where it is made, put it where it lives, and revoke the old one. A token that
was in `secrets/` stays readable in git history to anyone with the age key, so a leaked age key
means a new token for every row marked "git".

| Secret (namespace/name) | What it can do | Made in | Lives in |
|---|---|---|---|
| `kube-system/hcloud` `token` | Hetzner project, read and write: servers, volumes, networks. The CSI driver, the CCM and the autoscaler use it | Hetzner console, project, Security, API tokens | git |
| `kube-system/hcloud` `network` | not a secret: the name of the private network | | git, with the token |
| `tenbox-server/hcloud` `token` | Hetzner project, read only: the server reads each worker's type and price | Hetzner console | git |
| `tenbox/ghcr`, `tenbox-server/ghcr` | pull the private images from GHCR (`read:packages` of a classic token) | GitHub, Settings, Developer settings, Personal access tokens (classic) | git |
| `tenbox-server/github-app` `key.pem` | the private key of the GitHub App `tenbox-enojen` (id 5669876): mints a one-hour token with contents, pull requests and issues write for any repository the App is installed on. The server gives each Job one for its project's repository only (`task-<name>-<run>-github`) | GitHub, Settings, Developer settings, GitHub Apps, tenbox-enojen, Private keys; revoke the old key there | git |
| `tenbox/task-<name>-<run>-github` `token` | nothing by itself: the run placeholder. Through the injector, while its Job is open, `gh` and `git push` of that run, in one repository | the tenbox server, for each Job | the cluster only; goes with its Job |
| `tenbox-server/run-credentials` `key` | makes the run placeholder of any Job from its uid. Changing it breaks the runs open at that time | `openssl rand -base64 32` | git |
| `tenbox-server/run-credentials` `injector-token`, `egress/injector` `token` | the same value: asks the server for the run token of any open Job's placeholder (`POST /run-credentials`); opens no other route | `openssl rand -base64 32` | git |
| `egress/injector` `ca.key` | the key of the injector's CA: a certificate that the boxes trust, for the four credential hosts only (name constraints). A new key needs a new `install/egress/injector-ca.crt` in the same commit; runs open at that time keep the old CA and lose GitHub until they end | `install/README.md`, "The injector" | git |
| `tenbox-server/ntfy` `url` | sends a notice to the ntfy topic | the topic name itself | git |
| `tenbox-server/backup-bucket` `access_key`, `secret_key` | read and write every bucket of the Hetzner project's Object Storage: the daily copy off the host (install/README.md, "Backups") | Hetzner console, project, Security, S3 credentials | git |
| `monitoring/grafana-admin` `password` | the Grafana admin login | `openssl rand` | git |
| `tenbox/claude-credentials` | the claude.ai login of the boxes | `/login` on the computer | the login sync writes it (tenbox `machines/login/`), the only Secret it writes; never git |
| `tenbox/project-<project id>` | a project's tracker key and ntfy URL | the project's `tenbox.yaml` and local layers | `tenbox machine run` writes it before each Job; never git |
| `kube-system/tenyard-autoscaler` | the k3s join token (joins workers only) and the worker cloud-init | `k3s token create` on the server | `install/autoscaler/autoscaler.sh` writes it; it was left out of #68 |
| `monitoring/tailscale-auth` | a Tailscale auth key, used once | Tailscale admin console | by hand; can be deleted after the first start |
| `monitoring/tailscale-grafana` | the Tailscale node key, written by the node itself | | the cluster only |

Outside the cluster:

| Secret | What it can do | Lives in |
|---|---|---|
| the age key of Tenyard (`~/.config/tenyard/age.key`) | decrypts everything in `secrets/` | the computer and a password manager; never git |
| the kubeconfig (`~/.kube/tenyard.yaml`) | admin of the cluster | the computer |
