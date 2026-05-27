#!/usr/bin/env node
// npm/pkg/bin/bua
//
// Thin Node.js shim that locates and exec's the correct bundled Bua binary.
// This file is the `bin.bua` entry in package.json.

'use strict';

const { execFileSync } = require('child_process');
const path = require('path');
const fs   = require('fs');
const os   = require('os');

function getBundledBinary() {
  const platform = os.platform(); // 'darwin' | 'linux' | 'win32'
  const arch     = os.arch();     // 'x64' | 'arm64'

  let target = '';
  if (platform === 'darwin') {
    target = arch === 'arm64' ? 'darwin-arm64' : 'darwin-x64';
  } else if (platform === 'linux') {
    // Prefer musl-linked binary for max compatibility, but we bundle both.
    // For now, default to the musl one as it's static.
    target = arch === 'arm64' ? 'linux-arm64' : 'linux-x64';
  } else if (platform === 'win32') {
    target = 'win32-x64';
  }

  if (!target) {
    console.error(`[bua.js] Error: Unsupported platform/architecture: ${platform}/${arch}`);
    process.exit(1);
  }

  const binName = platform === 'win32' ? `bua-${target}.exe` : `bua-${target}`;
  return path.join(__dirname, binName);
}

const binPath = getBundledBinary();

// Ensure we are not trying to execute ourselves
if (binPath === __filename) {
  console.error('[bua.js] Error: shim tried to execute itself.');
  process.exit(1);
}

if (!fs.existsSync(binPath)) {
  console.error(
    `Bua binary not found at: ${binPath}\n` +
    `Your platform (${os.platform()}/${os.arch()}) might not be supported by this package version.`
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
