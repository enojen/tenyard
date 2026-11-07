# procs

A box is one cgroup to Kubernetes: claude, the tests, `tsc` and `bun install` add up to one CPU
and one memory number. procs splits that number by what the processes are. It runs as the
`processes` sidecar of every box pod, reads the pod's processes every 100 ms and serves the CPU
and memory of each group on `:9256/metrics`, which VictoriaMetrics scrapes every 10 s.

One static Rust binary on `scratch`, about 2 MB in memory, one dependency (`regex`).

## What it measures

| Series | What |
|---|---|
| `tenyard_procs_cpu_seconds_total{group}` | CPU of the group since the pod started, reaped children included |
| `tenyard_procs_anon_bytes{group}` | the group's anonymous resident memory now: heap and stacks, no file cache |
| `tenyard_procs_anon_peak_bytes{group}` | the highest group sum at any read |
| `tenyard_procs_processes{group}` | live processes in the group |

VictoriaMetrics adds the pod's labels `task`, `project`, `command` and `class`. On the cluster
the groups summed to 99.9 % of cAdvisor's CPU for the pod (`docs/numbers.md`).

## Groups

`groups.conf` names the groups, one rule per line: `<group> <sticky|open> <regex over the
command line>`. The first matching rule wins. Below a **sticky** group every descendant stays in
it, so a whole test run is `test`, also the git it calls. Below an **open** group a descendant
that matches a rule takes that group, and one that matches none stays in its parent's group.

```
test    sticky  \b(bun test|vitest|jest|mocha|playwright test|(npm|pnpm|yarn|bun) (run )?test)\b
claude  open    ^(\S*/)?claude( |$)
```

A new test runner, linter or package manager of a project is one line here. A broken file
never stops the box: every process is then `other`, and the error goes to stderr.

## Code

| File | Does |
|---|---|
| `src/main.rs` | the options, the read loop and the HTTP server of `/metrics` |
| `src/procfs.rs` | one snapshot of `/proc`: pid, parent, command line, CPU times, anonymous memory |
| `src/groups.rs` | reads `groups.conf` and gives each process its group |
| `src/account.rs` | adds each read to the groups, also the CPU of a process that ended between two reads, through its parent's reaped-children time, and counts it once |
| `src/metrics.rs` | the Prometheus text of the counters |

```
procs [--listen 0.0.0.0:9256] [--interval-ms 250] [--groups /config/groups.conf]
      [--proc /proc] [--cgroup <path>]
```

The pod passes `--interval-ms 100`. `--cgroup` reads only the processes under one cgroup v2
path, to try it on a computer next to other processes.

## Build and test

```sh
docker build --target check procs   # cargo fmt, clippy with -D warnings, the tests
docker build procs -t procs         # the image
```

`.github/workflows/procs.yml` runs both on every change to `procs/` and pushes the image to
`ghcr.io/enojen/tenyard-procs` as `:<commit>`, on `main` also as `:latest`. `task/job.yaml`
names one image by commit and digest; a new one reaches the boxes only when that line changes.

## Safety

The sidecar runs as user 65534 with no capabilities, no privilege escalation and a read-only
root file system. So it reads the `stat`, `statm` and `cmdline` of the box's processes, but not
their `environ` or files: the env of the box holds its tokens. It exports group names only,
never a command line.

How exact it is, its known limits and the measurements behind it:
[`docs/reference.md`](../docs/reference.md), "Process metrics".
