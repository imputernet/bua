#!/usr/bin/env node
// npm/pkg/bin/bua
//
// Thin Node.js shim that locates and exec's the real Bua binary.
// This file is the `bin.bua` entry in package.json.

'use strict';

const { execFileSync } = require('child_process');
const path = require('path');
const fs   = require('fs');
const os   = require('os');

const binName = os.platform() === 'win32' ? 'bua.exe' : 'bua';
// The native binary is placed in the same bin/ directory as this shim
const binPath = path.join(__dirname, binName);

// Ensure we are not trying to execute ourselves
if (binPath === __filename) {
  console.error('[bua.js] Error: shim tried to execute itself.');
  process.exit(1);
}

if (!fs.existsSync(binPath)) {
  console.error(
    'Bua binary not found. Run `npm install bua.js` to trigger postinstall,\n' +
    'or build from source: https://github.com/imputernet/bua'
  );
  process.exit(1);
}

try {
  execFileSync(binPath, process.argv.slice(2), {
    stdio: 'inherit',
    env: process.env,
  });
} catch (err) {
  if (err.status != null) {
    process.exit(err.status);
  }
  throw err;
}
