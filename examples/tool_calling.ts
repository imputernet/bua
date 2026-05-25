// examples/tool_calling.ts
//
// Demonstrates the full tool calling interface.
// Run: bua run examples/tool_calling.ts \
//        --allow-fs=./examples --allow-net=httpbin.org

import { call, list, has } from 'bua:tools';
import * as trace from 'bua:trace';

async function main() {
  console.log('=== Bua Tool Calling Demo ===\n');

  // 1. List available tools
  const tools = list();
  console.log(`Available tools (${tools.length}):`);
  for (const tool of tools) {
    console.log(`  • ${tool.name}: ${tool.description}`);
  }
  console.log();

  // 2. Check tool existence
  if (has('bua_read_file')) {
    console.log('✓ bua_read_file is registered\n');
  }

  // 3. Read a file
  trace.info('Reading this script');
  try {
    const content = await call('bua_read_file', {
      path: './examples/tool_calling.ts',
    }) as { content: string; bytes: number };
    console.log(`✓ File read: ${content.bytes} bytes`);
  } catch (err: any) {
    console.log(`  File read: ${err.message}`);
  }

  // 4. HTTP GET
  trace.info('HTTP GET demo');
  try {
    const result = await call('bua_http_get', {
      url: 'https://httpbin.org/json',
    }) as { status: number; body: string };

    const data = JSON.parse(result.body);
    console.log(`✓ HTTP GET ${result.status}`);
    console.log(`  slideshow title: ${data?.slideshow?.title ?? '(not found)'}`);
  } catch (err: any) {
    console.log(`  HTTP GET: ${err.message} (need --allow-net=httpbin.org)`);
  }

  console.log('\n✓ Tool calling complete');
  console.log('  Every call is recorded in the execution trace.');
  console.log('  In --deterministic mode, results are replayed from the snapshot.');
}

main().catch(err => {
  trace.error(`Demo failed: ${err.message}`);
  process.exit(1);
});
