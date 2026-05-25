// examples/capability_sandbox.ts
//
// Demonstrates capability enforcement and isolation.
//
// Run: bua run examples/capability_sandbox.ts --allow-fs=./examples
// (Intentionally NOT granting --allow-net to show denial)

import * as trace from 'bua:trace';
import * as fs from 'bua:fs';
import { call } from 'bua:tools';

trace.info('Capability sandbox demo starting');

// ✓ This should succeed (we have --allow-fs=./examples)
async function testAllowedOp() {
  try {
    const content = await fs.readFile('./examples/capability_sandbox.ts');
    console.log(`✓ File read succeeded: ${content.length} bytes`);
    trace.info('Allowed fs read succeeded');
  } catch (err: any) {
    console.error(`✗ File read failed unexpectedly: ${err.message}`);
  }
}

// ✗ This should fail with PermissionError (no --allow-net)
async function testDeniedOp() {
  try {
    const result = await call('bua_http_get', {
      url: 'https://api.example.com/data',
    });
    console.error('✗ Network call succeeded — this should have been denied!');
  } catch (err: any) {
    if (err.name === 'PermissionError') {
      console.log(`✓ Network access correctly denied: ${err.message}`);
      trace.info('Capability denial working correctly');
    } else {
      console.log(`✓ Network blocked (${err.name}): ${err.message}`);
    }
  }
}

// ✗ This should fail — path outside granted root
async function testPathEscape() {
  try {
    const content = await fs.readFile('/etc/passwd');
    console.error('✗ Path escape succeeded — this is a security bug!');
  } catch (err: any) {
    console.log(`✓ Path escape correctly denied: ${err.message}`);
    trace.info('Path escape blocked');
  }
}

async function main() {
  console.log('=== Capability Sandbox Demo ===\n');

  await testAllowedOp();
  await testDeniedOp();
  await testPathEscape();

  console.log('\n=== Summary ===');
  console.log('Bua enforces capabilities at runtime.');
  console.log('Denied operations throw typed PermissionError.');
  console.log('Every check is logged to the execution trace.');
}

main().catch(err => {
  console.error(`Unexpected error: ${err.message}`);
  process.exit(1);
});
