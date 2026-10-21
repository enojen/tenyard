# Tenyard

Tenbox is the box. Tenyard is the yard it runs in.

Tenyard is a small Kubernetes runtime for coding agents: one Job and one volume per
task, low requests and high limits so that several agents share a node and still burst,
and nothing running when no task runs. It is built on k3s on Hetzner, from plain
manifests. [Tenbox](https://github.com/enojen/tenbox) is its first client; any agent that
fits in a container can be the next.

The line between the two: what is inside the box belongs to the agent's own repository
(Tenbox keeps its image, `Dockerfile` and `boot.sh`, in `machines/k8s/`); where and how the
box runs belongs to Tenyard. Tenyard never knows what an image does, only how to run it.

## What is here today

- `install/`: how the cluster is set up, on one Hetzner server, with a rollback.
- `task/`: one agent run as a Job on the task's volume, with the Secrets it reads.

Everything else (resource classes, snapshots, the autoscaler, the
queue, the preview URLs, the resize controller) comes step by step; the README grows with
the code.

## Why not a sandbox provider

Fly Machines, Daytona and Vercel Sandbox start a box in seconds too, but they bill the
cores you allocate, not the cores you use, and an agent uses almost none while it waits
for the model. On a shared Fly CPU a 20 minute build gets throttled to about one
fifteenth of a core. One cx33 node (4 vCPU, 8 GB, about 10 EUR a month) carries four to
six agents at once, with full cores for the one that is running its tests. The numbers
behind this are in `docs/numbers.md` once they are measured.

## One run: `task/`

One agent run is one Job: the box image, one tenbox command, the task's volume on
`/home/tenzel`, the secrets as env. The volume outlives the pod, so the next run of the task
continues from it; the pod ends when the command and the claude session it left behind are
done. `task/run.sh <task> "<command>"` applies the volume (`task/pvc.yaml`, kept when it
exists) and the Job (`task/job.yaml`), then follows the log until the pod ends:

```
KUBECONFIG=~/.kube/tenyard.yaml task/run.sh TEN-451 "tenbox start TEN-451"
PROJECT=demo REPO=https://github.com/me/demo.git task/run.sh DEMO-1 "tenbox build DEMO-1"
```

`PROJECT`, `REPO`, `IMAGE` (`ghcr.io/enojen/tenbox-box:latest`) and `NAMESPACE` (`tenbox`)
are env. The Job is `task-<task>-<time>` with the labels `app: tenyard, task: <task>`; a task
has one run at a time (its volume mounts on one pod), so `run.sh` refuses while a Job of the
task is active. The Job runs at most 8 hours, never retries, gets 60 seconds to stop, and is
deleted 10 minutes after it ends. Requests are low (0.5 CPU, 2 GiB) and limits high
(4 CPU, 5 GiB), so several agents share the node and the one that runs tests bursts.

The Job reads five Secrets in the namespace; `task/secrets.sh` writes them from the files
on your computer, until the agent's own login sync does:

| Secret | Key | What |
|---|---|---|
| `ghcr` | `.dockerconfigjson` | pull secret of the private image (a GitHub token with `read:packages`) |
| `github-token` | `token` | `gh` and `git push` in the box |
| `claude-credentials` | `credentials.json` | the claude.ai login, `~/.claude/.credentials.json` |
| `tracker` | `api_key` | the tracker key (optional) |
| `ntfy` | `url` | the notification topic (optional) |

What the box does with them is the image's business (tenbox `machines/k8s/boot.sh`).
