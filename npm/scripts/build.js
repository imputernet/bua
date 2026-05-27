#!/usr/bin/env node
// npm/scripts/build.js
//
// Run by the release workflow after all platform binaries are downloaded:
//   node npm/scripts/build.js v0.1.0
//
// Produces npm/pkg/ ready for `npm publish`.

'use strict';

const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

const version = (process.argv[2] || '0.1.0').replace(/^v/, '');
const ROOT = path.join(__dirname, '..', '..');
const PKG_DIR = path.join(__dirname, '..', 'pkg');
const BIN_DIR = path.join(ROOT, 'npm', 'binaries');

// Platform-specific optional packages to generate
const PLATFORMS = [
    { pkg: '@imputer/bua-darwin-x64', archive: `bua-v${version}-x86_64-apple-darwin.tar.gz`, bin: 'bua' },
    { pkg: '@imputer/bua-darwin-arm64', archive: `bua-v${version}-aarch64-apple-darwin.tar.gz`, bin: 'bua' },
    { pkg: '@imputer/bua-linux-x64', archive: `bua-v${version}-x86_64-linux-musl.tar.gz`, bin: 'bua' },
    { pkg: '@imputer/bua-linux-arm64', archive: `bua-v${version}-aarch64-linux-musl.tar.gz`, bin: 'bua' },
    { pkg: '@imputer/bua-win32-x64', archive: `bua-v${version}-x86_64-windows-msvc.zip`, bin: 'bua.exe' },
];

// 1. Update version in main package.json
const mainPkg = JSON.parse(fs.readFileSync(path.join(PKG_DIR, 'package.json'), 'utf8'));
mainPkg.version = version;
for (const p of PLATFORMS) {
    if (mainPkg.optionalDependencies?.[p.pkg]) {
        mainPkg.optionalDependencies[p.pkg] = version;
    }
}
fs.writeFileSync(path.join(PKG_DIR, 'package.json'), JSON.stringify(mainPkg, null, 2) + '\n');

// 2. Copy README + LICENSE into pkg/
fs.copyFileSync(path.join(ROOT, 'README.md'), path.join(PKG_DIR, 'README.md'));
fs.copyFileSync(path.join(ROOT, 'LICENSE'), path.join(PKG_DIR, 'LICENSE'));

// 3. Build each optional platform package
const platformPkgsDir = path.join(ROOT, 'npm', 'platform-pkgs');
fs.mkdirSync(platformPkgsDir, { recursive: true });

for (const { pkg, archive, bin } of PLATFORMS) {
    const archivePath = path.join(BIN_DIR, archive);
    if (!fs.existsSync(archivePath)) {
        console.warn(`  SKIP ${pkg} — archive not found: ${archivePath}`);
        continue;
    }

    const pkgDir = path.join(platformPkgsDir, pkg);
    const binDir = path.join(pkgDir, 'bin');
    fs.mkdirSync(binDir, { recursive: true });

    // Extract binary from archive
    const tmpDir = path.join(platformPkgsDir, `_tmp_${pkg.replace(/\//g, '_')}`);
    fs.mkdirSync(tmpDir, { recursive: true });

    if (archive.endsWith('.zip')) {
        execSync(`unzip -o "${archivePath}" "${bin}" -d "${tmpDir}"`, { stdio: 'pipe' });
    } else {
        execSync(`tar -xzf "${archivePath}" -C "${tmpDir}" "${bin}"`, { stdio: 'pipe' });
    }

    const extractedBin = path.join(tmpDir, bin);
    if (!fs.existsSync(extractedBin)) {
        console.warn(`  SKIP ${pkg} — binary not in archive`);
        fs.rmSync(tmpDir, { recursive: true });
        continue;
    }

    fs.copyFileSync(extractedBin, path.join(binDir, bin));
    if (!bin.endsWith('.exe')) fs.chmodSync(path.join(binDir, bin), 0o755);
    fs.rmSync(tmpDir, { recursive: true });

    // Write platform package.json
    const [, nameWithOsArch] = pkg.split('/'); // @imputer/bua-os-arch
    const parts = nameWithOsArch.split('-');
    const osName = parts[1];
    const archName = parts[2];

    const platPkg = {
        name: pkg,
        version,
        description: `Bua binary for ${osName}/${archName}`,
        os: [osName],
        cpu: [archName === 'x64' ? 'x64' : 'arm64'],
        main: `bin/${bin}`,
        files: ['bin/'],
        license: 'MIT',
        repository: mainPkg.repository,
    };
    fs.writeFileSync(path.join(pkgDir, 'package.json'), JSON.stringify(platPkg, null, 2) + '\n');
    fs.copyFileSync(path.join(ROOT, 'LICENSE'), path.join(pkgDir, 'LICENSE'));

    console.log(`  Built ${pkg}@${version}`);
}

console.log(`\nnpm package ready: ${PKG_DIR}`);
console.log('Platform packages:', platformPkgsDir);
console.log('\nNext steps in CI:');
console.log('  npm publish npm/pkg/ --access public');
for (const { pkg } of PLATFORMS) {
    console.log(`  npm publish npm/platform-pkgs/${pkg}/ --access public`);
}
