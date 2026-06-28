import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { dirsToKeep, archesToDelete, pruneVendor } from './prune-sidecar-vendor.mjs';

test('dirsToKeep: mac-universal keeps BOTH darwin arches', () => {
  assert.deepEqual(dirsToKeep('mac-universal').sort(), ['arm64-darwin', 'x64-darwin']);
});
test('dirsToKeep: linux/win keep exactly one (independent literals)', () => {
  assert.deepEqual(dirsToKeep('linux-x64'), ['x64-linux']);
  assert.deepEqual(dirsToKeep('win-x64'), ['x64-win32']);
});
test('dirsToKeep: unknown target throws', () => {
  assert.throws(() => dirsToKeep('solaris-sparc'), /unknown prune target/);
});
test('archesToDelete: mac-universal deletes exactly the 3 non-darwin', () => {
  assert.deepEqual(archesToDelete('mac-universal').sort(), ['arm64-linux', 'x64-linux', 'x64-win32']);
});
test('archesToDelete: every target deletes 1..4, never all five', () => {
  for (const t of ['mac-universal', 'linux-x64', 'win-x64']) {
    const d = archesToDelete(t);
    assert.ok(d.length >= 1 && d.length < 5, `${t} deletes ${d.length}`);
  }
});

function fakeVendor() {
  const base = mkdtempSync(join(tmpdir(), 'uaw-prune-'));
  const pkg = join(base, 'node_modules', '@anthropic-ai', 'claude-agent-sdk');
  const vendor = join(pkg, 'vendor');
  for (const arch of ['arm64-darwin', 'x64-darwin', 'x64-linux', 'arm64-linux', 'x64-win32']) {
    mkdirSync(join(vendor, 'ripgrep', arch), { recursive: true });
    writeFileSync(join(vendor, 'ripgrep', arch, arch.includes('win32') ? 'rg.exe' : 'rg'), '');
    writeFileSync(join(vendor, 'ripgrep', arch, 'ripgrep.node'), '');
  }
  writeFileSync(join(vendor, 'ripgrep', 'COPYING'), 'license');
  mkdirSync(join(vendor, 'claude-code-jetbrains-plugin', 'lib'), { recursive: true });
  writeFileSync(join(vendor, 'claude-code-jetbrains-plugin', 'lib', 'x.jar'), '');
  writeFileSync(join(pkg, 'sdk.mjs'), '');
  return { base, vendor };
}

test('pruneVendor mac-universal: keeps both darwin + COPYING, drops other arches + jetbrains', () => {
  const { base, vendor } = fakeVendor();
  try {
    pruneVendor(vendor, 'mac-universal');
    assert.ok(existsSync(join(vendor, 'ripgrep', 'arm64-darwin', 'rg')));
    assert.ok(existsSync(join(vendor, 'ripgrep', 'x64-darwin', 'ripgrep.node')));
    assert.ok(existsSync(join(vendor, 'ripgrep', 'COPYING')), 'unknown entry COPYING must survive');
    assert.ok(!existsSync(join(vendor, 'ripgrep', 'x64-linux')));
    assert.ok(!existsSync(join(vendor, 'ripgrep', 'arm64-linux')));
    assert.ok(!existsSync(join(vendor, 'ripgrep', 'x64-win32')));
    assert.ok(!existsSync(join(vendor, 'claude-code-jetbrains-plugin')));
  } finally {
    rmSync(base, { recursive: true, force: true });
  }
});
test('pruneVendor: postcondition throws if a kept arch is missing', () => {
  const { base, vendor } = fakeVendor();
  try {
    rmSync(join(vendor, 'ripgrep', 'x64-darwin'), { recursive: true, force: true });
    assert.throws(() => pruneVendor(vendor, 'mac-universal'), /postcondition failed/);
  } finally {
    rmSync(base, { recursive: true, force: true });
  }
});
test('pruneVendor: refuses a non-vendor root', () => {
  const base = mkdtempSync(join(tmpdir(), 'uaw-notvendor-'));
  try {
    assert.throws(() => pruneVendor(base, 'linux-x64'), /not a sidecar vendor root/);
  } finally {
    rmSync(base, { recursive: true, force: true });
  }
});
test('pruneVendor win-x64: keeps x64-win32 (rg.exe) and passes the postcondition', () => {
  const { base, vendor } = fakeVendor();
  try {
    pruneVendor(vendor, 'win-x64'); // must not throw — exercises the rg.exe postcondition
    assert.ok(existsSync(join(vendor, 'ripgrep', 'x64-win32', 'rg.exe')));
    assert.ok(existsSync(join(vendor, 'ripgrep', 'x64-win32', 'ripgrep.node')));
    assert.ok(!existsSync(join(vendor, 'ripgrep', 'x64-darwin')));
  } finally {
    rmSync(base, { recursive: true, force: true });
  }
});
test('pruneVendor: postcondition throws if sdk.mjs is missing (half-installed sidecar)', () => {
  const { base, vendor } = fakeVendor();
  try {
    rmSync(join(vendor, '..', 'sdk.mjs'), { force: true });
    assert.throws(() => pruneVendor(vendor, 'linux-x64'), /postcondition failed/);
  } finally {
    rmSync(base, { recursive: true, force: true });
  }
});
