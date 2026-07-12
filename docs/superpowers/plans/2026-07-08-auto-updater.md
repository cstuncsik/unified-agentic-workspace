# Auto-updater Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **⚠ Author-gated:** Task 1 needs a minisign key the AUTHOR generates + stores as a repo secret (a subagent can't set secrets or handle a private key). Two design decisions were ASSUMED while the author was away — confirm before executing: ① releases publish as full (non-prerelease); ② startup-check + prompt + manual check.

**Goal:** UAW checks GitHub Releases on launch and can update itself in place (first updater-enabled release = v0.1.2).

**Architecture:** Tauri v2 `updater` + `process` plugins; feed = `latest.json` at the release's `/releases/latest/download/`, built by **`tauri-action`'s native `includeUpdaterJson`** (no hand-rolled manifest); artifacts minisign-signed (pubkey in-app, private key a CI secret); a frontend composable checks on mount (gated off in e2e) + shows a full-width dismissable banner.

**Tech Stack:** Rust (`tauri-plugin-updater`, `tauri-plugin-process`), Vue 3 (`@tauri-apps/plugin-updater`/`-process`), GitHub Actions (`tauri-action`).

---

## File Structure
- **Modify `src-tauri/Cargo.toml`** — the two plugin deps + `[package].version` → `0.1.2`.
- **Modify `src-tauri/src/lib.rs`** — register both plugins + an `updater_enabled` command.
- **Modify `src-tauri/tauri.conf.json`** — `plugins.updater` (pubkey + endpoint), `bundle.createUpdaterArtifacts`, version → `0.1.2`.
- **Modify `src-tauri/capabilities/default.json`** — `updater:default`, `process:allow-restart`.
- **Modify `.gitignore`** — `*.key`.
- **Modify `package.json`** — the two JS deps + version → `0.1.2`.
- **Create `src/composables/useUpdater.ts`** + **`src/components/UpdateBanner.vue`**; **modify `src/App.vue`** (wrap the grid so the banner spans full-width; gated startup check; manual affordance).
- **Modify `wdio.conf.ts`** (`UAW_DISABLE_UPDATER`) + **`e2e/specs/smoke.e2e.ts`** (assert the banner is absent).
- **Modify `.github/workflows/release.yml`** — signing env on `build`, drop `--prerelease`, fix stale "pre-release" copy. (**No** `finalize` change, **no** manifest script — `tauri-action` owns `latest.json`.)

**Ordering:** Task 1 (Rust/config, incl. the author key) enables a signed build. Task 2 (frontend) is the UI + e2e-safety. Task 3 (CI) wires the release. Verified by `cargo build`/clippy, `pnpm build` (typecheck), a branch `pnpm e2e:docker` run, and the manual cross-version smoke.

---

### Task 1: Rust plugins, config, capabilities, versions, the e2e gate + signing key

**⚠ Step 3 is AUTHOR-manual (secrets) — a subagent stops there and reports; the author runs it, then the rest proceeds.**

**Files:** `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`, `.gitignore`

- [ ] **Step 1: Cargo deps + version** — in `src-tauri/Cargo.toml`, after `tauri-plugin-opener = "2"` add:
```toml
tauri-plugin-updater = "2"
tauri-plugin-process = "2"
```
and bump `[package]` `version = "0.1.1"` → `version = "0.1.2"` (line 3 — the release gate asserts this equals the tag).

- [ ] **Step 2: Register the plugins + the `updater_enabled` command** — in `src-tauri/src/lib.rs`:
  - After `.plugin(tauri_plugin_opener::init())`:
```rust
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
```
  - Add this free function above `pub fn run()`:
```rust
/// Whether the frontend should run the startup update check. The e2e harness sets
/// UAW_DISABLE_UPDATER so the auto-check (and its banner) never fire during tests.
#[tauri::command]
fn updater_enabled() -> bool {
    std::env::var("UAW_DISABLE_UPDATER").is_err()
}
```
  - Add `updater_enabled,` to the `tauri::generate_handler![ … ]` list (e.g. after `commands::board::get_board,`).

- [ ] **Step 3 (AUTHOR, manual — secrets): generate the signing key**
```bash
pnpm tauri signer generate -w "$HOME/.uaw-updater.key"   # choose a password; prints the PUBLIC KEY (dW...)
gh secret set TAURI_SIGNING_PRIVATE_KEY < "$HOME/.uaw-updater.key"
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD          # paste the password
```
Copy the printed **public key** for Step 4. The `~/.uaw-updater.key` file stays outside the repo.

- [ ] **Step 4: Wire `tauri.conf.json`**
  - `"version": "0.1.1"` → `"version": "0.1.2"`.
  - In `"bundle"`, add `"createUpdaterArtifacts": true`.
  - Add a top-level `"plugins"` block with the **public key from Step 3**:
```json
  "plugins": {
    "updater": {
      "pubkey": "<PASTE THE PUBLIC KEY FROM STEP 3>",
      "endpoints": ["https://github.com/cstuncsik/unified-agentic-workspace/releases/latest/download/latest.json"]
    }
  }
```

- [ ] **Step 5: Capabilities + gitignore**
  - `src-tauri/capabilities/default.json` `permissions`: `["core:default", "opener:default", "updater:default", "process:allow-restart"]`.
  - `.gitignore`: add a line `*.key`.

- [ ] **Step 6: Build + clippy — expect PASS** (needs the real pubkey; Tauri validates it)

Run: `cd src-tauri && cargo build 2>&1 | tail -4 && cargo clippy --all-targets 2>&1 | tail -3`
Expected: builds + clippy clean. A malformed pubkey → a `tauri` build-script error (paste Step 3's output).

- [ ] **Step 7: Commit** (the private key is NOT among these files)
```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/lib.rs src-tauri/tauri.conf.json src-tauri/capabilities/default.json .gitignore
git commit -m "feat(updater): register updater+process plugins, config, capabilities, e2e gate"
```

---

### Task 2: Frontend — check on launch + a full-width dismissable banner

**Files:** `package.json`, Create `src/composables/useUpdater.ts` + `src/components/UpdateBanner.vue`, Modify `src/App.vue`, `wdio.conf.ts`, `e2e/specs/smoke.e2e.ts`

- [ ] **Step 1: JS deps + version** — in `package.json`: add to `dependencies`
```json
    "@tauri-apps/plugin-process": "^2",
    "@tauri-apps/plugin-updater": "^2",
```
and bump `"version"` → `"0.1.2"`. Run `pnpm install` (updates `pnpm-lock.yaml`).

- [ ] **Step 2: The composable** — `src/composables/useUpdater.ts` (module-singleton, mirrors `useConfirm.ts`):
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
      } else {
        available.value = null; // clear a stale banner from a prior check
        if (!silent) toast.success("You're on the latest version.");
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

- [ ] **Step 3: The banner** — `src/components/UpdateBanner.vue` (design-system: `re-button` + `--re-color-*` + `role="status"`):
```vue
<script setup lang="ts">
import { useUpdater } from "../composables/useUpdater";

const { available, installing, installAndRestart, dismiss } = useUpdater();
</script>

<template>
  <div v-if="available" class="update-banner" role="status" data-testid="update-banner">
    <span>UAW {{ available.version }} is available.</span>
    <button class="re-button" data-variant="brand" :disabled="installing" @click="installAndRestart">
      {{ installing ? "Updating…" : "Update & Restart" }}
    </button>
    <button class="re-button" data-variant="ghost" :disabled="installing" @click="dismiss">Dismiss</button>
  </div>
</template>

<style scoped>
.update-banner {
  display: flex;
  gap: 0.75rem;
  align-items: center;
  padding: 0.5rem 1rem;
  background: var(--re-color-bg-muted);
  border-bottom: 1px solid var(--re-color-border);
}
</style>
```
(If `--re-color-bg-muted`/`--re-color-border` aren't the exact token names, grep the app's existing `<style>` blocks / the Renascent tokens for the nearest surface + border `--re-color-*` and use those — do NOT invent `--color-*` vars.)

- [ ] **Step 4: Wire `App.vue`** — wrap the grid, gate the startup check, add the manual affordance:
  - **Script** (`<script setup>`): add
```ts
import { invoke } from "@tauri-apps/api/core";
import UpdateBanner from "./components/UpdateBanner.vue";
import { useUpdater } from "./composables/useUpdater";
```
    and after the other composable/store setup: `const updater = useUpdater();`
  - **`onMounted`** (App.vue:54) — make it async + gate the check:
```ts
onMounted(async () => {
  workspaces.load();
  if (await invoke<boolean>("updater_enabled")) {
    void updater.checkForUpdate({ silent: true });
  }
});
```
  - **Template** — wrap the existing root grid so the banner sits above it, full width:
```vue
<template>
  <div class="app-root">
    <UpdateBanner />
    <div class="app">
      <!-- …existing sidebar + main, unchanged… -->
    </div>
  </div>
</template>
```
  - **Manual affordance** — beside `<ThemeToggle />` (App.vue:200):
```vue
        <button class="re-button" data-variant="ghost" @click="updater.checkForUpdate({ silent: false })">
          Check for updates
        </button>
```
  - **Styles** (`<style scoped>`) — add `.app-root` and change `.app`'s height (App.vue:251-255):
```css
.app-root {
  display: flex;
  flex-direction: column;
  height: 100vh;
}
.app {
  display: grid;
  grid-template-columns: 240px 1fr;
  flex: 1;
  min-height: 0;
}
```
(i.e. move `height: 100vh` off `.app` onto `.app-root`; `.app` keeps its grid but now fills the remaining flex space.)

- [ ] **Step 5: e2e gate + assertion**
  - `wdio.conf.ts` — in `beforeSession` (after the other `process.env.UAW_*` assignments, ~line 62): `process.env.UAW_DISABLE_UPDATER = "1";`
  - `e2e/specs/smoke.e2e.ts` — add an assertion (in an existing `it`, or a new one) that the banner never renders under the gate:
```ts
    expect(await $('[data-testid="update-banner"]').isExisting()).toBe(false);
```

- [ ] **Step 6: Typecheck + build — expect PASS**

Run: `pnpm build 2>&1 | tail -5` (`vue-tsc --noEmit && vite build`).
Expected: no type errors; build succeeds. (No frontend unit test — the repo has no JS unit runner; the composable is covered by typecheck + the manual smoke. Do NOT add vitest.)

- [ ] **Step 7: Commit**
```bash
git add package.json pnpm-lock.yaml src/composables/useUpdater.ts src/components/UpdateBanner.vue src/App.vue wdio.conf.ts e2e/specs/smoke.e2e.ts
git commit -m "feat(updater): check on launch + a full-width dismissable banner (e2e-gated)"
```

---

### Task 3: Release CI — sign, publish as a full release

**Files:** `.github/workflows/release.yml`

- [ ] **Step 1: Publish as a full release** — in `create-release`, drop `--prerelease` (keep `--draft`) and de-stale the copy:
  - The `gh release create` line → `gh release create "$GITHUB_REF_NAME" --draft --title "UAW $GITHUB_REF_NAME" --notes-file body.md \`
  - Rename the step `name: Create the draft prerelease and emit its id` → `name: Create the draft release and emit its id`.
  - In the `body.md` heredoc, change "unsigned pre-release." → "unsigned release." (Gatekeeper/SmartScreen instructions stay; it's still OS-unsigned, just no longer a GitHub *pre*-release.)

- [ ] **Step 2: Sign the update artifacts** — on the `build` job's `tauri-apps/tauri-action` step, add the signing env. **Do NOT set `includeUpdaterJson`** — its default `true` makes `tauri-action` build + merge `latest.json` from build metadata (both darwin arches at the universal artifact, `pub_date`/`notes`, per-platform `.sig`), which is more robust than any hand-rolled suffix matching:
```yaml
      - uses: tauri-apps/tauri-action@84b9d35b5fc46c1e45415bdb6144030364f7ebc5 # v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
        with:
          releaseId: ${{ needs.create-release.outputs.release_id }}
          args: ${{ matrix.args }}
```
(Optional determinism: add `max-parallel: 1` under the `build` job's `strategy:` to serialize the manifest read-modify-write; skip it and lean on the draft gate + the pre-publish 4-key check if you prefer speed.) **`finalize` is unchanged** — it downloads all assets (now incl. `latest.json`) and checksums + attests them.

- [ ] **Step 3: Validate the YAML**

Run: `ruby -ryaml -e "YAML.load_file('.github/workflows/release.yml'); puts 'YAML OK'"`

- [ ] **Step 4: Commit**
```bash
git add .github/workflows/release.yml
git commit -m "feat(updater): sign update artifacts + publish as a full release"
```

---

## After all tasks

1. **Final whole-branch review** (opus) over `git diff main...HEAD` — plugin registration + capabilities + the `updater_enabled` gate, the `createUpdaterArtifacts`/pubkey/endpoint wiring, all three version bumps (esp. `Cargo.toml` for the gate), the CI signing env + `includeUpdaterJson` default + the `--prerelease` drop, the banner's full-width layout + design-system classes, and that no private key is committed (`git show` the Task-1 commit).
2. **`cargo test` + `cargo clippy` green; `pnpm build` typechecks; RUN `pnpm e2e:docker` on the branch** — must be 12/12 (the startup check is gated off by `UAW_DISABLE_UPDATER`, and the new spec asserts the banner is absent). Do not assume it — run it.
3. **Manual cross-version smoke — the real proof** (CI can't simulate a version delta), on **all three OSes**:
   - Cut a real **v0.1.2** (the pipeline signs + `tauri-action` publishes `latest.json`); confirm the release has the updater artifacts + `.sig`s and a `latest.json` with **all four** platform keys (`darwin-aarch64`, `darwin-x86_64`, `windows-x86_64`, `linux-x86_64`) — this catches a pubkey/private-key mismatch or a missing platform *before* users hit it.
   - On macOS, Windows, and Linux (AppImage): launch a v0.1.1-or-lower build → banner → Update & Restart → it downloads, verifies the signature, installs, relaunches into v0.1.2.
   - **Negative (rejection) check** — corrupt a signature or use a different key in a test manifest → confirm `downloadAndInstall()` **fails** (the "Update failed to install" toast, no relaunch). The security guarantee rests on rejection; prove it.
   - Release notes: **`.deb` doesn't auto-update**; **v0.1.1 users update to v0.1.2 manually once**; **key rotation would strand all clients** (a future note like the v0.1.1 one).
4. **Finish the branch** (superpowers:finishing-a-development-branch): push + PR. **Do NOT publish v0.1.2 until the cross-version + rejection smoke passes.** Note: local `pnpm bundle` now needs `TAURI_SIGNING_PRIVATE_KEY` set (createUpdaterArtifacts); `e2e:build`'s `--no-bundle` is unaffected.

---

## Self-Review

**Spec coverage:**
- Signing key (pubkey in conf, private key as secret, `.gitignore *.key`) → Task 1 Steps 3–5. ✓
- `createUpdaterArtifacts` + `plugins.updater` (pubkey + `/releases/latest/` endpoint) → Task 1 Step 4. ✓
- `updater`+`process` plugins + capabilities + the `updater_enabled` gate → Task 1 Steps 2, 5. ✓
- All three version bumps (Cargo.toml is the gated one) → Task 1 Steps 1, 4; Task 2 Step 1. ✓
- Frontend check-on-mount (gated) + full-width banner (design-system + `role=status`) + manual check + stale-banner clear → Task 2. ✓
- e2e gate (`UAW_DISABLE_UPDATER`) + banner-absent assertion + RUN e2e → Task 2 Step 5, After-tasks 2. ✓
- CI: drop `--prerelease` + de-stale copy, signing env, `includeUpdaterJson` default (no manifest script/finalize change) → Task 3. ✓
- Manual smoke on all 3 OSes + the rejection check + the platform/key gate → After-tasks 3. ✓
- Docs: deb-no-autoupdate, v0.1.1-stranded, key-rotation-strands, local-bundle-needs-key → After-tasks 3–4. ✓
- Out of scope (OS signing, deb auto-update, dynamic server, channels, delta) → no task touches them. ✓

**Placeholder scan:** the only fill-in is the **public key** (Task 1 Step 4) — it doesn't exist until the author generates it (Step 3). Every code/CI block is complete. The `--re-color-*` token names have a verify-and-substitute fallback (not a TBD).

**Type/contract consistency:** `useUpdater()` returns `{available, installing, checkForUpdate, installAndRestart, dismiss}` — used identically in `UpdateBanner.vue` (Task 2 Step 3) and `App.vue` (Step 4). The `updater_enabled` Rust command (Task 1 Step 2) matches the `invoke<boolean>("updater_enabled")` call (Task 2 Step 4) and the `UAW_DISABLE_UPDATER` env it reads (Task 2 Step 5, `wdio.conf.ts`). `plugins.updater.pubkey` pairs with `TAURI_SIGNING_PRIVATE_KEY` (Task 3). The manifest platform keys are `tauri-action`'s (`{os}-{arch}`), matching Tauri's runtime matcher — not hand-authored.
