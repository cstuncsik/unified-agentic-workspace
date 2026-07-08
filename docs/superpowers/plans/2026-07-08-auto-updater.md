# Auto-updater Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **⚠ Author-gated:** Task 2 needs a minisign key the AUTHOR generates + stores as a repo secret (a subagent can't set secrets or safely handle a private key). And two design decisions were ASSUMED while the author was away — confirm before executing: ① releases published as full (non-prerelease); ② startup-check + prompt UX.

**Goal:** UAW checks GitHub Releases on launch and can update itself in place (first updater-enabled release = v0.1.2).

**Architecture:** Tauri v2 `updater` + `process` plugins; feed = `latest.json` served from the GitHub release's `/releases/latest/download/`; update artifacts signed with a minisign key (pubkey in-app, private key a CI secret); the release CI's `finalize` job assembles the multi-platform `latest.json`; a frontend composable checks on mount + shows a dismissable banner.

**Tech Stack:** Rust (`tauri-plugin-updater`, `tauri-plugin-process`), Vue 3 (`@tauri-apps/plugin-updater`/`-process`), Node `node --test`, GitHub Actions.

---

## File Structure
- **Create `scripts/build-latest-json.mjs`** (+ `.test.mjs`) — pure `buildLatestJson` + `platformKeysFor` + a thin dir-scanning CLI that assembles the Tauri updater manifest from the signed artifacts. Zero-dep, `node --test`.
- **Modify `src-tauri/Cargo.toml`** — `tauri-plugin-updater`, `tauri-plugin-process`.
- **Modify `src-tauri/src/lib.rs`** — register both plugins.
- **Modify `src-tauri/tauri.conf.json`** — `plugins.updater` (pubkey + endpoint), `bundle.createUpdaterArtifacts: true`, version → `0.1.2`.
- **Modify `src-tauri/capabilities/default.json`** — `updater:default`, `process:allow-restart`.
- **Modify `package.json`** — `@tauri-apps/plugin-updater`, `@tauri-apps/plugin-process`; version → `0.1.2`.
- **Create `src/composables/useUpdater.ts`** (module-singleton state) + **`src/components/UpdateBanner.vue`**; **modify `src/App.vue`** (render the banner + startup check + a manual "Check for updates").
- **Modify `.github/workflows/release.yml`** — signing env on `build`, `includeUpdaterJson: false`, drop `--prerelease`, assemble+upload `latest.json` in `finalize`.

**Ordering:** Task 1 (the manifest script) is standalone + foundational (CI uses it). Task 2 (Rust plugins + conf + the key) enables a signed build. Task 3 (frontend) is the UI. Task 4 (CI + version) wires the release. Task 1 is pure-TDD; Task 2's key step is author-manual; Tasks 3–4 are verified by typecheck + the manual cross-version smoke.

---

### Task 1: `build-latest-json.mjs` — the updater-manifest assembler + tests

**Files:** Create `scripts/build-latest-json.mjs`, `scripts/build-latest-json.test.mjs`

- [ ] **Step 1: Write the failing tests** — `scripts/build-latest-json.test.mjs`:

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import { buildLatestJson, platformKeysFor } from "./build-latest-json.mjs";

test("platformKeysFor maps each updater artifact to its Tauri platform keys", () => {
  assert.deepEqual(platformKeysFor("UAW_0.1.2_universal.app.tar.gz"), ["darwin-aarch64", "darwin-x86_64"]);
  assert.deepEqual(platformKeysFor("UAW_0.1.2_x64-setup.exe"), ["windows-x86_64"]);
  assert.deepEqual(platformKeysFor("UAW_0.1.2_amd64.AppImage"), ["linux-x86_64"]);
});
test("platformKeysFor ignores non-updater assets (installers, sigs, checksums)", () => {
  for (const f of ["UAW_0.1.2_universal.dmg", "UAW_0.1.2_amd64.deb",
                   "UAW_0.1.2_universal.app.tar.gz.sig", "UAW_0.1.2_x64-setup.exe.sig",
                   "UAW_0.1.2_amd64.AppImage.sig", "checksums.txt"]) {
    assert.deepEqual(platformKeysFor(f), [], f);
  }
});
test("buildLatestJson: both darwin keys point at the SAME universal artifact", () => {
  const m = buildLatestJson({
    version: "0.1.2", notes: "n", pubDate: "2026-07-08T00:00:00Z",
    entries: [
      { keys: ["darwin-aarch64", "darwin-x86_64"], url: "https://x/app.tar.gz", signature: "SIGMAC" },
      { keys: ["windows-x86_64"], url: "https://x/setup.exe", signature: "SIGWIN" },
      { keys: ["linux-x86_64"], url: "https://x/app.AppImage", signature: "SIGLIN" },
    ],
  });
  assert.equal(m.version, "0.1.2");
  assert.equal(m.pub_date, "2026-07-08T00:00:00Z");
  assert.deepEqual(m.platforms["darwin-aarch64"], { signature: "SIGMAC", url: "https://x/app.tar.gz" });
  assert.deepEqual(m.platforms["darwin-x86_64"], m.platforms["darwin-aarch64"]);
  assert.equal(m.platforms["windows-x86_64"].signature, "SIGWIN");
  assert.equal(m.platforms["linux-x86_64"].signature, "SIGLIN");
});
test("buildLatestJson throws (never ships a half-manifest) if a platform is missing", () => {
  assert.throws(() => buildLatestJson({
    version: "0.1.2", notes: "", pubDate: "2026-07-08T00:00:00Z",
    entries: [{ keys: ["windows-x86_64"], url: "u", signature: "s" }],
  }), /missing platforms: darwin-aarch64, darwin-x86_64, linux-x86_64/);
});
```

- [ ] **Step 2: Run — expect FAIL (module missing)**

Run: `node --test scripts/build-latest-json.test.mjs 2>&1 | tail -8`
Expected: cannot find `./build-latest-json.mjs`.

- [ ] **Step 3: Implement** — `scripts/build-latest-json.mjs`:

```js
import { readFileSync, readdirSync, realpathSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

// Map an updater artifact filename to its Tauri updater platform keys. A universal-mac
// build has no `universal` updater key — list BOTH darwin arches at the same artifact.
// NOTE: confirm these suffixes against a real `tauri build` + createUpdaterArtifacts output
// (Task 2's build); a mismatch makes `buildLatestJson` throw "missing platforms" — loud, not silent.
export function platformKeysFor(filename) {
  if (filename.endsWith(".app.tar.gz")) return ["darwin-aarch64", "darwin-x86_64"];
  if (filename.endsWith("-setup.exe")) return ["windows-x86_64"];
  if (filename.endsWith(".AppImage")) return ["linux-x86_64"];
  return [];
}

// Pure: assemble the Tauri updater manifest; throw if any required platform is absent.
export function buildLatestJson({ version, notes, pubDate, entries }) {
  const REQUIRED = ["darwin-aarch64", "darwin-x86_64", "windows-x86_64", "linux-x86_64"];
  const platforms = {};
  for (const e of entries) {
    for (const k of e.keys) platforms[k] = { signature: e.signature, url: e.url };
  }
  const missing = REQUIRED.filter((k) => !platforms[k]);
  if (missing.length) throw new Error(`latest.json missing platforms: ${missing.join(", ")}`);
  return { version, notes, pub_date: pubDate, platforms };
}

function argOf(argv, flag) {
  const i = argv.indexOf(flag);
  if (i === -1 || i + 1 >= argv.length) throw new Error(`missing ${flag} <value>`);
  return argv[i + 1];
}

// Thin CLI: scan a dir of downloaded release assets, read each updater artifact's `.sig`,
// build the manifest, print it. Usage: --dir D --repo owner/repo --tag vX --version X [--notes N]
function main(argv) {
  const dir = argOf(argv, "--dir");
  const repo = argOf(argv, "--repo");
  const tag = argOf(argv, "--tag");
  const version = argOf(argv, "--version");
  const notes = argv.includes("--notes") ? argOf(argv, "--notes") : "";
  const base = `https://github.com/${repo}/releases/download/${tag}`;
  const files = readdirSync(dir);
  const entries = [];
  for (const f of files) {
    const keys = platformKeysFor(f);
    if (!keys.length) continue;
    if (!files.includes(`${f}.sig`)) throw new Error(`missing signature ${f}.sig`);
    const signature = readFileSync(join(dir, `${f}.sig`), "utf8").trim();
    entries.push({ keys, url: `${base}/${encodeURIComponent(f)}`, signature });
  }
  const manifest = buildLatestJson({ version, notes, pubDate: new Date().toISOString(), entries });
  process.stdout.write(JSON.stringify(manifest, null, 2));
}

if (process.argv[1] && realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))) {
  try {
    main(process.argv.slice(2));
  } catch (e) {
    console.error(String(e?.message ?? e));
    process.exit(1);
  }
}
```

- [ ] **Step 4: Run — expect PASS**

Run: `node --test scripts/build-latest-json.test.mjs 2>&1 | tail -6` (4 tests pass) and `node --check scripts/build-latest-json.mjs`.

- [ ] **Step 5: Commit**

```bash
git add scripts/build-latest-json.mjs scripts/build-latest-json.test.mjs
git commit -m "feat(updater): latest.json assembler for the release feed"
```

---

### Task 2: Rust updater + process plugins, tauri.conf, capabilities, signing key

**⚠ Contains an AUTHOR-manual key step — a subagent stops at Step 3 and reports; the author does Step 3 then the rest proceeds.**

**Files:** `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`

- [ ] **Step 1: Add the Cargo deps** — in `src-tauri/Cargo.toml` `[dependencies]`, after `tauri-plugin-opener = "2"`:
```toml
tauri-plugin-updater = "2"
tauri-plugin-process = "2"
```

- [ ] **Step 2: Register both plugins** — in `src-tauri/src/lib.rs`, after `.plugin(tauri_plugin_opener::init())`:
```rust
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
```

- [ ] **Step 3 (AUTHOR, manual — needs secrets): generate the signing key**

```bash
# Generate a minisign keypair (choose a password; store both securely):
pnpm tauri signer generate -w "$HOME/.uaw-updater.key"
# → prints the PUBLIC KEY (a `dW...` base64 string) and writes the private key to ~/.uaw-updater.key
# Store the private key + password as repo secrets (NEVER commit the key file):
gh secret set TAURI_SIGNING_PRIVATE_KEY < "$HOME/.uaw-updater.key"
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD   # paste the password when prompted
```
Copy the printed **public key** — it goes in `tauri.conf.json` in Step 4. (The private key file stays out of the repo; `.uaw-updater.key` is not under the repo tree.)

- [ ] **Step 4: Wire `tauri.conf.json`** — set the version, add `createUpdaterArtifacts`, and the `plugins.updater` block with the **public key from Step 3**:
  - `"version": "0.1.1"` → `"version": "0.1.2"`.
  - In `"bundle"`, add `"createUpdaterArtifacts": true`.
  - Add a top-level `"plugins"` block:
```json
  "plugins": {
    "updater": {
      "pubkey": "<PASTE THE PUBLIC KEY FROM STEP 3>",
      "endpoints": ["https://github.com/cstuncsik/unified-agentic-workspace/releases/latest/download/latest.json"]
    }
  }
```

- [ ] **Step 5: Grant the capabilities** — in `src-tauri/capabilities/default.json`, extend `permissions`:
```json
  "permissions": ["core:default", "opener:default", "updater:default", "process:allow-restart"]
```

- [ ] **Step 6: Build + clippy — expect PASS** (requires the real pubkey from Step 3; Tauri validates it)

Run: `cd src-tauri && cargo build 2>&1 | tail -4 && cargo clippy --all-targets 2>&1 | tail -3`
Expected: builds (the two plugins compile), clippy clean. If the pubkey is malformed, `tauri`'s build script errors — paste the exact Step-3 output.

- [ ] **Step 7: Commit** (the private key is NOT among these files)

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/lib.rs src-tauri/tauri.conf.json src-tauri/capabilities/default.json
git commit -m "feat(updater): register the updater+process plugins, config, and capabilities"
```

---

### Task 3: Frontend — check on launch + a dismissable update banner

**Files:** `package.json`, Create `src/composables/useUpdater.ts` + `src/components/UpdateBanner.vue`, Modify `src/App.vue`

- [ ] **Step 1: Add the JS deps** — in `package.json` `dependencies` (keep sorted-ish):
```json
    "@tauri-apps/plugin-process": "^2",
    "@tauri-apps/plugin-updater": "^2",
```
Run `pnpm install` (updates the lockfile).

- [ ] **Step 2: The composable** — `src/composables/useUpdater.ts` (module-singleton so the banner + a manual button share one state):
```ts
import { ref } from "vue";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { useToast } from "./useToast";

const available = ref<{ version: string } | null>(null);
const installing = ref(false);
let pending: Update | null = null;

export function useUpdater() {
  const toast = useToast();

  // `silent` = the startup path: never toast when up-to-date or on error, only surface a real update.
  async function checkForUpdate({ silent }: { silent: boolean }) {
    try {
      pending = await check();
      if (pending) {
        available.value = { version: pending.version };
      } else if (!silent) {
        toast.success("You're on the latest version.");
      }
    } catch {
      if (!silent) toast.error("Update check failed.");
    }
  }

  async function installAndRestart() {
    if (!pending) return;
    installing.value = true;
    try {
      await pending.downloadAndInstall();
      await relaunch();
    } catch {
      installing.value = false;
      toast.error("Update failed to install.");
    }
  }

  function dismiss() {
    available.value = null;
  }

  return { available, installing, checkForUpdate, installAndRestart, dismiss };
}
```

- [ ] **Step 3: The banner** — `src/components/UpdateBanner.vue`:
```vue
<script setup lang="ts">
import { useUpdater } from "../composables/useUpdater";

const { available, installing, installAndRestart, dismiss } = useUpdater();
</script>

<template>
  <div v-if="available" class="update-banner" data-testid="update-banner">
    <span>UAW {{ available.version }} is available.</span>
    <button type="button" :disabled="installing" @click="installAndRestart">
      {{ installing ? "Updating…" : "Update & Restart" }}
    </button>
    <button type="button" :disabled="installing" @click="dismiss">Dismiss</button>
  </div>
</template>

<style scoped>
.update-banner {
  display: flex;
  gap: 0.5rem;
  align-items: center;
  padding: 0.5rem 1rem;
  background: var(--color-surface-raised, #1e1e28);
  border-bottom: 1px solid var(--color-border, #333);
}
</style>
```

- [ ] **Step 4: Wire into `App.vue`** — import + render the banner at the top of the template, run the silent startup check in `onMounted`, and add a manual "Check for updates" affordance near `ThemeToggle`:
  - Script: `import UpdateBanner from "./components/UpdateBanner.vue";` and `import { useUpdater } from "./composables/useUpdater";` then `const updater = useUpdater();`
  - In the existing `onMounted(() => { … })` (App.vue:54), add as the first line: `void updater.checkForUpdate({ silent: true });`
  - Template: put `<UpdateBanner />` as the first child of the root; add a button beside `<ThemeToggle />`: `<button type="button" class="muted" @click="updater.checkForUpdate({ silent: false })">Check for updates</button>`

- [ ] **Step 5: Typecheck + build — expect PASS**

Run: `pnpm build 2>&1 | tail -5` (this is `vue-tsc --noEmit && vite build`).
Expected: no type errors; build succeeds. (No frontend unit test — the repo has no JS unit runner; the composable is trivial + typechecked, and the real behavior is the manual cross-version smoke. Do NOT add vitest.)

- [ ] **Step 6: Commit**

```bash
git add package.json pnpm-lock.yaml src/composables/useUpdater.ts src/components/UpdateBanner.vue src/App.vue
git commit -m "feat(updater): check on launch + a dismissable update banner"
```

---

### Task 4: Release CI — sign, aggregate the manifest, publish as full release

**Files:** `.github/workflows/release.yml`

- [ ] **Step 1: Publish as a full release** — in the `create-release` `gh release create` line, remove `--prerelease` (keep `--draft`):
```bash
          gh release create "$GITHUB_REF_NAME" --draft --title "UAW $GITHUB_REF_NAME" --notes-file body.md \
```

- [ ] **Step 2: Sign the update artifacts + suppress per-job manifests** — in the `build` job's `tauri-apps/tauri-action` step, add the signing env + the `includeUpdaterJson: false` input:
```yaml
      - uses: tauri-apps/tauri-action@84b9d35b5fc46c1e45415bdb6144030364f7ebc5 # v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
        with:
          releaseId: ${{ needs.create-release.outputs.release_id }}
          args: ${{ matrix.args }}
          includeUpdaterJson: false
```
(Each matrix job now uploads its signed `.sig` + updater artifact but NOT a single-platform `latest.json` — `finalize` assembles the aggregate one, avoiding the 3-job clobber/race.)

- [ ] **Step 3: Assemble + upload `latest.json` in `finalize`** — add a step to the `finalize` job AFTER the checksums step (it reuses the already-downloaded `dist-assets`):
```yaml
      - name: Assemble + upload the updater manifest
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: |
          version="${GITHUB_REF_NAME#v}"
          node scripts/build-latest-json.mjs \
            --dir dist-assets --repo "$GITHUB_REPOSITORY" --tag "$GITHUB_REF_NAME" \
            --version "$version" --notes "UAW $GITHUB_REF_NAME" > latest.json
          gh release upload "$GITHUB_REF_NAME" latest.json --repo "$GITHUB_REPOSITORY" --clobber
```
Note: the `finalize` job checks out no repo today — it needs `scripts/build-latest-json.mjs`. Add `- uses: actions/checkout@df4cb1c069e1874edd31b4311f1884172cec0e10 # v6` as the FIRST step of `finalize` (and Node is present on `ubuntu-latest`; if `node` isn't guaranteed, add `actions/setup-node`). If `build-latest-json.mjs` errors "missing platforms", the real updater-artifact suffixes differ from `platformKeysFor` — fix the suffixes (Task 1) against the actual asset names on the release.

- [ ] **Step 4: Validate the workflow YAML**

Run: `ruby -ryaml -e "YAML.load_file('.github/workflows/release.yml'); puts 'YAML OK'"`

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "feat(updater): sign update artifacts + publish latest.json; ship as full release"
```

---

## After all tasks

1. **Final whole-branch review** (opus) over `git diff main...HEAD` — plugin registration + capabilities, the `createUpdaterArtifacts`/pubkey/endpoint wiring, the CI signing + the `finalize` manifest assembly (checkout added; `includeUpdaterJson:false`), the `--prerelease` drop, the frontend check-swallows-errors (so the e2e/dev off-bundle path is a silent no-op), and that no private key is committed.
2. **`cargo test` + `cargo clippy` + `node --test scripts/build-latest-json.test.mjs`** green; **`pnpm e2e:docker`** stays green (the startup `check()` errors off-bundle → swallowed silently → no banner/toast → 12/12).
3. **Manual cross-version smoke — the real proof** (CI can't simulate a version delta):
   - After merge, cut a real **v0.1.2** (the pipeline signs + publishes `latest.json`); confirm the release has `*.app.tar.gz(.sig)`, `*-setup.exe(.sig)`, `*.AppImage(.sig)`, and `latest.json` with all four platform keys.
   - On a Mac running a **v0.1.1-or-lower** build (or a locally version-lowered build), launch it → the banner should appear → Update & Restart → it downloads, verifies the signature, installs, relaunches into v0.1.2.
   - Note in the release body: **`.deb` users don't auto-update** (grab the new `.deb`); **v0.1.1 users update to v0.1.2 manually once** (v0.1.1 predates the updater).
4. **Finish the branch** (superpowers:finishing-a-development-branch): push + PR. (Do NOT publish v0.1.2 until the smoke-test passes.)

---

## Self-Review

**Spec coverage:**
- Signing key (pubkey in conf, private key as secret) → Task 2 Steps 3–4. ✓
- `createUpdaterArtifacts` + `plugins.updater` (pubkey + `/releases/latest/` endpoint) → Task 2 Step 4. ✓
- `updater`+`process` plugins + capabilities → Task 2 Steps 1–2, 5. ✓
- Frontend check-on-mount + dismissable prompt + manual check, errors swallowed → Task 3. ✓
- CI: drop `--prerelease`, signing env, `includeUpdaterJson:false`, `finalize` assembles `latest.json` → Task 4. ✓
- Both darwin keys → same universal artifact (the mac-match correctness case) → Task 1 (`platformKeysFor`/`buildLatestJson`) + its test. ✓
- `latest.json` = a pure, unit-tested assembler → Task 1. ✓
- Per-platform reach (deb excluded) + v0.1.1-stranded note → After-tasks 3. ✓
- Version → 0.1.2 → Task 2 Step 4 (conf) + Task 3 Step 1 (package.json). ✓
- Out of scope (signing/notarization, deb auto-update, dynamic server, channels, delta) → no task touches them. ✓

**Placeholder scan:** the only fill-in is the **public key** (Task 2 Step 4) — it genuinely doesn't exist until the author generates it in Step 3; every code/CI block is complete. No TBDs.

**Type/contract consistency:** `buildLatestJson({version,notes,pubDate,entries})` + `platformKeysFor(filename)` are identical between Task 1's test and impl, and the CLI invocation in Task 4 Step 3 matches the CLI flags (`--dir/--repo/--tag/--version/--notes`). `useUpdater()` returns `{available, installing, checkForUpdate, installAndRestart, dismiss}` — used identically in `UpdateBanner.vue` (Task 3 Step 3) and `App.vue` (Step 4). The `plugins.updater.pubkey` (conf) pairs with the `TAURI_SIGNING_PRIVATE_KEY` secret (CI). The manifest platform keys (`darwin-aarch64`/`darwin-x86_64`/`windows-x86_64`/`linux-x86_64`) match Tauri's `{os}-{arch}` matcher.
