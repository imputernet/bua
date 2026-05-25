// examples/multi_agent.ts
// Demonstrates spawning child agents and collecting results.
// Run: bua run examples/multi_agent.ts --allow-fs=./examples --allow-net=*

declare const Bua: {
  tools: {
    call(name: string, args: Record<string, unknown>): Promise<{ output: unknown; error?: string }>;
  };
  agent: {
    id: string;
    spawn(config: {
      entrypoint: string;
      allowFs?: string[];
      allowNet?: string[];
      timeout?: number;
    }): Promise<{ id: string; result: unknown; error?: string }>;
  };
  trace: {
    log(level: "info" | "warn" | "error" | "debug", msg: string, meta?: unknown): void;
  };
};

interface WorkItem {
  url: string;
  label: string;
}

// Supervisor agent: fans out work to N child agents in parallel.
async function supervisor(items: WorkItem[]): Promise<void> {
  Bua.trace.log("info", `Supervisor spawning ${items.length} workers`);

  const tasks = items.map((item) =>
    Bua.agent.spawn({
      entrypoint: "./examples/worker_agent.ts",
      allowNet: [new URL(item.url).hostname],
      timeout: 30_000,
    }).then((result) => ({ item, result }))
  );

  const results = await Promise.allSettled(tasks);

  let succeeded = 0;
  let failed = 0;

  for (const settled of results) {
    if (settled.status === "fulfilled") {
      const { item, result } = settled.value;
      if (result.error) {
        Bua.trace.log("warn", `Worker failed for ${item.label}: ${result.error}`);
        failed++;
      } else {
        Bua.trace.log("info", `Worker succeeded for ${item.label}`);
        succeeded++;
      }
    } else {
      Bua.trace.log("error", `Worker spawn failed: ${settled.reason}`);
      failed++;
    }
  }

  console.log(`\nResults: ${succeeded} succeeded, ${failed} failed`);
}

const workload: WorkItem[] = [
  { url: "https://httpbin.org/get", label: "httpbin" },
  { url: "https://httpbin.org/json", label: "httpbin-json" },
  { url: "https://httpbin.org/uuid", label: "httpbin-uuid" },
];

supervisor(workload).catch((e) => {
  console.error("Supervisor error:", e);
  process.exit(1);
});
