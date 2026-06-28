# Packaging Slice A — Bundle the SDK sidecar — Design

**Goal:** A locally-built app's SDK agent finds + runs its bundled Node sidecar. Today `resolve_sidecar_script` resolves the script against `current_dir()` (the OS launch dir in a packaged app) → the bundled sidecar is never found → a `tauri build` ships an app whose SDK agent (and model picker) can't start. Provable by a clean-tree local `tauri build` → launch → run a real SDK agent.

**Status:** Approved design (post 5-discipline review, verified against Tauri v2.11 source; findings folded in). Ready for an implementation plan.

**Context:** Slice A of "packaging / real distribution" (the original goal, now unblocked — its prerequisites cross-platform keystore #19, SDK sidecar cross-platform #20, SDK credential isolation #21, PTY ambient #22 all shipped). The review split packaging into **Slice A (this — bundle the sidecar, a local build works)** and **Slice B (the release CI)**.

---

## Background (verified against Tauri v2.11 source)
- `tauri-build`'s build script copies `bundle.resources` at **compile time** (every `cargo build`, incl. `tauri dev`), walking whatever is on disk into `target/<profile>/...`; the bundler later copies that into the app's resource dir. A *missing* mapped child (e.g. an un-installed `node_modules`) is **not** an error — the dir still copies with fewer files. So an incomplete sidecar ships **silently**.
- The `bundle.resources` **map form** `{ "../sidecar/claude-agent-sdk": "sidecar/claude-agent-sdk" }` recurses the dir and lands files at `<resource_dir>/sidecar/claude-agent-sdk/<...>` (avoids the `_up_/` prefix the list form emits for `../`) — verified by `tauri-utils` test `resource_paths_iter_map_allow_walk`.
- In **dev**, `resource_dir()` returns the cargo target dir (`target/debug/`), and the build script has *already* copied the committed `index.mjs`/`list-models.mjs` there — but **not** `node_modules` (not installed for dev). So a naive "use resource_dir if the script exists" would pick a deps-less copy in dev and break the SDK agent where it works today. (The chosen design sidesteps this entirely: dev hard-returns to cwd before ever consulting `resource_dir`, so that dev resource copy is inert by design — the trap is closed structurally, not by an existence check.)
- `beforeBuildCommand`/npm scripts run from the **repo root**; the `resources` source `../sidecar/...` is resolved from **src-tauri/** (where `tauri.conf.json` lives). Two different cwds.
- `@anthropic-ai/claude-agent-sdk@0.1.0` has `dependencies: {}`; `--omit=optional` drops only the optional native `@img/sharp-*`. The 75 MB is **not pure-JS**: ~9 MB vendored Claude Code `cli.js` + ~55 MB native ripgrep binaries (`rg`/`ripgrep.node`) for **8 platform/arch variants** + ~3 MB jetbrains-plugin. It's arch-agnostic only because it bundles *every* arch (the runtime picks `rg` by platform). `--ignore-scripts` is safe + necessary (the SDK's `prepare` is a publish-guard; no needed postinstall). PR #20 added the `node_bin(env)`/`UAW_AGENT_NODE` seam (node-discovery hook).
- Consumers of the resolver: only `resolve_sdk_sidecar` (`start_sdk_session`, has `AppHandle`) and `resolve_sdk_models_sidecar` (`list_account_models`, lacks `AppHandle`).

---

## The design

### 1. Bundle the sidecar — `src-tauri/tauri.conf.json`
Add `bundle.resources` in **map form**: `"resources": { "../sidecar/claude-agent-sdk": "sidecar/claude-agent-sdk" }`. Leave `targets: "all"` (per-OS `--bundles` is Slice B). `externalBin` is the wrong mechanism (it's for single per-triple-named executables Tauri sidecar-spawns; this is a runnable Node tree executed by an external `node`) — `bundle.resources` + `resource_dir()` is idiomatic; no `capabilities` entry needed (`resource_dir()` + `std::process::Command` is plain Rust).

### 2. Install the sidecar deps before bundling — `package.json` (a wrapper script, NOT `beforeBuildCommand`)
`beforeBuildCommand` runs on **every** `tauri build` incl. the e2e's `tauri build --debug --no-bundle` (which collects no resources) — so a 75 MB `npm ci` there is waste + a network-flake risk on the e2e path. Instead:
- `"sidecar:install": "npm ci --ignore-scripts --omit=optional --prefix sidecar/claude-agent-sdk"` — cwd-independent via `--prefix`; `--ignore-scripts` closes the lifecycle-RCE surface; `--omit=optional` drops the native sharp.
- `"bundle": "pnpm sidecar:install && pnpm tauri build"` — **the bundling entry point** (the local proof + Slice B's CI call this; a raw `tauri build` does not install the sidecar — caught loudly by the post-build assertion below). `beforeBuildCommand` stays `"pnpm build"` (e2e untouched).

### 3. Resolve from `resource_dir` — `src-tauri/src/services/agent/mod.rs` (dev/release split, pure + testable)
Change the resolver to a pure, unit-testable function with a `dev` flag the callers compute from `cfg!(debug_assertions)`:
```rust
fn resolve_sidecar_script(env_var: &str, rel: &str, resource_dir: Option<&Path>, dev: bool) -> String {
    // 1. Env override (dev/e2e) always wins.
    if let Ok(v) = std::env::var(env_var) {
        let t = v.trim();
        if !t.is_empty() { return t.to_string(); }
    }
    if dev {
        // 2. DEV: the repo sidecar via cwd (its node_modules is the dev-installed working tree).
        return std::env::current_dir()
            .map(|d| d.join(rel).to_string_lossy().into_owned())
            .unwrap_or_else(|_| rel.to_string());
    }
    // 3. RELEASE: ONLY the bundled resource — NEVER cwd (cwd is the agent-writable worktree;
    //    a planted sidecar there would run with the account key → a script-hijack/key-exfil
    //    vector). A missing resource → a non-existent path → spawn fails closed; the post-build
    //    assertion guarantees a correctly-bundled app has it.
    resource_dir
        .map(|d| d.join(rel).to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("/nonexistent/uaw-bundled-sidecar/{rel}"))
}
```
- **Precedence: env → (dev: cwd | release: resource-only, fail-closed).** This (a) keeps dev working (cwd = the repo `sidecar/` with its installed deps — status quo; no 75 MB dev-copy reliance), (b) makes release use ONLY the read-only bundled resource (closing the cwd script-hijack), and (c) never silently uses cwd in a release binary.
- **Reuse `rel` for the resource join** (`resource_dir.join(rel)`) — `rel` is already `sidecar/claude-agent-sdk/index.mjs` and the map destination mirrors the source tree, so this reproduces the bundled layout with **no new `RESOURCE_SUBPATH` constant** (one source of truth).
- `resolve_sdk_sidecar(resource_dir: Option<&Path>)` / `resolve_sdk_models_sidecar(resource_dir: Option<&Path>)` pass `cfg!(debug_assertions)` as `dev`. Callers compute `app.path().resource_dir().ok()` and pass `.as_deref()`:
  - `start_sdk_session` already has `app: AppHandle`.
  - **`list_account_models` gains an injected `app: AppHandle`** (Tauri fills it by type; the frontend `invoke("list_account_models", { codingWorkspaceId, accountId })` is unchanged — same pattern as `start_agent_session`). Its `spawn_oneshot` **cwd stays as-is** (the cwd is the helper's working dir, orthogonal to script resolution — do not repoint it).
- The pure `Option<&Path>` seam keeps the resolver unit-testable (no `AppHandle` in `#[cfg(test)]`) — matches the file's `node_bin(env)`/`resolve_program` style.

### 4. Commit the sidecar lockfile — `sidecar/claude-agent-sdk/.gitignore`
Un-ignore `package-lock.json` (keep `node_modules/` ignored) and commit it; `npm ci` requires it. Regenerate it once via `npm install --package-lock-only` (records all platforms incl. the omitted sharp, so the lockfile carries full sha512 integrity even though sharp isn't installed). A lockfile change is a **reviewed security event covering the vendored payload** (the `cli.js` + ripgrep binaries a new version re-vendors), not just the version number.

---

## Security
- **Net hardening:** moving script resolution from `current_dir()` (the agent-writable worktree, in a packaged app) to the read-only `resource_dir` closes a local script-hijack/key-exfil vector (a planted `sidecar/.../index.mjs` would otherwise run with the bound account's `ANTHROPIC_API_KEY`). The dev/release split makes the cwd path **unreachable in a release binary** — a bundling regression fails closed (non-existent path → spawn error), it does not silently fall to cwd.
- Both resolver consumers move together: `list_account_models` injects the same key into `list-models.mjs`, so it carries the identical vector — its `AppHandle` + resource resolution land in this slice too.
- Supply chain: `npm ci --ignore-scripts` (no lifecycle RCE in the build) + a committed `package-lock.json` (per-tarball sha512) + exact-pinned `@anthropic-ai/claude-agent-sdk@0.1.0` is an acceptable v1 posture. The bundle adds no secret-at-rest (the key is injected per-run, masked from transcripts via `sdk::redact`). `rel` is hard-coded literals — no path-traversal/user-input surface.

## Verification
- **Rust unit test on the pure `resolve_sidecar_script`** (the precedence matrix — assert **full paths**, not `ends_with` which every branch satisfies): env override wins over a **present** `Some(seeded_dir)`; `dev=false` + `Some(dir)` with a seeded `<dir>/sidecar/claude-agent-sdk/index.mjs` → the path **contains the temp dir**; `dev=false` + `None` → a non-cwd sentinel (does **not** contain the cwd); `dev=true` → cwd (does **not** contain the temp dir). Seed via `std::env::temp_dir().join(new_id())` (the repo's existing FS-test idiom; no new dep). Use a unique env-var name per precedence case to avoid the shared-`UAW_AGENT_SDK_SIDECAR` multi-thread race.
- **Update the existing** `resolve_sdk_sidecar_prefers_env` / `resolve_sdk_models_sidecar_prefers_env` to call the new signature with `None` (they prove env-precedence, unaffected by resource_dir/dev).
- **Post-build presence assertion** (turns the silent-incomplete-bundle into a loud failure): the `bundle` script (and Slice B's CI) asserts, after `tauri build`, that the bundled app's Resources contain `sidecar/claude-agent-sdk/node_modules/@anthropic-ai/claude-agent-sdk/sdk.mjs` (a `test -f` against the platform bundle path; macOS for the local proof).
- **The Docker e2e is blind to this slice** (it sets `UAW_AGENT_SDK_SIDECAR`/`_MODELS` (precedence 1) + builds `--no-bundle`) — so it stays green but proves nothing about the bundled path; that's a stated gap, covered by the manual proof. `agent-sdk.e2e.ts:165` (the model-picker e2e) is the existing proof that the injected `AppHandle` is frontend-invisible.
- **The real proof — a manual local-build checklist** (on a **clean tree**: `rm -rf sidecar/claude-agent-sdk/node_modules` first, so the dev tree can't mask it): run `pnpm bundle`; `unset UAW_AGENT_SDK_SIDECAR UAW_AGENT_SDK_MODELS`; launch the bundled app **from a neutral cwd** (`cd /tmp && open .../UAW.app`) so a cwd hit can't mask a broken bundle; (1) a real SDK plan run **produces a feed**; (2) the **model picker populates** (proves `list-models.mjs` + the new `list_account_models` `AppHandle`). A `tauri dev` smoke (start an SDK session) confirms no dev regression.

## Out of scope → Slice B (release CI) + logged decisions
universal-mac (`--target universal-apple-darwin`), per-OS `--bundles` (replacing `targets:"all"`), the tag-triggered 3-job pipeline, prerelease + checksums, SHA-pinned actions, the cross-platform post-build assertion. Deferred: code-signing/notarization; bundling Node (Node-on-PATH stays the documented downloader prereq, reusing the `node_bin`/`UAW_AGENT_NODE` seam). **Logged, accepted:** the bundle is ~75 MB; **~40 MB is cross-arch ripgrep** (6 of 8 arches unused on a single-platform build) + ~3 MB jetbrains-plugin → a **Slice-B prune target**, not silence here. **Slice-B CI note:** distributables must be built in **release** (`pnpm bundle` → release `tauri build`). Because `dev = cfg!(debug_assertions)`, a hand-rolled `tauri build --debug` *bundle* would resolve the sidecar via cwd, not `resource_dir` — it fails safe (the dev tree, never an insecure path) but won't exercise the bundled path; CI must not ship a `--debug` distributable.

## Review findings incorporated
The silent incomplete-bundle (compile-time resource copy) + the cwd fallback re-opening the hijack in release → the **dev/release precedence split** (cwd dev-only; release resource-only-fail-closed) + a **post-build presence assertion** + a **clean-tree** proof · the `bundle` wrapper script (not `beforeBuildCommand`, which the `--no-bundle` e2e doesn't want) with `--prefix` (the repo-root vs src-tauri/ cwd nuance) · reuse `rel` (no new subpath constant) · the unit test asserts **full paths** + a `dev` param so the release branch is testable + the existing `prefers_env` tests updated to `None` · the 4-point manual checklist (env unset, real run produces a feed, neutral cwd, model picker) · `list_account_models` `AppHandle` + its cwd left alone · corrected "pure-JS → native ripgrep ×8 arches" + the ~40 MB prune logged for Slice B · lockfile already on disk (un-ignore + commit; `--package-lock-only` note; bump = reviewed-payload event) · `externalBin` category error + no capability entry recorded.
