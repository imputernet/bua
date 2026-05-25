// examples/snapshot_restore.ts
//
// Demonstrates checkpoint, snapshot, and restore.
//
// Run full execution:
//   bua run examples/snapshot_restore.ts \
//     --allow-fs=./output --allow-net=httpbin.org \
//     --snapshot=./output/demo.bsnap
//
// Restore from snapshot:
//   bua replay ./output/demo.bsnap

import * as trace from 'bua:trace';
import * as fs from 'bua:fs';
import * as memory from 'bua:memory';
import { call } from 'bua:tools';

interface PipelineState {
  step: number;
  results: unknown[];
  startedAt: number;
}

async function runPipeline() {
  const state: PipelineState = {
    step: 0,
    results: [],
    startedAt: Date.now(),
  };

  // Persist state to agent memory (included in MemoryStratum)
  await memory.put('pipeline_state', state);
  trace.info('Pipeline state persisted to memory');

  // Step 1
  trace.info('Step 1: Fetch initial data');
  const r1 = await call('bua_http_get', { url: 'https://httpbin.org/get' });
  state.results.push(r1);
  state.step = 1;
  await memory.put('pipeline_state', state);

  // Checkpoint after step 1 (snapshot includes memory + trace + tool log)
  trace.checkpoint('after-step-1');
  trace.info('Checkpoint written: after-step-1');

  // Step 2
  trace.info('Step 2: Fetch JSON data');
  const r2 = await call('bua_http_get', { url: 'https://httpbin.org/json' });
  state.results.push(r2);
  state.step = 2;
  await memory.put('pipeline_state', state);

  trace.checkpoint('after-step-2');

  // Step 3: Write results
  trace.info('Step 3: Write output');
  const output = {
    completedAt: Date.now(),
    durationMs: Date.now() - state.startedAt,
    stepCount: state.step,
    resultCount: state.results.length,
  };

  await fs.writeFile('./output/pipeline-result.json', JSON.stringify(output, null, 2));
  trace.info('Pipeline complete', output);

  console.log('Pipeline finished successfully.');
  console.log(`Duration: ${output.durationMs}ms`);
  console.log('Snapshot written to ./output/demo.bsnap');
  console.log('Restore with: bua replay ./output/demo.bsnap');
}

runPipeline().catch(err => {
  trace.error(`Pipeline failed: ${err.message}`);
  process.exit(1);
});
