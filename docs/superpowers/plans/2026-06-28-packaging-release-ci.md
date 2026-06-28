# Packaging Slice B — Release CI — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A pushed `v*` tag builds unsigned installers for 3 OSes (mac universal DMG, Windows NSIS, Linux AppImage+deb) as a draft GitHub Release, with a per-OS-pruned sidecar, SHA-256 checksums, and build-provenance attestation.

**Architecture:** A Node prune script (unit-tested, CI-only, fail-closed) shrinks the bundled sidecar's vendor tree per target; a 4-job `release.yml` (`test-prune` → `create-release` → `build` matrix → `finalize`) runs it, builds via `tauri-action`, and publishes a draft; a small Rust change makes a missing Node a clear runtime error.

**Tech Stack:** GitHub Actions, `tauri-apps/tauri-action`, Node `node --test` (built-in, zero deps), Rust (`std::io::ErrorKind`).

---

## File Structure
- `scripts/prune-sidecar-vendor.mjs` — the prune: pure core (`dirsToKeep`/`archesToDelete`) + side-effecting `pruneVendor` + a CLI with a CI-scope guard. (Sibling to the existing `scripts/run-e2e.sh`.)
- `scripts/prune-sidecar-vendor.test.mjs` — `node --test` unit + tmpdir-fixture tests (no framework).
- `src-tauri/src/services/agent/sdk.rs` — a `node_spawn_error` helper mapping ENOENT → a clear Node-prereq message at both spawn sites; update one test.
- `.github/workflows/release.yml` — the 4-job pipeline.
- `README.md` — one line: the SDK agent needs Node ≥18 on PATH.

**Task ordering:** Task 1 (prune script) is foundational — the workflow calls it. Task 2 (Node error) is independent. Task 3 writes the workflow using the script. Task 4 hardens the workflow's actions to SHAs. Tasks 1–2 are TDD; 3–4 are config validated by YAML-parse + structural checklist + a post-merge dry-run.

---

### Task 1: The sidecar vendor prune script + tests

**Files:**
- Create: `scripts/prune-sidecar-vendor.mjs`
- Create: `scripts/prune-sidecar-vendor.test.mjs`

- [ ] **Step 1: Write the failing tests** (`scripts/prune-sidecar-vendor.test.mjs`)

```js
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
    writeFileSync(join(vendor, 'ripgrep', arch, 'rg'), '');
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
```

- [ ] **Step 2: Run the tests — expect FAIL (module missing)**

Run: `node --test scripts/prune-sidecar-vendor.test.mjs`
Expected: FAIL — `Cannot find module './prune-sidecar-vendor.mjs'`.

- [ ] **Step 3: Implement the script** (`scripts/prune-sidecar-vendor.mjs`)

```js
import { realpathSync, existsSync, rmSync } from 'node:fs';
import { join, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const SIDECAR_PKG = 'sidecar/claude-agent-sdk/node_modules/@anthropic-ai/claude-agent-sdk';
const KNOWN_ARCHES = ['arm64-darwin', 'x64-darwin', 'x64-linux', 'arm64-linux', 'x64-win32'];

// Pure core — which ripgrep arch dirs to keep for a build target.
export function dirsToKeep(target) {
  if (target === 'mac-universal') return ['arm64-darwin', 'x64-darwin']; // runs as either arch
  if (target === 'linux-x64') return ['x64-linux'];
  if (target === 'win-x64') return ['x64-win32'];
  throw new Error(`unknown prune target: ${target}`);
}
// Delete only KNOWN arches not kept — leaves COPYING + any unrecognized future dir untouched.
export function archesToDelete(target) {
  const keep = dirsToKeep(target);
  return KNOWN_ARCHES.filter((a) => !keep.includes(a));
}

// Delete non-target ripgrep arches + the inert jetbrains plugin under `vendor`, then assert
// the postcondition. `vendor` must resolve to a `.../@anthropic-ai/claude-agent-sdk/vendor`.
export function pruneVendor(vendor, target) {
  const root = realpathSync(vendor);
  const expected = join('@anthropic-ai', 'claude-agent-sdk', 'vendor');
  if (!root.endsWith(expected)) {
    throw new Error(`refusing to prune: not a sidecar vendor root: ${root}`);
  }
  const within = (rel) => {
    const p = join(root, rel);
    if (p !== root && !p.startsWith(root + sep)) {
      throw new Error(`refusing to delete outside vendor root: ${p}`);
    }
    return p;
  };
  for (const arch of archesToDelete(target)) {
    rmSync(within(join('ripgrep', arch)), { recursive: true, force: true });
  }
  rmSync(within('claude-code-jetbrains-plugin'), { recursive: true, force: true });

  // Postcondition: kept arches intact, SDK entry present, jetbrains gone.
  for (const arch of dirsToKeep(target)) {
    for (const f of ['rg', 'ripgrep.node']) {
      if (!existsSync(join(root, 'ripgrep', arch, f))) {
        throw new Error(`prune postcondition failed: missing ripgrep/${arch}/${f}`);
      }
    }
  }
  if (!existsSync(join(root, '..', 'sdk.mjs'))) {
    throw new Error('prune postcondition failed: missing sdk.mjs (sidecar not installed?)');
  }
  if (existsSync(join(root, 'claude-code-jetbrains-plugin'))) {
    throw new Error('prune postcondition failed: jetbrains plugin still present');
  }
}

function argFor(argv, flag) {
  const i = argv.indexOf(flag);
  if (i === -1 || i + 1 >= argv.length) throw new Error(`missing ${flag} <value>`);
  return argv[i + 1];
}

function main(argv) {
  const target = argFor(argv, '--target');
  const force = argv.includes('--force');
  // CI-only: the prune mutates the tree `pnpm dev` resolves the sidecar from (mod.rs dev branch).
  if (!process.env.CI && !force) {
    throw new Error(
      'refusing to prune outside CI (would mutate the dev tree the SDK agent runs from in `pnpm dev`). ' +
        'Pass --force to override; restore afterward with `pnpm sidecar:install`.'
    );
  }
  const vendor = join(SIDECAR_PKG, 'vendor');
  if (!existsSync(vendor)) {
    throw new Error(`sidecar vendor not found at ${vendor} — run \`pnpm sidecar:install\` first`);
  }
  pruneVendor(vendor, target);
  console.log(`pruned sidecar vendor for ${target} (kept ${dirsToKeep(target).join(', ')})`);
}

// Run as CLI only when invoked directly (not when imported by the test).
if (process.argv[1] && realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))) {
  try {
    main(process.argv.slice(2));
  } catch (e) {
    console.error(String(e?.message ?? e));
    process.exit(1);
  }
}
```

- [ ] **Step 4: Run the tests — expect PASS**

Run: `node --test scripts/prune-sidecar-vendor.test.mjs`
Expected: PASS (8 tests). Also run `node --check scripts/prune-sidecar-vendor.mjs` (Expected: no output).

- [ ] **Step 5: Sanity-check the CLI guard refuses outside CI**

Run: `node scripts/prune-sidecar-vendor.mjs --target linux-x64; echo "exit=$?"`
Expected: prints the "refusing to prune outside CI" error and `exit=1` (it never touches your real sidecar tree).

- [ ] **Step 6: Commit**

```bash
git add scripts/prune-sidecar-vendor.mjs scripts/prune-sidecar-vendor.test.mjs
git commit -m "feat(packaging): sidecar vendor prune script (CI-only, fail-closed, unit-tested)"
```

---

### Task 2: Node-on-PATH durable error + README line

**Files:**
- Modify: `src-tauri/src/services/agent/sdk.rs` (`spawn` ~227, `spawn_oneshot` ~270, the test `spawn_missing_node_is_opaque` ~431)
- Modify: `README.md`

- [ ] **Step 1: Update the existing test to expect the new message** (it currently asserts opaque)

In `src-tauri/src/services/agent/sdk.rs`, replace the `spawn_missing_node_is_opaque` test (lines ~430-445) with:

```rust
    #[test]
    fn spawn_missing_node_reports_node_not_found() {
        // A missing `node` (ENOENT) must surface the Node prerequisite, not an opaque error,
        // so a shipped app tells the user what to install.
        let err = match spawn(
            "goal",
            "plan",
            "",
            "",
            &std::env::temp_dir(),
            &[("UAW_AGENT_NODE".into(), "/no/such/node-xyz".into())],
        ) {
            Err(e) => e,
            Ok(_) => panic!("expected spawn to fail"),
        };
        assert!(err.contains("Node.js was not found on PATH"), "got: {err}");
    }
```

- [ ] **Step 2: Run the test — expect FAIL**

Run: `cd src-tauri && cargo test --lib spawn_missing_node 2>&1 | tail -8`
Expected: FAIL — the error is still `"Failed to start the agent sidecar"`, not containing "Node.js was not found on PATH".

- [ ] **Step 3: Add the helper + wire both spawn sites**

In `sdk.rs`, add this helper (place it just above `fn spawn`):

```rust
/// Map a sidecar spawn failure to a message. A missing `node` (ENOENT) gets a specific,
/// actionable error so a shipped app surfaces the Node prerequisite instead of an opaque
/// failure; any other spawn error keeps the caller's opaque string.
fn node_spawn_error(e: &std::io::Error, opaque: &str) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        "Node.js was not found on PATH. The SDK agent requires Node.js >= 18 (PTY agents are unaffected).".to_string()
    } else {
        opaque.to_string()
    }
}
```

In `spawn` (~line 227), change:
```rust
    let mut child = cmd
        .spawn()
        .map_err(|_| "Failed to start the agent sidecar".to_string())?;
```
to:
```rust
    let mut child = cmd
        .spawn()
        .map_err(|e| node_spawn_error(&e, "Failed to start the agent sidecar"))?;
```

In `spawn_oneshot` (~line 270), change:
```rust
    let mut child = cmd.spawn().map_err(|_| ERR.to_string())?;
```
to:
```rust
    let mut child = cmd.spawn().map_err(|e| node_spawn_error(&e, ERR))?;
```

- [ ] **Step 4: Run the full crate suite — expect PASS**

Run: `cd src-tauri && cargo test 2>&1 | tail -8 && cargo clippy --all-targets 2>&1 | tail -3`
Expected: all pass (the renamed test green; the other spawn tests unaffected — they don't trigger ENOENT). Clippy clean.

- [ ] **Step 5: Add the README prerequisite line**

In `README.md`, under the prerequisites/install/requirements section, add:
```markdown
- **Node.js 18+ on your PATH** — the SDK agent runs a Node sidecar. (The interactive PTY agents — claude/codex/gemini — use your own CLI logins and don't need this.)
```

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/services/agent/sdk.rs README.md
git commit -m "feat(packaging): a missing Node surfaces a clear SDK-agent error + README prereq"
```

---

### Task 3: The release workflow

**Files:**
- Create: `.github/workflows/release.yml`

- [ ] **Step 1: Write the workflow** (actions at version tags; SHA-pinned in Task 4)

```yaml
name: release

on:
  push:
    tags: ['v*']
  pull_request:
    paths:
      - '.github/workflows/release.yml'
      - 'scripts/prune-sidecar-vendor.mjs'
      - 'scripts/prune-sidecar-vendor.test.mjs'
      - 'src-tauri/tauri.conf.json'
      - 'sidecar/claude-agent-sdk/**'

permissions: {}

concurrency:
  group: release-${{ github.ref }}
  cancel-in-progress: false

jobs:
  test-prune:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
      - uses: actions/setup-node@v6
        with:
          node-version: 22
      - name: Lint + unit-test the prune script (no deps)
        run: |
          node --check scripts/prune-sidecar-vendor.mjs
          node --test scripts/prune-sidecar-vendor.test.mjs

  create-release:
    if: startsWith(github.ref, 'refs/tags/')
    needs: test-prune
    runs-on: ubuntu-latest
    permissions:
      contents: write
    outputs:
      release_id: ${{ steps.create.outputs.release_id }}
    steps:
      - uses: actions/checkout@v6
      - name: Assert the tag matches the bundle version
        run: |
          conf="v$(jq -r .version src-tauri/tauri.conf.json)"
          cargo_v="v$(grep -m1 '^version' src-tauri/Cargo.toml | cut -d'"' -f2)"
          test "$conf" = "$GITHUB_REF_NAME" || { echo "tag $GITHUB_REF_NAME != tauri.conf version $conf"; exit 1; }
          test "$cargo_v" = "$GITHUB_REF_NAME" || { echo "tag $GITHUB_REF_NAME != Cargo.toml version $cargo_v"; exit 1; }
      - name: Write the release body
        run: |
          cat > body.md <<'EOF'
          UAW desktop app — unsigned pre-release.

          **Requires [Node.js](https://nodejs.org) 18+ on your PATH** — the SDK agent runs a Node sidecar. (The interactive PTY agents use your own CLI logins.)

          ### Install (unsigned)
          - **macOS**: right-click the app -> Open the first time, or run `xattr -dr com.apple.quarantine /Applications/UAW.app`.
          - **Windows**: SmartScreen warns -> "More info" -> "Run anyway".
          - **Linux**: `chmod +x` the AppImage, or install the `.deb`.

          ### Verify
          - `checksums.txt` (SHA-256) detects download corruption — not tampering (a compromised release could regenerate it).
          - Build provenance: `gh attestation verify <file> --repo ${{ github.repository }}`.
          EOF
      - name: Create the draft prerelease and emit its id
        id: create
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: |
          gh release create "$GITHUB_REF_NAME" --draft --prerelease --title "UAW $GITHUB_REF_NAME" --notes-file body.md
          id=$(gh api "repos/$GITHUB_REPOSITORY/releases" --jq ".[] | select(.tag_name==\"$GITHUB_REF_NAME\") | .id")
          test -n "$id" || { echo "could not resolve release id"; exit 1; }
          echo "release_id=$id" >> "$GITHUB_OUTPUT"

  build:
    if: startsWith(github.ref, 'refs/tags/')
    needs: create-release
    permissions:
      contents: write
    strategy:
      fail-fast: false
      matrix:
        include:
          - os: macos-latest
            target: mac-universal
            args: --target universal-apple-darwin --bundles dmg
            rust-targets: aarch64-apple-darwin,x86_64-apple-darwin
          - os: ubuntu-22.04
            target: linux-x64
            args: --bundles appimage,deb
            rust-targets: ''
          - os: windows-latest
            target: win-x64
            args: --bundles nsis
            rust-targets: ''
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v6
      - uses: pnpm/action-setup@v6
      - uses: actions/setup-node@v6
        with:
          node-version: 22
          cache: pnpm
      - run: pnpm install --frozen-lockfile
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.rust-targets }}
      - uses: swatinem/rust-cache@v2
        with:
          workspaces: src-tauri -> target
      - if: matrix.os == 'ubuntu-22.04'
        name: Install Linux bundle deps
        # keep in sync with .github/workflows/e2e.yml (webkit/gtk/appindicator base set)
        run: |
          sudo apt-get update
          sudo apt-get install -y --no-install-recommends \
            libwebkit2gtk-4.1-dev build-essential curl wget file \
            libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev libgtk-3-dev
      - name: Install the sidecar (pinned, no scripts, no native optionals)
        run: pnpm sidecar:install
      - name: Prune the sidecar vendor tree for this target
        run: node scripts/prune-sidecar-vendor.mjs --target ${{ matrix.target }}
      - uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        with:
          releaseId: ${{ needs.create-release.outputs.release_id }}
          args: ${{ matrix.args }}

  finalize:
    if: startsWith(github.ref, 'refs/tags/')
    needs: build
    runs-on: ubuntu-latest
    permissions:
      contents: write
      id-token: write
      attestations: write
    steps:
      - name: Download the release installers
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: |
          mkdir -p dist-assets
          gh release download "$GITHUB_REF_NAME" --repo "$GITHUB_REPOSITORY" --dir dist-assets
      - uses: actions/attest-build-provenance@v1
        with:
          subject-path: 'dist-assets/*'
      - name: Generate + upload SHA-256 checksums
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: |
          (cd dist-assets && shasum -a 256 * > checksums.txt)
          gh release upload "$GITHUB_REF_NAME" dist-assets/checksums.txt --repo "$GITHUB_REPOSITORY"
```

- [ ] **Step 2: Validate the YAML parses** (macOS ships ruby+psych)

Run: `ruby -ryaml -e "YAML.load_file('.github/workflows/release.yml'); puts 'YAML OK'"`
Expected: `YAML OK`.

- [ ] **Step 3: Structural self-check** (read the file; confirm each, no command)

Confirm: `permissions: {}` at top; `contents: write` only on create-release/build/finalize (+ `id-token`/`attestations` only on finalize); the three release jobs carry `if: startsWith(github.ref, 'refs/tags/')` and `test-prune` does not; `build` has `fail-fast: false`; the matrix `args`/`target`/`rust-targets` triple is consistent with Task 1's targets (`mac-universal`/`linux-x64`/`win-x64`); `tauri-action` uses `releaseId: ${{ needs.create-release.outputs.release_id }}`; `finalize` has no `actions/checkout`.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "feat(packaging): tag-triggered release pipeline (3 OSes, draft, checksums + attestation)"
```

---

### Task 4: SHA-pin the release workflow's actions

**Files:**
- Modify: `.github/workflows/release.yml`

The release workflow builds shipped binaries, so its actions are pinned to commit SHAs (the CI gates stay on `@vN`). This task resolves and applies the pins.

- [ ] **Step 1: Resolve the current SHA for each action's pinned ref**

Run (needs network + `gh` auth):
```bash
for ref in actions/checkout@v6 actions/setup-node@v6 pnpm/action-setup@v6 \
           swatinem/rust-cache@v2 tauri-apps/tauri-action@v0 \
           actions/attest-build-provenance@v1 dtolnay/rust-toolchain@master; do
  repo=${ref%@*}; tag=${ref#*@}
  sha=$(gh api "repos/$repo/commits/$tag" --jq .sha 2>/dev/null)
  echo "$ref -> $sha"
done
```
Expected: a full 40-char SHA per action. (If `gh` lacks network here, this task is the one flagged follow-up — leave the `@vN` tags and mark Task 4 BLOCKED with a note that pinning must happen before the first real release.)

- [ ] **Step 2: Replace each `uses:` ref with `<owner>/<repo>@<sha> # <tag>`**

For every `uses:` in `release.yml`, replace the `@tag` with the resolved `@<sha>` and append a trailing `# <tag>` comment recording what it maps to. Special case: change `dtolnay/rust-toolchain@stable` to `dtolnay/rust-toolchain@<sha-of-master> # master (stable)` and add `toolchain: stable` to its `with:` block (a SHA ref can't carry the toolchain in the ref name):
```yaml
      - uses: dtolnay/rust-toolchain@<sha> # master (stable)
        with:
          toolchain: stable
          targets: ${{ matrix.rust-targets }}
```

- [ ] **Step 3: Validate the YAML still parses**

Run: `ruby -ryaml -e "YAML.load_file('.github/workflows/release.yml'); puts 'YAML OK'"`
Expected: `YAML OK`. Confirm every `uses:` now has a 40-char SHA + a `# tag` comment, and `dtolnay/rust-toolchain` carries `toolchain: stable`.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "chore(packaging): pin the release workflow's actions to commit SHAs"
```

---

## After all tasks

1. **Final whole-branch review** (opus) over `git diff main...HEAD` — the prune correctness (keep-set polarity, root-guard, postcondition, CI-scope), the workflow's permissions floor + job graph + the draft `releaseId` handshake, the Node-on-PATH error, and that nothing changed `tauri.conf.json`/`beforeBuildCommand`/the e2e path.
2. **Docker e2e** (`pnpm e2e:docker`) — regression gate; this branch doesn't touch the e2e path, so it must stay green (12 spec files). Plus `cd src-tauri && cargo test` (the Node-error change).
3. **Manual dry-run** (post-merge, needs the GitHub repo): push a throwaway tag `git tag v0.0.0-rc1 && git push origin v0.0.0-rc1` → confirm the run produces a **draft** prerelease with a DMG + NSIS `.exe` + AppImage + `.deb` + `checksums.txt` + a provenance attestation, then **delete the draft + the tag**. (`tauri.conf.json` version must equal the tag, or `create-release` fails by design — bump it first or use a matching tag.) Note: attestation needs a public repo (or GHAS); if private, it no-ops/fails — drop that step or accept checksums-only.
4. **Manual cross-OS publish smoke** (when cutting the real release): launch each installer, confirm the SDK agent starts (the packed-bundle copy proof CI can't reach), then publish the draft.
5. **Finish the branch** (superpowers:finishing-a-development-branch): push + PR.

---

## Self-Review

**Spec coverage:**
- Prune script (keep-set, root-guard, CI-scope, postcondition, numbers) → Task 1. ✓
- `node --test` unit + fixture tests, no vitest → Task 1 Step 1. ✓
- 4-job pipeline (`test-prune`/`create-release`/`build`/`finalize`), mixed triggers, `if`-guards → Task 3. ✓
- `permissions: {}` floor + per-job writes → Task 3 Step 1 + Step 3. ✓
- `gh release download/upload` (not `download-artifact`); finalize checkout-free → Task 3. ✓
- Draft+prerelease, `releaseId` handshake, version gate → Task 3 (create-release). ✓
- Per-OS `--bundles`, universal mac (`rust-targets`), ubuntu-22.04 + apt set, no `tauri.conf` edit → Task 3 (build matrix). ✓
- Attestation + checksums-relabel + Node-prereq in the body → Task 3 (finalize + body.md). ✓
- SHA-pin (incl. the `dtolnay/rust-toolchain` `toolchain: stable` shape) → Task 4. ✓
- Durable Node-on-PATH error + README → Task 2. ✓
- Dry-run + cross-OS smoke → After-tasks 3-4. ✓
- Out of scope (signing, Node bundling, updater, 22.04 migration) → no task touches them. ✓

**Placeholder scan:** none — full script, full workflow, full Rust diff, exact commands. The SHA values in Task 4 are resolved by the provided command (not hardcoded by design — a hardcoded SHA would be stale).

**Type/contract consistency:** `dirsToKeep`/`archesToDelete`/`pruneVendor` signatures match between Task 1's test and implementation. The prune `--target` tokens (`mac-universal`/`linux-x64`/`win-x64`) are identical in Task 1's `dirsToKeep`, Task 1's CLI, and Task 3's matrix. `node_spawn_error(&std::io::Error, &str) -> String` is used identically at both `sdk.rs` spawn sites. `release_id` output name is consistent between `create-release` (`steps.create.outputs.release_id` → job `outputs.release_id`) and `build`/`finalize` (`needs.create-release.outputs.release_id`).
