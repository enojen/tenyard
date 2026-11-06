// The replay of Jobs on simulated Hetzner workers under one policy (#32). Times are in seconds.

/** A pod request: CPU in millicores, memory in MiB. */
export type Resources = { cpuMilli: number; memoryMiB: number };
export type JobClass = "plan" | "build" | "heavy";
/** One box: created at `arrival`, it runs `runSeconds` once its box has started. */
export type Job = {
  id: string;
  arrival: number;
  runSeconds: number;
  request: Resources;
  command: string;
  class: JobClass;
};
export type WorkerType = { name: string; capacity: Resources; hourlyEur: number };

/** When an empty worker goes: after 50 min empty, or at minute 50 of its paid hour (#33). */
export type EmptyRule = "after-50-min" | "hour-end";
/** Where a pod goes among the ready workers: least memory allocated, or most that still fits. */
export type Placement = "spread" | "pack";
/** The memory request of a Job: as recorded, or the measured working set of its class. */
export type RequestSource = "recorded" | "measured";
export type TypeOrder = "fixed" | "price-per-gb-hour";
export type Policy = {
  name: string;
  emptyRule: EmptyRule;
  placement: Placement;
  request: RequestSource;
  typeOrder: TypeOrder;
  warmSlots: number;
};

export type WorkerRun = {
  type: string;
  orderedAt: number;
  removedAt: number;
  paidHours: number;
  busyHours: number;
  eur: number;
};
export type RunResult = {
  eur: number;
  paidHours: number;
  busyHours: number;
  wasteHours: number;
  /** From Job created to box started. */
  waitP50: number;
  waitP95: number;
  /** Jobs that waited for a new worker. */
  coldStarts: number;
  workers: WorkerRun[];
};

const SECONDS_PER_HOUR = 3600;
const BOX_START_SECONDS = 20;
const WORKER_READY_SECONDS = 80;
const EMPTY_WAIT_SECONDS = 3000;
const HOUR_END_FROM_SECONDS = 3000;
const HOUR_END_TO_SECONDS = 3300;
// The warm slot is up one hour before the first Job, as on a cluster that runs all week.
const WARM_UP_SECONDS = 3600;
const P50 = 0.5;
const P95 = 0.95;

// The kubelet keeps 1 GiB and 250m out of the boxes (install/README.md).
const RESERVED: Resources = { cpuMilli: 250, memoryMiB: 1024 };
const WARM_SLOT: Resources = { cpuMilli: 500, memoryMiB: 2048 };
// Working set times 1.1, max over 2026-11-05/06 (decisions.md).
const MEASURED_MEMORY_MIB: Record<JobClass, number> = { plan: 418, build: 527, heavy: 2765 };

// Net EUR per hour in hel1, Hetzner GET /pricing on 2026-11-06.
export const WORKER_TYPES: readonly WorkerType[] = [
  { name: "cx43", capacity: { cpuMilli: 8000, memoryMiB: 16_384 }, hourlyEur: 0.0256 },
  { name: "cpx32", capacity: { cpuMilli: 4000, memoryMiB: 8192 }, hourlyEur: 0.0569 },
  { name: "cpx42", capacity: { cpuMilli: 8000, memoryMiB: 16_384 }, hourlyEur: 0.1114 },
];

export const P0: Policy = {
  name: "P0",
  emptyRule: "after-50-min",
  placement: "spread",
  request: "recorded",
  typeOrder: "fixed",
  warmSlots: 0,
};
export const P1: Policy = { ...P0, name: "P1", emptyRule: "hour-end" };
export const P2: Policy = { ...P1, name: "P2", placement: "pack" };
export const P3: Policy = { ...P2, name: "P3", request: "measured" };
export const P4: Policy = { ...P3, name: "P4", typeOrder: "price-per-gb-hour" };
export const P5: Policy = { ...P4, name: "P5", warmSlots: 1 };
export const POLICIES: readonly Policy[] = [P0, P1, P2, P3, P4, P5];

type Pod = { request: Resources; placeholder: boolean };
type Interval = { from: number; to: number };
type Worker = {
  type: WorkerType;
  orderedAt: number;
  readyAt: number;
  removedAt: number | undefined;
  pods: Pod[];
  busy: Interval[];
  emptySince: number | undefined;
};

type FinishEvent = { kind: "finish"; at: number; worker: Worker; pod: Pod; started: number };
type ArrivalEvent = { kind: "arrival"; at: number; job: Job };
type RemovalEvent = { kind: "removal"; at: number; worker: Worker; emptySince: number };
type SimEvent = FinishEvent | ArrivalEvent | RemovalEvent;
// At one instant a finish frees room before an arrival, and an arrival keeps a worker that
// would go at that instant.
const EVENT_RANK: Record<SimEvent["kind"], number> = { finish: 0, arrival: 1, removal: 2 };

type Sim = {
  policy: Policy;
  types: readonly WorkerType[];
  now: number;
  workers: Worker[];
  queue: SimEvent[];
  head: number;
  jobsLeft: number;
  waits: number[];
  coldStarts: number;
};

/** Which pods take room on a worker: all of them, or only real Jobs (a placeholder yields). */
type Counted = "all" | "jobs";
type BoxStart = { job: Job; worker: Worker; pod: Pod };

/** Every started hour from the order to the removal, at least one. */
export function paidHours(orderedAt: number, removedAt: number): number {
  return Math.max(1, Math.ceil((removedAt - orderedAt) / SECONDS_PER_HOUR));
}

export function workerTypeOrder(order: TypeOrder): readonly WorkerType[] {
  if (order === "fixed") return WORKER_TYPES;
  return [...WORKER_TYPES].sort((a, b) => pricePerMiB(a) - pricePerMiB(b));
}

export function replay(jobs: readonly Job[], policy: Policy): RunResult {
  const sim: Sim = {
    policy,
    types: workerTypeOrder(policy.typeOrder),
    now: 0,
    workers: [],
    queue: [],
    head: 0,
    jobsLeft: jobs.length,
    waits: [],
    coldStarts: 0,
  };
  if (jobs.length === 0) return summarize(sim);
  sim.now = Math.min(...jobs.map((job) => job.arrival)) - warmUp(policy);
  for (let slot = 0; slot < policy.warmSlots; slot++) {
    placePlaceholder(sim, { request: WARM_SLOT, placeholder: true });
  }
  for (const job of jobs) schedule(sim, { kind: "arrival", at: job.arrival, job });
  for (let event = nextEvent(sim); event; event = nextEvent(sim)) {
    sim.now = event.at;
    handle(sim, event);
  }
  return summarize(sim);
}

function pricePerMiB(type: WorkerType): number {
  return type.hourlyEur / type.capacity.memoryMiB;
}

function warmUp(policy: Policy): number {
  return policy.warmSlots > 0 ? WARM_UP_SECONDS : 0;
}

function schedule(sim: Sim, event: SimEvent): void {
  let low = sim.head;
  let high = sim.queue.length;
  while (low < high) {
    const middle = (low + high) >> 1;
    const queued = sim.queue[middle];
    if (queued && compareEvents(queued, event) <= 0) low = middle + 1;
    else high = middle;
  }
  sim.queue.splice(low, 0, event);
}

function compareEvents(a: SimEvent, b: SimEvent): number {
  return a.at - b.at || EVENT_RANK[a.kind] - EVENT_RANK[b.kind];
}

function nextEvent(sim: Sim): SimEvent | undefined {
  const event = sim.queue[sim.head];
  sim.head += 1;
  return event;
}

function handle(sim: Sim, event: SimEvent): void {
  if (event.kind === "arrival") placeJob(sim, event.job);
  else if (event.kind === "finish") finishJob(sim, event);
  else removeIfStillEmpty(event);
}

function placeJob(sim: Sim, job: Job): void {
  const pod: Pod = { request: requestOf(job, sim.policy), placeholder: false };
  const worker =
    readyWorker(sim, pod.request, "all") ??
    readyWorker(sim, pod.request, "jobs") ??
    coldWorker(sim, pod.request);
  const evicted = fits(worker, pod.request, "all") ? [] : evictPlaceholders(worker);
  bind(worker, pod);
  for (const placeholder of evicted) placePlaceholder(sim, placeholder);
  startBox(sim, { job, worker, pod });
}

function requestOf(job: Job, policy: Policy): Resources {
  if (policy.request === "recorded") return job.request;
  return { cpuMilli: job.request.cpuMilli, memoryMiB: MEASURED_MEMORY_MIB[job.class] };
}

function placePlaceholder(sim: Sim, pod: Pod): void {
  bind(readyWorker(sim, pod.request, "all") ?? coldWorker(sim, pod.request), pod);
}

function startBox(sim: Sim, box: BoxStart): void {
  const started = Math.max(sim.now, box.worker.readyAt) + BOX_START_SECONDS;
  sim.waits.push(started - box.job.arrival);
  if (box.worker.readyAt > sim.now) sim.coldStarts += 1;
  const at = started + box.job.runSeconds;
  schedule(sim, { kind: "finish", at, worker: box.worker, pod: box.pod, started });
}

function finishJob(sim: Sim, event: FinishEvent): void {
  const { worker } = event;
  worker.pods = worker.pods.filter((pod) => pod !== event.pod);
  worker.busy.push({ from: event.started, to: sim.now });
  if (worker.pods.length === 0) markEmpty(sim, worker);
  sim.jobsLeft -= 1;
  if (sim.jobsLeft === 0) dropWarmSlots(sim);
}

// The replay ends with its last Job: the warm slots go and their workers get empty.
function dropWarmSlots(sim: Sim): void {
  for (const worker of aliveWorkers(sim)) {
    if (evictPlaceholders(worker).length > 0 && worker.pods.length === 0) markEmpty(sim, worker);
  }
}

function markEmpty(sim: Sim, worker: Worker): void {
  worker.emptySince = sim.now;
  const at = removalTime(sim.policy.emptyRule, worker, sim.now);
  schedule(sim, { kind: "removal", at, worker, emptySince: sim.now });
}

function removalTime(rule: EmptyRule, worker: Worker, emptySince: number): number {
  if (rule === "after-50-min") return emptySince + EMPTY_WAIT_SECONDS;
  const intoHour = (emptySince - worker.orderedAt) % SECONDS_PER_HOUR;
  if (intoHour >= HOUR_END_FROM_SECONDS && intoHour < HOUR_END_TO_SECONDS) return emptySince;
  const untilMark = HOUR_END_FROM_SECONDS - intoHour;
  return emptySince + (untilMark > 0 ? untilMark : untilMark + SECONDS_PER_HOUR);
}

function removeIfStillEmpty(event: RemovalEvent): void {
  const { worker } = event;
  if (worker.removedAt !== undefined || worker.emptySince !== event.emptySince) return;
  worker.removedAt = event.at;
}

function aliveWorkers(sim: Sim): Worker[] {
  return sim.workers.filter((worker) => worker.removedAt === undefined);
}

function readyWorker(sim: Sim, request: Resources, counted: Counted): Worker | undefined {
  const candidates = aliveWorkers(sim).filter(
    (worker) => worker.readyAt <= sim.now && fits(worker, request, counted),
  );
  return pickByPlacement(candidates, sim.policy.placement);
}

function pickByPlacement(workers: Worker[], placement: Placement): Worker | undefined {
  let best: Worker | undefined;
  for (const worker of workers) {
    if (!best || isBetter(worker, best, placement)) best = worker;
  }
  return best;
}

function isBetter(worker: Worker, best: Worker, placement: Placement): boolean {
  const memory = allocated(worker, "all").memoryMiB;
  const bestMemory = allocated(best, "all").memoryMiB;
  return placement === "spread" ? memory < bestMemory : memory > bestMemory;
}

/** A worker that is booting and has room, else a new one. */
function coldWorker(sim: Sim, request: Resources): Worker {
  const booting = aliveWorkers(sim).find(
    (worker) => worker.readyAt > sim.now && fits(worker, request, "all"),
  );
  return booting ?? orderWorker(sim, request);
}

function orderWorker(sim: Sim, request: Resources): Worker {
  const type = sim.types.find((candidate) => fitsIn(allocatable(candidate), request));
  if (!type) throw new Error(`no worker type fits ${request.cpuMilli}m/${request.memoryMiB}Mi`);
  const worker: Worker = {
    type,
    orderedAt: sim.now,
    readyAt: sim.now + WORKER_READY_SECONDS,
    removedAt: undefined,
    pods: [],
    busy: [],
    emptySince: undefined,
  };
  sim.workers.push(worker);
  return worker;
}

function bind(worker: Worker, pod: Pod): void {
  worker.pods.push(pod);
  worker.emptySince = undefined;
}

function evictPlaceholders(worker: Worker): Pod[] {
  const placeholders = worker.pods.filter((pod) => pod.placeholder);
  worker.pods = worker.pods.filter((pod) => !pod.placeholder);
  return placeholders;
}

function fits(worker: Worker, request: Resources, counted: Counted): boolean {
  const used = allocated(worker, counted);
  const free = minus(allocatable(worker.type), used);
  return fitsIn(free, request);
}

function fitsIn(free: Resources, request: Resources): boolean {
  return request.cpuMilli <= free.cpuMilli && request.memoryMiB <= free.memoryMiB;
}

function allocatable(type: WorkerType): Resources {
  return minus(type.capacity, RESERVED);
}

function allocated(worker: Worker, counted: Counted): Resources {
  const pods = worker.pods.filter((pod) => counted === "all" || !pod.placeholder);
  return {
    cpuMilli: pods.reduce((sum, pod) => sum + pod.request.cpuMilli, 0),
    memoryMiB: pods.reduce((sum, pod) => sum + pod.request.memoryMiB, 0),
  };
}

function minus(a: Resources, b: Resources): Resources {
  return { cpuMilli: a.cpuMilli - b.cpuMilli, memoryMiB: a.memoryMiB - b.memoryMiB };
}

function summarize(sim: Sim): RunResult {
  const workers = sim.workers.map((worker) => workerRun(worker, sim.now));
  const paid = sum(workers.map((worker) => worker.paidHours));
  const busy = sum(workers.map((worker) => worker.busyHours));
  const eur = sum(workers.map((worker) => worker.eur));
  const waits = [...sim.waits].sort((a, b) => a - b);
  return {
    eur,
    paidHours: paid,
    busyHours: busy,
    wasteHours: paid - busy,
    waitP50: percentile(waits, P50),
    waitP95: percentile(waits, P95),
    coldStarts: sim.coldStarts,
    workers,
  };
}

function workerRun(worker: Worker, end: number): WorkerRun {
  const removedAt = worker.removedAt ?? end;
  const paid = paidHours(worker.orderedAt, removedAt);
  return {
    type: worker.type.name,
    orderedAt: worker.orderedAt,
    removedAt,
    paidHours: paid,
    busyHours: unionSeconds(worker.busy) / SECONDS_PER_HOUR,
    eur: paid * worker.type.hourlyEur,
  };
}

function unionSeconds(intervals: Interval[]): number {
  const sorted = [...intervals].sort((a, b) => a.from - b.from);
  let total = 0;
  let reached = Number.NEGATIVE_INFINITY;
  for (const { from, to } of sorted) {
    total += Math.max(0, to - Math.max(from, reached));
    reached = Math.max(reached, to);
  }
  return total;
}

/** Nearest rank; 0 when there are no values. */
function percentile(sorted: number[], rank: number): number {
  const index = Math.max(0, Math.ceil(rank * sorted.length) - 1);
  return sorted[index] ?? 0;
}

function sum(values: number[]): number {
  return values.reduce((total, value) => total + value, 0);
}
