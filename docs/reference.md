# Reference

What Tenyard is made of today and how it behaves, as built. `install/README.md` says how the
cluster is built, run and rebuilt; `docs/numbers.md` holds the measurements.

## One run: `task/`

One agent run is one Job: the box image, one tenbox command, the task's volume on
`/home/tenbox`, the secrets as env. The volume outlives the pod, so the next run of the task
continues from it; the pod ends when the command and the claude session it left behind are
done. The agent fills the `__X__` fields of `task/pvc.yaml` and `task/job.yaml` and posts
both to the cluster API; the volume is kept when it exists. Tenbox does it with
`tenbox machine run` (tenbox `docs/reference.md`, "Machines"):

```
tenbox machine run TEN-451 -- tenbox start TEN-451
tenbox machine logs TEN-451 --follow
```

The Job is `task-<task>-<time>` with the labels `app: tenyard, task: <task>`; a task has one
run at a time (its volume mounts on one pod), so the agent refuses while a Job of the task is
active. The Job runs at most 8 hours, never retries, gets 60 seconds to stop, and is
deleted 10 minutes after it ends. Its CPU and RAM come from a resource class (below). No box
runs on the server: a Job that fits on no worker makes the autoscaler add one, and the box
starts there in about 2 minutes instead of 20 s (`docs/numbers.md`); a worker with no box
goes after 50 minutes, counted from its last box, not from its billed hour (#33 changes that;
`install/README.md`, "Workers").

The box runs as `tenbox` (uid 1001), not root, with no privilege escalation, no capabilities
and the runtime's default seccomp profile; the pod's `fsGroup` gives a new volume to its group.
A pod of namespace `tenbox` reaches DNS and the internet only: the NetworkPolicy `box-egress`
(`install/cluster/networkpolicy.yaml`) drops its traffic to the private ranges, the link-local
range with the cloud metadata endpoint, and so to the cluster API, the nodes and other pods.
Ingress is open, so the procs sidecar is still scraped. A pod gets at most 2048 PIDs (the
workers' kubelet, `install/autoscaler/worker-cloud-init.yaml`), and the box at most 4Gi of the
node's disk outside its volume (`ephemeral-storage`, request 256Mi): past it the kubelet evicts
the pod (`docs/numbers.md`, "PIDs and local disk of a box").

The box goes out through the egress proxy, Squid in namespace `egress` (`install/egress`): the
Job sets `HTTPS_PROXY`, `HTTP_PROXY`, `NO_PROXY` (localhost, for the services of a test),
`NODE_USE_ENV_PROXY` (Node's own `fetch` reads no proxy without it), `DISABLE_TELEMETRY` and
`DISABLE_ERROR_REPORTING` (no claude telemetry out of the box;
`CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` would also turn off Remote Control), and boot.sh passes
them to the run. The proxy lets every request out today, except to the private ranges
and to ports other than 80 and 443, and writes one JSON line per request to VictoriaLogs
(`stream:egress`): time, the box's pod IP, method, host, port, status, Squid's result, bytes each
way and duration. An HTTPS request shows its host and port only; the proxy never opens TLS. Only
pods of `tenbox` reach it. A box still has DNS and a direct way out; #90 closes both and keeps
only the hosts the log shows.

Squid sends the CONNECT of four credential hosts, `github.com`, `api.github.com`, `api.linear.app`
and `api.clickup.com`, to the injector (a sidecar in Squid's pod, `install/egress/proxy.yaml`; code
in `injector/`) on `127.0.0.1:3129` and never straight out, so without the injector these hosts fail. The injector ends the box's TLS
with a leaf of its own CA and writes the run's real token in place of the run placeholder
(`tbxph_...`) in `Authorization`; it asks the tenbox server for that token (`POST /run-credentials`
with its own token): the run token for GitHub, the project's tracker key for its tracker's host.
Any other `Authorization` passes as it is, and it never reads a body. The box
trusts the CA through the ConfigMap `injector-ca` of namespace `tenbox`, mounted at
`/etc/tenyard/injector/ca.crt` and named by `TENBOX_INJECTOR_CA`: boot.sh adds it to the system
roots for git, gh, curl, node and bun. The CA's name constraints allow only these four names (and
names under them), so its key cannot stand in for another site. The injector starts before Squid
and its readiness is the pod's: a pod whose injector is down leaves the Service, so Squid never
marks it dead (a Squid that starts without it, or that fails 10 connections to it, fails the
first request after it is back). The pod reaches port 4300 of the server for the lookup.

### Resource classes

`task/classes.yaml` names three classes; the agent picks one per Job (tenbox: `start` and
`close` take `plan`, `build` takes `machine.class` of the project's `tenbox.yaml`, default
`build`), and the pod carries it as the label `class`:

| class | CPU request | RAM, request = limit | for |
|---|---|---|---|
| `plan` | 250m | 1536Mi | a plan, a close, a short command |
| `build` | 500m | 2Gi | a build of a project with a light test sweep |
| `heavy` | 500m | 3Gi | a build whose check chain holds up to about 2.5 GB |

The CPU request is low and there is no CPU limit: an agent waits for the model most of the time
and needs full cores only while tests run, so boxes share the cores by their requests and the
one that runs tests takes every idle core. A limit would throttle it while the node has cores to
spare. RAM is different: memory that is in use cannot be taken back, so a node whose boxes ask
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

`heavy` was 5Gi for a project's 4 GB test sweep; with its database moved to a Postgres service
(below), that project's whole check chain peaks at about 2.3 GB, so `heavy` is 3Gi.

`install/cluster/limitrange.yaml` gives a container that names no resources, such as a pod made by
hand on a task volume, the `plan` class, without a CPU limit too.

### Services

A project whose tests need a database gets it as a service: a sidecar container in the pod of
its Job, reached on `127.0.0.1`. A pod has no docker, but it can hold more containers. Each file
in `task/services/` is one service, written as a strategic merge patch of the Job: the sidecar,
its volumes and the env of the `box` container. The agent picks the services per Job (tenbox:
`machine.services` of the project's `tenbox.yaml`, on `build` only) and adds each file as
`kubectl patch --type strategic` does: an entry replaces the one of its name.

| service | what | box env |
|---|---|---|
| `postgres` | `pgvector/pgvector:pg16`, data in memory, no flush; RAM 512Mi request, 1Gi limit | `TEST_PG_URL` |

A sidecar is a native one, an init container with `restartPolicy: Always`: the box starts only
after its startup probe passes, and the Job ends when the box ends. Its RAM is its own, on top of
the class of the box; data in an `emptyDir` with `medium: Memory` counts in that RAM.

The Job reads four Secrets and one ConfigMap in the namespace. The first Secret is in
`secrets/tenbox/`, applied by Flux; the tenbox server writes the run placeholder for each Job,
the agent's login sync writes the login, and the agent writes the project Secret before each Job
(tenbox: `tenbox machine run`). The ConfigMap `injector-ca` (`ca.crt`) is the injector's CA,
from `install/egress/injector-ca.crt`:

| Secret | Key | What |
|---|---|---|
| `ghcr` | `.dockerconfigjson` | pull secret of the private image (a GitHub token with `read:packages`) |
| `task-<name>-<run>-github` | `token`, `tracker` | the run placeholder, `tbxph_` and an HMAC of the Job's uid: the injector writes the run token in its place, a GitHub App token for the repository of the Job's project only (contents, pull requests and issues write), one hour long. Empty when the App is not installed there, and the box then stops at start. `tracker` is the same placeholder, only when the server has a tracker key for the project; the Job reads it as `TRACKER_API_KEY` and the injector writes that key in its place on the tracker's host only. The pod waits until the server writes it; the Job owns it, so it goes with the Job. The Job carries the label `project-id` for it |
| `claude-credentials` | `credentials.json` | the claude.ai login, `~/.claude/.credentials.json` |
| `project-<project id>` | `ntfy_url` | the project's notification topic (optional) |

What the box does with them is the image's business (tenbox `machines/k8s/boot.sh`).

### Process metrics

cAdvisor sees a box as one cgroup: claude, the tests, `tsc` and the installs are one number.
To see which of them uses the CPU and the RAM, every box pod has a second native sidecar,
`processes`: `procs/`, a small Rust program (one static binary on scratch, image
`ghcr.io/enojen/tenyard-procs`, pushed by `.github/workflows/procs.yml`), on port 9256. The pod
shares one process namespace (`shareProcessNamespace`), so the sidecar sees the box's processes.
VictoriaMetrics scrapes it every 10 s (`install/README.md`, "Metrics").

It reads `/proc` every 100 ms and counts each process's own CPU (utime + stime) and the growth of
its cutime + cstime: the kernel adds a child's whole CPU to its parent when the parent reaps it,
so a process that starts and ends between two reads is not lost. A vanished process hands what
was already counted of it to its nearest live ancestor, so only its unseen tail is added, once.
Its counters run from the pod's start, and the sidecar stays 15 s after the box ends
(`preStop`), so the last scrape reads the end. process-exporter, which counts only the processes
alive at a scrape, saw 19 % of a box's CPU on a check run of tenbox (2026-11-06).

Memory is each process's anonymous resident memory, its heap and stacks: resident less shared in
`/proc/<pid>/statm`, the `RssAnon` of `status`. Its sum over a group is what the group's own
cgroup counts as `anon`. Resident memory (RSS) would count a shared page of a file, the bun binary
and its libraries, once in every process: on a `bun test` run the RSS sum was 2.6 times the
group's real memory. The file cache is not in any group; cAdvisor has it for the pod.

`procs/groups.conf`, inside the image, puts each process into the first group whose rule matches
its command line. Below a sticky group every descendant stays in it (a whole test run is `test`,
also the git it calls); below an open group a descendant that matches a rule takes that group,
and one that matches none stays in its parent's group:

| group | kind | what |
|---|---|---|
| `claude` | open | claude and the tool commands it runs that match no other group |
| `tenbox` | open | the tenbox CLI |
| `test` | sticky | `bun test`, vitest, jest, mocha, playwright, `npm`/`pnpm`/`yarn`/`bun` `test` |
| `tsc` | sticky | `tsc`, `tsgo`, `vue-tsc` |
| `lint` | sticky | biome, eslint, oxlint, prettier, knip |
| `install` | sticky | `bun`, `npm`, `pnpm`, `yarn` `install`, `ci`, `i`, `add` |
| `git` | sticky | git and its helpers |
| `procs` | open | procs itself, in its own sidecar |
| `service` | sticky | the sidecars of `task/services/` |
| `other` | open | boot.sh, sudo, its poll of the sessions, the pause process |

The series are `tenyard_procs_cpu_seconds_total`, `tenyard_procs_anon_bytes`,
`tenyard_procs_anon_peak_bytes` (the highest group sum at any read) and
`tenyard_procs_processes`, per `group`, with the pod labels `task`, `project`, `command` and
`class`.

How exact it is, measured on a computer: a systemd scope with one child cgroup per group gives
each group's exact CPU, while procs runs on the same processes with its rules (`bun test
cli/commands` of tenbox, `tsgo` three times, a git loop and a claude-like loop with tool
commands, 85-123 s of CPU). Four runs, 2026-11-06:

| | 250 ms | 100 ms |
|---|---|---|
| all groups | 99.8 % | 99.8 % |
| test, tsc | 99.7-100 % | 99.7-100 % |
| git (3-5 s) | 86-117 % | 93-98 % |
| claude (2-3 s) | 67-126 % | 103-114 % |

The small groups trade a few tenths of a second: a git call of claude that lives less than one
read counts as claude, the group of its parent.

Memory on the same runs, with the memory controller on each child cgroup: its `anon` read every
50 or 200 ms next to procs at 100 ms, compared at the same instant (the area: the sum over all
reads) and at the highest read. Four runs, 2026-11-06:

| | area | peak | peak of the RSS sum |
|---|---|---|---|
| all groups (607-625 MiB) | 99.0-100.1 % | 99.6-101.5 % | 162-173 % |
| test (377-423 MiB) | 99.8-101.0 % | 88.3-100.2 % | 260 % |
| tsc (539-549 MiB) | 95.3-100.3 % | 99.5-101.5 % | 106 % |
| git, claude (about 1 MiB) | 107-154 % | 97-142 % | 900-1600 % |

The 88.3 % is a run that read procs every 200 ms and so skipped half of its reads at the test
run's short top. procs's own `tenyard_procs_anon_peak_bytes`, which keeps the top of every read,
against the truth read every 50 ms: test 420 of 423 MiB, tsc 542 of 543 MiB. The small groups
are off by less than 1 MiB.

On the cluster, a box that ran tenbox's checks three times and its e2e tests (2026-11-06): the
groups summed to 108.48 s, cAdvisor's CPU of the pod to 108.54 s (99.9 %): test 87.3, tsc 12.5,
lint 4.3, other 2.2, install 1.5, git 0.4, tenbox 0.2. procs itself used 0.9 s of CPU in the 2.6
minutes and 1 MiB.

The sidecar is not a way into the box. It runs as user 65534 with no capabilities, no privilege
escalation and a read-only root file system, so it reads the `stat`, `statm` and `cmdline` of the
box's processes but not their `environ`, `io` or files (`/proc/<pid>/root`); a root sidecar in
the same pod reads the env of the box's boot process, which holds the tokens (tested 2026-11-06).
It exports group names only, never a command line. It asks for 10m CPU and 8Mi, limit 32Mi, on
top of the class of the box; it holds about 2 MB. A broken groups file never stops the box: every
process is then `other`.

Known limits:

- A process reaped by a parent that is not in the pod, or that ignores its children, loses the
  CPU it had after its last read.
- Memory is anonymous only: the file cache, the kernel's own memory of the pod (page tables,
  sockets) and shared memory (`/dev/shm`, a database's shared buffers) are in no group. cAdvisor
  counts the pod's cgroup, which holds them all.
- A peak shorter than one read (100 ms) is missed.
- The planner, worker and reviewer subagents share one `claude` process and are not split.
- PID 1 of the pod is the pause process, not boot.sh. So boot.sh ends at once on the `SIGTERM`
  of a deleted pod; as PID 1 it ignored the signal and ran on until it ended by itself or the
  60 s grace ran out (a deleted `status` run, 2026-11-06: gone after 2 s, before after 31 s).

## Tools

`tools/install.sh` downloads every tool into `.tools/bin`, each pinned by version and sha256:
kustomize, kubeconform, kube-linter, shellcheck, sops, tofu and flux. It also points git at
`.githooks/`.

| Script | Does |
|---|---|
| `check-manifests.sh` | builds the four Flux layers and Flux itself, validates every object against the API of the cluster's version (kubeconform, strict) and lints it (kube-linter); an object that breaks a check on purpose says why in an `ignore-check.kube-linter.io/<check>` annotation |
| `check-scripts.sh` | shellcheck on every script and git hook |
| `check-secrets.sh` | fails on a file in `secrets/` that sops did not encrypt and on a Secret with values in a layer; `--staged` is the pre-commit hook |
| `check-infra.sh` | `tofu fmt` and `validate` of `infra/`, with no token and no state |
| `apply-secrets.sh --diff` | what the Secrets in git would change on the cluster, values hidden |
| `check-backup.sh [<day>]` | downloads a day's backups and checks that they open, decrypted in RAM in a container with no network |
| `restore-server.sh <day>` | puts the server's database of a backup on the cluster of `KUBECONFIG` |

The first four are the checks of `tenbox.yaml` and of CI on every PR.
