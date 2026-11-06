import { describe, expect, test } from "bun:test";
import {
  type Job,
  type JobClass,
  P0,
  P1,
  P2,
  P3,
  P4,
  P5,
  POLICIES,
  paidHours,
  replay,
  workerTypeOrder,
} from "./cluster.ts";

type JobSpec = {
  arrival: number;
  runSeconds: number;
  cpuMilli?: number;
  memoryMiB?: number;
  class?: JobClass;
};

function job(spec: JobSpec): Job {
  return {
    id: `job-${spec.arrival}`,
    arrival: spec.arrival,
    runSeconds: spec.runSeconds,
    request: { cpuMilli: spec.cpuMilli ?? 500, memoryMiB: spec.memoryMiB ?? 2048 },
    command: "build",
    class: spec.class ?? "build",
  };
}

function burst(count: number, spec: JobSpec): Job[] {
  return Array.from({ length: count }, () => job(spec));
}

describe("cold start", () => {
  test("cold start on an empty cluster waits for a new worker", () => {
    const run = replay([job({ arrival: 0, runSeconds: 900 })], P0);

    expect(run.coldStarts).toBe(1);
    expect(run.waitP50).toBe(100);
    expect(run.waitP95).toBe(100);
    expect(run.workers.map((worker) => worker.type)).toEqual(["cx43"]);
    expect(run.paidHours).toBe(2);
    expect(run.busyHours).toBe(0.25);
    expect(run.wasteHours).toBe(1.75);
    expect(run.eur).toBeCloseTo(2 * 0.0256);
  });
});

describe("warm worker", () => {
  test("warm worker starts the second job in 20 s", () => {
    const run = replay(
      [job({ arrival: 0, runSeconds: 900 }), job({ arrival: 600, runSeconds: 900 })],
      P0,
    );

    expect(run.coldStarts).toBe(1);
    expect(run.waitP50).toBe(20);
    expect(run.waitP95).toBe(100);
    expect(run.workers).toHaveLength(1);
  });
});

describe("hour end", () => {
  test("P1 removes the empty worker at minute 50 of its paid hour", () => {
    const jobs = [job({ arrival: 0, runSeconds: 900 })];
    const p0 = replay(jobs, P0);
    const p1 = replay(jobs, P1);

    expect(p0.paidHours).toBe(2);
    expect(p1.paidHours).toBe(1);
    expect(p0.wasteHours - p1.wasteHours).toBe(1);
    expect(p1.workers[0]?.removedAt).toBe(3000);
  });

  test("a worker that gets empty between minute 50 and 55 goes at once", () => {
    const run = replay([job({ arrival: 0, runSeconds: 3000 })], P1);

    expect(run.workers[0]?.removedAt).toBe(3100);
    expect(run.paidHours).toBe(1);
  });
});

describe("packing", () => {
  // Eight build Jobs bring up two cx43 (seven fit on one); two short Jobs come later.
  const jobs = [
    ...burst(8, { arrival: 0, runSeconds: 600 }),
    ...burst(2, { arrival: 2700, runSeconds: 600 }),
  ];

  test("P2 packs pods so a second worker can go", () => {
    const spread = replay(jobs, P1);
    const pack = replay(jobs, P2);

    expect(spread.workers.map((worker) => worker.paidHours)).toEqual([2, 2]);
    expect(spread.workers.map((worker) => worker.busyHours)).toEqual([
      (600 + 600) / 3600,
      (600 + 600) / 3600,
    ]);
    expect(pack.workers.map((worker) => worker.paidHours)).toEqual([2, 1]);
    expect(pack.workers[1]?.removedAt).toBe(3000);
    expect(pack.paidHours).toBeLessThan(spread.paidHours);
  });
});

describe("warm slot", () => {
  test("P5 warm slot removes the cold start", () => {
    const jobs = [job({ arrival: 0, runSeconds: 900 })];
    const p4 = replay(jobs, P4);
    const p5 = replay(jobs, P5);

    expect(p4.coldStarts).toBe(1);
    expect(p4.waitP50).toBe(100);
    expect(p5.coldStarts).toBe(0);
    expect(p5.waitP50).toBe(20);
    expect(p5.paidHours).toBeGreaterThan(p4.paidHours);
  });

  test("the placeholder keeps its worker and adds no busy time", () => {
    const run = replay([job({ arrival: 0, runSeconds: 900 })], P5);

    expect(run.workers).toHaveLength(1);
    expect(run.workers[0]?.orderedAt).toBe(-3600);
    expect(run.busyHours).toBe(0.25);
    expect(run.paidHours).toBe(2);
  });

  test("a Job that fits only in the warm slot takes it and the placeholder moves", () => {
    // A cx43 has 7750m for boxes: the 500m slot and 14 build Jobs fit, the 15th takes the slot.
    const jobs = burst(15, { arrival: 0, runSeconds: 900 });
    const run = replay(jobs, P5);

    expect(run.coldStarts).toBe(0);
    expect(run.workers).toHaveLength(2);
    expect(run.workers[1]?.orderedAt).toBe(0);
  });
});

describe("worker fit", () => {
  test("a cx43 holds seven build boxes by memory", () => {
    const run = replay(burst(8, { arrival: 0, runSeconds: 600 }), P0);

    expect(run.workers).toHaveLength(2);
    expect(run.coldStarts).toBe(8);
  });

  test("a cx43 holds seven boxes of one core by CPU", () => {
    const run = replay(
      burst(15, { arrival: 0, runSeconds: 600, cpuMilli: 1000, memoryMiB: 100 }),
      P0,
    );

    expect(run.workers).toHaveLength(3);
  });

  test("a Job waits for a worker that is booting when it fits there", () => {
    const run = replay(
      [job({ arrival: 0, runSeconds: 600 }), job({ arrival: 30, runSeconds: 600 })],
      P0,
    );

    expect(run.workers).toHaveLength(1);
    expect(run.coldStarts).toBe(2);
    expect(run.waitP50).toBe(70);
  });
});

describe("policies", () => {
  test("P3 asks the measured memory of the class", () => {
    const jobs = burst(8, { arrival: 0, runSeconds: 600 });

    expect(replay(jobs, P2).workers).toHaveLength(2);
    expect(replay(jobs, P3).workers).toHaveLength(1);
  });

  test("P4 orders the worker types by price per GB-hour", () => {
    expect(workerTypeOrder(P0.typeOrder).map((type) => type.name)).toEqual([
      "cx43",
      "cpx32",
      "cpx42",
    ]);
    expect(workerTypeOrder(P4.typeOrder).map((type) => type.name)).toEqual([
      "cx43",
      "cpx42",
      "cpx32",
    ]);
  });

  test("each policy is the one before with one change", () => {
    const knobs = ["emptyRule", "placement", "request", "typeOrder", "warmSlots"] as const;
    const changed = POLICIES.slice(1).map((policy, index) =>
      knobs.filter((knob) => policy[knob] !== POLICIES[index]?.[knob]),
    );

    expect(POLICIES.map((policy) => policy.name)).toEqual(["P0", "P1", "P2", "P3", "P4", "P5"]);
    expect(changed).toEqual([
      ["emptyRule"],
      ["placement"],
      ["request"],
      ["typeOrder"],
      ["warmSlots"],
    ]);
  });
});

describe("paid hours", () => {
  test("every started hour from the order to the removal counts", () => {
    expect(paidHours(0, 0)).toBe(1);
    expect(paidHours(0, 3600)).toBe(1);
    expect(paidHours(0, 3601)).toBe(2);
    expect(paidHours(100, 4000)).toBe(2);
  });

  test("no Jobs cost nothing", () => {
    expect(replay([], P5)).toEqual({
      eur: 0,
      paidHours: 0,
      busyHours: 0,
      wasteHours: 0,
      waitP50: 0,
      waitP95: 0,
      coldStarts: 0,
      workers: [],
    });
  });
});
