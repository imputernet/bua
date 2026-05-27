#!/usr/bin/env node
// npm/scripts/build.js
//
// Run by the release workflow after all platform binaries are downloaded:
//   node npm/scripts/build.js v0.1.0
//
// Produces npm/pkg/ ready for `npm publish`.

'use strict';

const fs   = require('fs');
const path = require('path');
const { execSync } = require('child_process');

const version = (process.argv[2] || '0.1.0').replace(/^v/, '');
const ROOT     = path.join(__dirname, '..', '..');
const PKG_DIR  = path.join(__dirname, '..', 'pkg');
const BIN_DIR  = path.join(ROOT, 'npm', 'binaries');

// Platform-specific targets to bundle into the single package
const PLATFORMS = [
  { target: 'darwin-x64',    archive: `bua-v${version}-x86_64-apple-darwin.tar.gz`,     bin: 'bua'     },
  { target: 'darwin-arm64',  archive: `bua-v${version}-aarch64-apple-darwin.tar.gz`,    bin: 'bua'     },
  { target: 'linux-x64',     archive: `bua-v${version}-x86_64-linux-musl.tar.gz`,       bin: 'bua'     },
  { target: 'linux-arm64',   archive: `bua-v${version}-aarch64-linux-musl.tar.gz`,      bin: 'bua'     },
  { target: 'linux-x64-gnu', archive: `bua-v${version}-x86_64-unknown-linux-gnu.tar.gz`, bin: 'bua'     },
  { target: 'win32-x64',     archive: `bua-v${version}-x86_64-windows-msvc.zip`,        bin: 'bua.exe' },
];

console.log(`Building bua.js@${version} (bundling all binaries)...`);

// 1. Update version in package.json
const pkgPath = path.join(PKG_DIR, 'package.json');
const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
pkg.version = version;
delete pkg.optionalDependencies;
if (pkg.scripts) delete pkg.scripts.postinstall;

// Ensure scripts/ is not in files array if we are deleting it
if (pkg.files) {
  pkg.files = pkg.files.filter(f => f !== 'scripts/' && f !== 'scripts');
}

fs.writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + '\n');

// 2. Copy README + LICENSE into pkg/
fs.copyFileSync(path.join(ROOT, 'README.md'), path.join(PKG_DIR, 'README.md'));
fs.copyFileSync(path.join(ROOT, 'LICENSE'),   path.join(PKG_DIR, 'LICENSE'));

// 3. Extract each binary into pkg/bin/
const pkgBinDir = path.join(PKG_DIR, 'bin');
fs.mkdirSync(pkgBinDir, { recursive: true });

for (const { target, archive, bin } of PLATFORMS) {
  const archivePath = path.join(BIN_DIR, archive);
  if (!fs.existsSync(archivePath)) {
    console.warn(`  SKIP ${target} — archive not found: ${archivePath}`);
    continue;
  }

  const destBinName = target.startsWith('win32') ? `bua-${target}.exe` : `bua-${target}`;
  const destBinPath = path.join(pkgBinDir, destBinName);

  // Extract binary from archive
  const tmpDir = path.join(ROOT, 'npm', `_tmp_${target}`);
  if (fs.existsSync(tmpDir)) fs.rmSync(tmpDir, { recursive: true });
  fs.mkdirSync(tmpDir, { recursive: true });

  try {
    if (archive.endsWith('.zip')) {
      execSync(`unzip -o "${archivePath}" "${bin}" -d "${tmpDir}"`, { stdio: 'pipe' });
    } else {
      execSync(`tar -xzf "${archivePath}" -C "${tmpDir}" "${bin}"`, { stdio: 'pipe' });
    }
  } catch (err) {
    console.error(`  ERROR extracting ${target}: ${err.message}`);
    continue;
  }

  const extractedBin = path.join(tmpDir, bin);
  if (!fs.existsSync(extractedBin)) {
    console.warn(`  SKIP ${target} — binary not in archive`);
    fs.rmSync(tmpDir, { recursive: true });
    continue;
  }

  fs.copyFileSync(extractedBin, destBinPath);
  if (!bin.endsWith('.exe')) fs.chmodSync(destBinPath, 0o755);
  fs.rmSync(tmpDir, { recursive: true });

  console.log(`  Bundled ${destBinName}`);
}

console.log(`\nnpm package ready in ${PKG_DIR}`);
console.log('Next steps in CI:');
console.log('  npm publish npm/pkg/ --access public');
