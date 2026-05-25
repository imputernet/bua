// examples/deterministic_replay.ts
//
// Demonstrates deterministic execution and replay.
//
// First run (record):
//   bua run examples/deterministic_replay.ts \
//     --allow-net=httpbin.org \
//     --deterministic \
//     --snapshot=./replay-demo.bsnap
//
// Replay (must produce identical output):
//   bua replay ./replay-demo.bsnap --verify

import * as time from 'bua:time';
import * as random from 'bua:random';
import * as trace from 'bua:trace';
import { call } from 'bua:tools';

// Seed everything for determinism
random.seed(42);
time.freeze();

trace.info('Starting deterministic execution', {
  timestamp: time.now(),
  seed: 42,
});

async function deterministicPipeline() {
  // Time is frozen — this will always be the same value
  const startTime = time.now();
  console.log(`Execution timestamp (frozen): ${startTime}`);

  // Random values are seeded — always the same sequence
  const values = Array.from({ length: 5 }, () => random.randInt(0, 100));
  console.log(`Random sequence (seeded): ${values.join(', ')}`);
  trace.info(`Generated values: ${values}`);

  // Tool calls are recorded and replayed deterministically
  const result = await call('bua_http_get', {
    url: 'https://httpbin.org/get',
    headers: { 'X-Request-Id': `det-${values[0]}` },
  });

  const response = result as { status: number; body: string };
  console.log(`HTTP status: ${response.status}`);
  trace.info('HTTP call recorded for replay');

  // Advance virtual time
  time.advance(1000); // +1 second
  console.log(`Time after advance: ${time.now()}`);

  // Checkpoint: this execution state can be restored exactly
  trace.checkpoint('after-http-call');

  return {
    startTime,
    values,
    httpStatus: response.status,
    endTime: time.now(),
  };
}

deterministicPipeline()
  .then(result => {
    console.log('\nDeterministic result:');
    console.log(JSON.stringify(result, null, 2));
    console.log('\n✓ Run `bua replay ./replay-demo.bsnap --verify` to confirm determinism');
  })
  .catch(err => {
    console.error(`Failed: ${err.message}`);
    process.exit(1);
  });
