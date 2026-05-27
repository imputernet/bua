#!/usr/bin/env node
// npm/pkg/scripts/install.js
//
// Postinstall: resolves the correct platform-specific binary package and
// symlinks (or copies) it to npm/pkg/bin/bua so `npx bua` works.

'use strict';

const fs   = require('fs');
const path = require('path');
const os   = require('os');

const PACKAGE_NAME = 'bua.js';
const BIN_DIR = path.join(__dirname, '..', 'bin');

function getPlatformPackage() {
  const platform = os.platform(); // 'darwin' | 'linux' | 'win32'
  const arch     = os.arch();     // 'x64' | 'arm64'

  const packageMap = {
    'darwin-x64':   `${PACKAGE_NAME}-darwin-x64`,
    'darwin-arm64': `${PACKAGE_NAME}-darwin-arm64`,
    'linux-x64':    `${PACKAGE_NAME}-linux-x64`,
    'linux-arm64':  `${PACKAGE_NAME}-linux-arm64`,
    'win32-x64':    `${PACKAGE_NAME}-win32-x64`,
  };

  const key = `${platform}-${arch}`;
  const pkg = packageMap[key];
  if (!pkg) {
    throw new Error(
      `Bua does not have a pre-built binary for ${platform}/${arch}.\n` +
      `Please build from source: https://github.com/imputernet/bua`
    );
  }
  return pkg;
}

function findBinary(pkgName) {
  try {
    // Resolve the optional dep package directory
    const pkgDir = path.dirname(require.resolve(`${pkgName}/package.json`));
    const binName = process.platform === 'win32' ? 'bua.exe' : 'bua';
    const binPath = path.join(pkgDir, 'bin', binName);
    if (fs.existsSync(binPath)) return binPath;
  } catch {
    // Optional dep not installed (e.g. wrong platform package forced install)
  }
  return null;
}

function main() {
  if (!fs.existsSync(BIN_DIR)) {
    fs.mkdirSync(BIN_DIR, { recursive: true });
  }

  const pkgName = getPlatformPackage();
  const srcBin  = findBinary(pkgName);

  const binName = process.platform === 'win32' ? 'bua.exe' : 'bua';
  const destBin = path.join(BIN_DIR, binName);

  if (!srcBin) {
    // Write a stub that prints a helpful error
    const stub = process.platform === 'win32'
      ? `@echo off\necho Bua: binary not found. Please reinstall bua.js or build from source.\nexit /b 1\n`
      : `#!/bin/sh\necho "Bua: binary not found. Please reinstall bua.js or build from source." >&2\nexit 1\n`;
    fs.writeFileSync(destBin, stub);
    if (process.platform !== 'win32') fs.chmodSync(destBin, 0o755);
    console.warn(`[bua.js] Warning: could not find binary for ${pkgName}`);
    return;
  }

  // Copy (not symlink) for Windows compatibility
  fs.copyFileSync(srcBin, destBin);
  if (process.platform !== 'win32') fs.chmodSync(destBin, 0o755);

  console.log(`[bua.js] Installed ${pkgName} → ${destBin}`);
}

try {
  main();
} catch (err) {
  console.error(`[bua.js] Install failed: ${err.message}`);
  // Don't hard-fail postinstall — allow `npm install` to succeed
  // even if the binary isn't found. The binary check happens at runtime.
  process.exit(0);
}
