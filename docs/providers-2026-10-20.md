# Sandbox providers for Tenbox machines (official docs, read 2026-10-20)

Need per task: our Dockerfile (bun, git, gh, claude), outbound internet, ~2 GB disk that
survives stop/start for days, copy ~/.claude in, exec `tenbox approve <id>` from outside,
one Claude Code run of 20-60 min, EU region, API with a token. Our profile: ~1 vCPU avg,
1.1 GB RAM, 1 h, 2 GB disk. Hetzner today: cx33 0.0136 EUR/h billed per started hour,
20 primary IPs, ~32 shared vCPU per project.

| Provider | Own image | Disk kept on stop | Keep-awake caveat | Price for our task (~1 h) | Plan fee | Concurrency | EU | Source/lock-in |
|---|---|---|---|---|---|---|---|---|
| Fly Machines | yes (registry.fly.io) | volume, $0.15/GB-mo, survives destroy | none: we stop/start ourselves | shared-1x 2 GB $0.0176/h (fra x1.15) + volume ~ $0.03 | none | dynamic cap (~50), raise by mail | ams arn cdg fra lhr | closed platform, flyctl OSS, no SDK (plain fetch) |
| Fly Sprites | no (fixed Ubuntu, claude+bun preinstalled) | 100 GB auto-synced, survives sleep | must hold a Task (<=1 h, renew) or it sleeps after ~30 s idle | $0.0385/CPU-h + $0.0219/GB-h -> ~$0.08; Fly example 4 h Claude = $0.23 | none | PAYG 3 running; Hero $100/mo 100 | region not chosen | closed |
| Daytona | yes (Dockerfile snapshot) | yes, stopped = disk only; auto-archive 7 d | auto-stop ignores background processes: set auto-stop 0 | 1 vCPU $0.0504 + 2 GiB $0.032 -> ~$0.09; $200 free credit | none | tier 1: 10 vCPU pool; tier 2 ($25): 100 | eu | closed since 2026-06-11; good API: sessions, re-attach, ssh, upload |
| Vercel Sandbox | yes (vcr registry) | auto-snapshot on stop, 30 d; drives | session max 45 min Hobby / 24 h Pro; extendTimeout | active CPU $0.128/h + 2 GB $0.042/h -> ~$0.07 | Pro $20/mo | 10 Hobby / 10,000 Pro | fra1 dub1 lhr1 cdg1 arn1 | low lock-in, SDK/CLI from anywhere |
| E2B | yes (Template SDK) | pause keeps disk+RAM, free while paused | 1 h runtime on Hobby; 24 h on Pro | 2 vCPU/2 GiB ~$0.13/h | Pro $150/mo | 20 Hobby / 100 Pro | EU on Pro via support | infra OSS (Apache-2), self-host on one KVM host possible |
| CodeSandbox | yes (.codesandbox/Dockerfile) | hibernate keeps disk+RAM 7 d | hibernationTimeout | Nano 2/4 $0.149/h, per started minute | free 10 VMs | 10 / 250 | not documented | legacy SDK; successor Together Sandbox needs sales enablement |
| Cloudflare | yes (wrangler) | NO: disk wiped on stop | running command does not count as activity | ~$0.072/vCPU-h + $0.009/GiB-h | Workers Paid $5/mo | ? | no region choice | orchestrator must be a Worker/DO |

## Verdict
1. Fly Machines: try first (TEN-450). Cheapest, our image, EU, volume, per-second, no fee, no keep-awake logic.
2. Daytona: plan B. Best API for our flow (background session + re-attach + ssh + file upload), $200 credit, eu. Closed source; 10 vCPU on tier 1.
3. Vercel Sandbox: plan C. Solid, low lock-in, but $20/mo and ~2x Fly per task.
4. Sprites: zero-ops but no own image, no region choice, 3 concurrent on PAYG. Look at it only after Fly Machines.
5. E2B, CodeSandbox, Cloudflare: no.

## Key URLs
Fly: docs.fly.io/machines/api/machines-resource, docs.fly.io/volumes/overview, docs.fly.io/about/pricing, fly.io/pricing-update/
Sprites: docs.fly.io/sprites/keeping-sprites-running.md, docs.fly.io/sprites/concepts/lifecycle.md, fly.io/sprites
Daytona: daytona.io/docs/en/sandboxes/, /persistence/, /process-code-execution/, /limits/, daytona.io/pricing, daytona.io/dotfiles/updates/daytona-is-going-closed-source
Vercel: vercel.com/docs/sandbox/pricing, /concepts/persistent-sandboxes, /concepts/regions
E2B: docs.e2b.dev/sandbox/rate-limits, /sandbox/persistence, e2b.dev/pricing, github.com/e2b-dev/infra
CodeSandbox: github.com/codesandbox/docs (sdk/*.mdx), docs.together.ai/docs/together-code-sandbox
Cloudflare: developers.cloudflare.com/sandbox/concepts/lifetime/, /containers/pricing/

## Fly Machines experiment (TEN-450, task TEN-451, 2026-10-20)
- app tenbox-box, region fra, image registry.fly.io/tenbox-box:v2 (295 MB, remote build 82 s)
- volume t_ten_451 3 GB: create 1.8 s; mounted by Fly at /home/tenzel (no bind mounts needed)
- machine shared-cpu-1x 2 GB: `fly machine run` 27 s to started; Firecracker boot 1.4 s on later starts
- boot script (secrets from app, gh login, clone, tenbox install): 3 s; setup bun install 1.8 s
- fix needed: image must keep .git (tenbox install wants a repo) -> v2
- tenbox start 12:49:32 -> plan ready 12:51:56 (2.4 min); Remote Control link worked
- approve over `fly machine exec`: 0.7 s; decide via tenbox-run: sent the build
- secrets: `fly secrets set --stage` by the user (auto mode refuses secret-store writes); machines get them at create
- trial: a machine stops after 5 min until a card is added; build resumed after `fly machine start` (2.2 s) + boot 3 s
- `fly machine exec` timeout max 600 s; long commands must detach (nohup) and be polled
- `fly machine run IMAGE --command X` is ignored: the command goes positionally (`fly machine run IMAGE /bin/sleep infinity`)
- shared-cpu-1x under 90 min of build load: steal 88-92%, ~1/12 core; checks hit timeouts -> rounds 1 and 2 failed (42, 62 new failures in `tenbox pr --wait` tests that pass on a fresh box); no build.json, session idle after 106 min; machine stopped 15:35 UTC, volume kept
- after ~20 min idle the same machine ran the loop in 1.5 s again (burst credit back)

## Raw benchmark (bench.sh: 1e9-iteration bun loop, steal over 5 s, `bun test cli domain rules`), 2026-10-20
| box | cores / RAM | single loop | all cores | steal | bun test |
|---|---|---|---|---|---|
| Hetzner cx33 (hel1) | 4 / 7.7 GB | 1.8 s | 1.9 s (4) | 0% | 96 s, 591 pass |
| Fly shared-cpu-2x fresh | 2 / 0.5 GB | 1.6 s | 1.5 s (2) | 0% | 92 s, 590 pass 1 fail |
| Fly shared-cpu-4x fresh | 4 / 1 GB | 1.6 s | 1.7 s (4) | 0% | 78 s, 591 pass |
| Fly performance-2x | 2 / 4 GB | 1.6 s | 1.5 s (2) | 0% | 83 s, 591 pass |
| Fly Sprite | 8 (shared host) / 16 GB | 1.7 s | 1.9 s (8) | 0% | 116 s, 561 pass 30 fail (env: user sprite) |
| Daytona daytona-large (eu) | 4 (cgroup, 48-core host) / 8 GB | 1.2 s | 1.6 s (4) | 0% | 107 s, 563 pass 28 fail (env: user daytona, Debian 13) |
Fresh Fly shared machines burst at full speed for minutes; sustained load gets throttled (see the 1x task machine). Sustained 25-min test on shared-cpu-2x: see bench-fly-sustain.log.

## Sustained load (full load on every allowed core; one timed 1e9 loop per minute)
- Fly shared-cpu-2x: min 1 2.5 s / 0 % steal, min 2 11.4 s / 81 %, min 3-19 23-39 s / 91-93 %. Throttled to ~1/15 core after one minute and stays there. Unusable for builds.
- Daytona 4 vCPU (cgroup 400000/100000 on a 48-core host, Debian 13): min 1-20 loop 1.6-2.0 s, 0 % steal, ~1 s throttle per minute (5 processes on a 4-core quota). Full cores for 20 min.
- Daytona short bench: create 1.6 s, single loop 1.2 s, 4 loops 1.6 s, bun test 107 s (28 env fails: user daytona, Debian). Toolbox API: `GET /api/sandbox/{id}/toolbox-proxy-url` -> `{url}/{id}/process/execute`, `{url}/{id}/files/upload-v2?path=` (multipart `file`), Bearer = API key. Sandbox deleted after the test.
- Hetzner cx33: 0 % steal during 90-min builds all night (unit checks 87-105 s every time).
Verdict 2026-10-20 evening: Fly shared out. Real candidates: Daytona (1.6 s create, full cores, per-second, $200 credit, API fits), Fly performance (dedicated, per-second, more API work), Hetzner host model (fast, cheapest, our code). Vercel still to measure.

## 2026-10-21: direction change and projection table
Decision in discussion: build our own box on k3s (Hetzner), learn Kubernetes, keep Tenbox as the orchestrator; runtime in its own open-source repo. Vercel test skipped.
Positioning research: sandbox layer and tracker->PR layer are crowded (Symphony Apr 2026, Vibe Kanban, OrchestratorInc, SuperPlane, no_human); open gap = trust/proof in the PR (Sonar 2026: 48 % verify, 38 % say AI code harder to review; Qodo 2026: review/validation is the top bottleneck).

| metric | Hetzner VM (today) | Fly Machines perf | Daytona 4 vCPU | Vercel (docs only) | k3s (projection) |
|---|---|---|---|---|---|
| warm start | 60-120 s | 5 s (2.2 + 3); new 27 s | 5 s (1.6 + boot) | few s (claim) | 3-6 s |
| setup | 20-40 s | 0 (image) | 0 (snapshot) | 0 (snapshot) | 0 (PVC snapshot) |
| waiting cost | full VM, hour-billed | disk only | disk only | free (snapshot) | ~0 (pod dead) |
| task-hour 4 vCPU/8 GB | ~0.02 EUR | ~$0.20 | $0.33 | ~$0.51 active CPU + $0.08 RAM | 0.01-0.03 EUR |
| idle agent CPU bill | full | full | full | ~0 (active CPU) | ~0 (burst share) |
| sustained steal | 0 % | perf 0 %; shared 91-93 % | 0 %, 20 min | ? | 0 % |
| agents per 4 vCPU | 1 | 1 | 1 | 1 | 4-6 |
| parallel limit | quota 32 vCPU -> 8 | dynamic ~50 | tier1 10 vCPU -> 2; tier2 25 | 10 / 10,000 | quota -> 30-45 |
| session cap | none | none | none | 45 min / 24 h | ours (8 h) |
| services | docker | processes | processes | processes | sidecars |
| fixed fee | 0 | 0 | 0 | Pro $20 | node 10-22 EUR |
| 30 agents, 2 h/day active, month | ~240 EUR | ~$264 | ~$436 | ~$20 + CPU | 66-90 EUR |
Projection numbers are estimates; measure before publishing (start time over 20 tasks, node CPU via Prometheus, invoice / agent-hours).
