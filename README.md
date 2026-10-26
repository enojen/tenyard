# Tenyard

Tenbox is the box. Tenyard is the yard it runs in.

Tenyard is a small Kubernetes runtime for coding agents: one Job and one volume per
task, low CPU requests and high CPU limits so that several agents share a node and still burst,
and nothing running when no task runs. It is built on k3s on Hetzner, from plain
manifests. [Tenbox](https://github.com/enojen/tenbox) is its first client; any agent that
fits in a container can be the next.

The line between the two: what is inside the box belongs to the agent's own repository
(Tenbox keeps its image, `Dockerfile` and `boot.sh`, in `machines/k8s/`); where and how the
box runs belongs to Tenyard. Tenyard never knows what an image does, only how to run it.

## What is here today

- `install/`: how the cluster is set up, on one Hetzner server, with a rollback, and the
  workers that the cluster autoscaler adds for the boxes and removes when they are idle.
- `task/`: one agent run as a Job on the task's volume, with the Secrets it reads.

Everything else (snapshots, the queue, the preview URLs, the resize controller) comes step by
step; the README grows with the code.

## Why not a sandbox provider

Fly Machines, Daytona and Vercel Sandbox start a box in seconds too, but they bill the
cores you allocate, not the cores you use, and an agent uses almost none while it waits
for the model. On a shared Fly CPU a 20 minute build gets throttled to about one
fifteenth of a core. One cx33 node (4 vCPU, 8 GB, about 10 EUR a month) carries four to
six agents at once, with full cores for the one that is running its tests. The numbers
behind this are in `docs/numbers.md` once they are measured.

## One run: `task/`

One agent run is one Job: the box image, one tenbox command, the task's volume on
`/home/tenbox`, the secrets as env. The volume outlives the pod, so the next run of the task
continues from it; the pod ends when the command and the claude session it left behind are
done. `task/run.sh <task> "<command>"` applies the volume (`task/pvc.yaml`, kept when it
exists) and the Job (`task/job.yaml`), then follows the log until the pod ends:

```
KUBECONFIG=~/.kube/tenyard.yaml task/run.sh TEN-451 "tenbox start TEN-451"
PROJECT=demo REPO=https://github.com/me/demo.git task/run.sh DEMO-1 "tenbox build DEMO-1"
```

`PROJECT`, `REPO`, `IMAGE` (`ghcr.io/enojen/tenbox-box:latest`), `CLASS` (`build`) and
`NAMESPACE` (`tenbox`) are env. The Job is `task-<task>-<time>` with the labels `app: tenyard, task: <task>`; a task
has one run at a time (its volume mounts on one pod), so `run.sh` refuses while a Job of the
task is active. The Job runs at most 8 hours, never retries, gets 60 seconds to stop, and is
deleted 10 minutes after it ends. Its CPU and RAM come from a resource class (below). No box
runs on the server: a Job that fits on no worker makes the autoscaler add one, and the box
starts there in about 2 minutes instead of 20 s (`docs/numbers.md`); a worker with no box
goes after 50 minutes, near the end of its billed hour (`install/README.md`, "Workers").

### Resource classes

`task/classes.yaml` names three classes; the agent picks one per Job (tenbox: `start` and
`close` take `plan`, `build` takes `machine.class` of the project's `tenbox.yaml`, default
`build`), and the pod carries it as the label `class`:

| class | CPU request / limit | RAM, request = limit | for |
|---|---|---|---|
| `plan` | 250m / 2 | 1536Mi | a plan, a close, a short command |
| `build` | 500m / 4 | 2Gi | a build of a project with a light test sweep |
| `heavy` | 500m / 4 | 5Gi | a build whose test sweep holds several GB |

The CPU request is low and its limit high: an agent waits for the model most of the time and
needs full cores only while tests run, so boxes share the cores and the one that runs tests
bursts. RAM is different: memory that is in use cannot be taken back, so a node whose boxes ask
for less than they hold kills one of them. Two boxes with a 2Gi request and a 4 GB test sweep
each ran tenbox-ci out of memory on 2026-10-26. So the RAM request is the limit, and both follow
the measured peak.

The box logs that peak as its last line, `tenbox-boot: memory peak <MiB> MiB, cpu <s> s`
(the cgroup's `memory.peak`, file cache included, so the need is at most that). Measured on
2026-10-26, tenbox's own task TEN-476:

| run | memory peak | CPU time | time |
|---|---|---|---|
| `start` (first boot: clone and `bun install`) | 1434 MiB | 32 s | 2.5 min |
| `approve` and `build` (one round, five checks) | 1290 MiB | 1114 s | 15 min |
| `pr`, `close`, a read of the task files | 87–164 MiB | 1–3 s | 1–1.5 min |

`heavy` stays at the 5Gi that a project's 4 GB test sweep needed until its own peak is measured.

`install/limitrange.yaml` gives a container that names no resources, such as a pod made by
hand on a task volume, the `plan` class.

The Job reads four Secrets in the namespace. `task/secrets.sh` writes the first three from the
files on your computer (the agent's own login sync also writes the last two of them); the
agent writes the project Secret before each Job (tenbox: `tenbox machine run`):

| Secret | Key | What |
|---|---|---|
| `ghcr` | `.dockerconfigjson` | pull secret of the private image (a GitHub token with `read:packages`) |
| `github-token` | `token` | `gh` and `git push` in the box |
| `claude-credentials` | `credentials.json` | the claude.ai login, `~/.claude/.credentials.json` |
| `project-<project id>` | `tracker_api_key`, `ntfy_url` | the project's tracker key and notification topic (both optional) |

What the box does with them is the image's business (tenbox `machines/k8s/boot.sh`).
