// examples/hello_agent.ts
// Run with: bua run examples/hello_agent.ts --allow-net=api.openai.com

interface ToolResult {
  output: unknown;
  error?: string;
}

// Bua runtime globals injected by the runtime
declare const Bua: {
  tools: {
    call(name: string, args: Record<string, unknown>): Promise<ToolResult>;
    list(): Array<{ name: string; description: string }>;
  };
  agent: {
    id: string;
    spawn(config: {
      entrypoint: string;
      capabilities?: string[];
    }): Promise<string>;
  };
  trace: {
    log(level: "info" | "warn" | "error", msg: string): void;
  };
  env: {
    get(key: string): string | undefined;
  };
};

async function main(): Promise<void> {
  Bua.trace.log("info", "hello_agent starting");

  // List available tools
  const tools = Bua.tools.list();
  console.log(`Available tools: ${tools.map(t => t.name).join(", ")}`);

  // Read a file using the built-in tool
  const result = await Bua.tools.call("bua_read_file", {
    path: "./examples/hello_agent.ts",
  });

  if (result.error) {
    Bua.trace.log("error", `File read failed: ${result.error}`);
    throw new Error(result.error);
  }

  const content = result.output as { content: string; bytes: number };
  Bua.trace.log("info", `Read ${content.bytes} bytes from self`);
  console.log(`Self-read: ${content.bytes} bytes`);

  // HTTP fetch example (requires --allow-net)
  try {
    const http = await Bua.tools.call("bua_http_get", {
      url: "https://httpbin.org/json",
    });
    const resp = http.output as { status: number; body: string };
    console.log(`HTTP status: ${resp.status}`);
  } catch (e) {
    console.log("Network call skipped (no --allow-net flag)");
  }

  Bua.trace.log("info", "hello_agent complete");
}

main().catch((e) => {
  console.error("Agent error:", e);
  process.exit(1);
});
