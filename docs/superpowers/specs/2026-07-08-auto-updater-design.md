# Auto-updater — Design

**Goal:** UAW checks for a newer release on launch and can update itself in place, so users don't manually re-download each version. First updater-enabled release = **v0.1.2**.

**Status:** Draft design — **two decisions ASSUMED (author away); confirm/veto before implementation:** ① releases published as **full (non-prerelease)** so the simple GitHub feed works; ② **startup check + dismissable prompt** (+ a manual check), not silent. Both are the recommended options and are cheaply reversible (config). Ready for an implementation plan; implementation is gated on the author's approval + adding the signing secret.

**Context:** Tauri 2 (updater supported, not yet wired). Ships via the tag-triggered `release.yml` (3-OS matrix → tauri-action → draft → manual publish; finalize adds checksums + attestation). Chosen now because the updater has an **"add early or strand users" property** — only builds *with* the updater can auto-update, so every release before it leaves its users on manual re-download. v0.1.1 (already public, no updater) is the one stranded version; v0.1.2+ auto-update.

---

## Design

### 1. Signing key (update integrity — the real control)
`tauri signer generate` → a minisign keypair. The **public key** goes in `tauri.conf.json` (`plugins.updater.pubkey`, committed); the **private key** + its password become repo secrets `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (the author adds them — I generate the pair and print the private key for them to paste; I do NOT set repo secrets autonomously). The updater only installs artifacts signed by this key → a compromised release can't push a malicious update without the private key. This is a stronger integrity guarantee than the release's `checksums.txt` (which a compromised release could regenerate).

### 2. `src-tauri/tauri.conf.json`
- `bundle.createUpdaterArtifacts: true` — makes `tauri build` emit the signed update artifact per platform alongside the installer (`.app.tar.gz`+`.sig` on macOS, the NSIS `-setup.exe`+`.sig` on Windows, the `.AppImage`+`.sig` on Linux). The DMG/deb still ship for fresh installs.
- `plugins.updater`: `{ "pubkey": "<generated>", "endpoints": ["https://github.com/cstuncsik/unified-agentic-workspace/releases/latest/download/latest.json"] }`. `/releases/latest/` resolves the latest **non-prerelease** release (hence decision ①).
- `bundle.targets` stays as-is (per-OS `--bundles` in CI); `createUpdaterArtifacts` adds the update artifacts without changing the installer set.

### 3. `src-tauri` plugins + capabilities
- Add `tauri-plugin-updater` and `tauri-plugin-process` (Cargo + the JS `@tauri-apps/plugin-updater` / `@tauri-apps/plugin-process`). Register both in `lib.rs` (`.plugin(tauri_plugin_updater::Builder::new().build())`, `.plugin(tauri_plugin_process::init())`).
- `capabilities/default.json`: add `"updater:default"` (check/download/install) and `"process:allow-restart"` (relaunch after install) to `permissions`.

### 4. Update flow — frontend-driven (`@tauri-apps/plugin-updater` + `plugin-process`)
The check/prompt/install lives in the frontend (Vue), keeping the UI logic where the rest of the app's UI is:
- **On app mount**, call `check()` (from `plugin-updater`). If it returns an `Update` (newer version on the feed), surface a **dismissable prompt** via the existing toast/notification mechanism: *"UAW <version> is available — Update & Restart"* + a Dismiss. If `null`, do nothing (silent when up-to-date).
- **On accept:** `update.downloadAndInstall()` (optionally show progress from its events), then `relaunch()` (from `plugin-process`).
- **Manual check:** a "Check for updates" action (in Settings/Providers or a small header affordance) runs the same `check()` and toasts either the update prompt or "You're on the latest version."
- **Failure:** any check/download error is swallowed to a quiet, non-blocking toast ("Update check failed") — never blocks app use. A small `useUpdater` composable owns this (pure-ish: the check→state mapping is unit-testable; the plugin calls are the thin edge).

### 5. Release CI — sign + aggregate the manifest (`release.yml`)
- **`create-release`:** drop `--prerelease` (keep `--draft`). The draft → manual-publish gate stays (pre-publish review); on publish it becomes the latest **full** release, which `/releases/latest/` serves. (Decision ①.)
- **`build` matrix:** add `env: TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (from secrets) so `createUpdaterArtifacts` signs. Set tauri-action `includeUpdaterJson: false` — do NOT let each matrix job upload its own single-platform `latest.json` (3 jobs would clobber/race each other's manifest).
- **`finalize`:** after the builds, assemble ONE `latest.json` from the platforms' `.sig` files + their release asset URLs and upload it (analogous to the existing checksums step; deterministic, no matrix race). Tauri's updater matches the manifest by the running app's `{os}-{arch}` — there is **no `universal` arch key**, so the universal-mac artifact must be listed under **BOTH** `darwin-aarch64` **and** `darwin-x86_64` (same url + signature), or Macs never match. Manifest: `{ version, notes, pub_date, platforms: { "darwin-aarch64": {signature, url}, "darwin-x86_64": {signature, url}, "windows-x86_64": {signature, url}, "linux-x86_64": {signature, url} } }` — the two darwin keys point at the *same* universal `.app.tar.gz` url + sig; URLs are the release's updater-artifact download URLs. A pure **`buildLatestJson(version, notes, entries[])`** helper (Node, zero-dep) does the assembly — unit-tested; finalize reads each `.sig`'s contents + the asset URLs and calls it.

### 6. Per-platform reach (documented)
Auto-update covers **macOS (universal DMG install → `.app.tar.gz` update)**, **Windows (NSIS)**, and **Linux AppImage**. **`.deb` users do NOT auto-update** (package-manager territory) — they update via a new `.deb`; the release notes say so.

## Security
The updater pubkey (committed) + the private signing key (repo secret only) mean the app installs **only** updates signed by us — tamper-proof even against a compromised GitHub release (stronger than `checksums.txt`). No new runtime secret in the app; the private key lives only in CI. The feed is HTTPS GitHub. `updater:default` + `process:allow-restart` are the minimal capabilities. `createUpdaterArtifacts` doesn't change what the installers contain (the sidecar bundling + prune from Slice A/B are untouched).

## Testing
- **`buildLatestJson` unit test** (`node --test`, zero-dep, like the prune script): correct manifest shape; **all four platform keys present incl. both `darwin-aarch64` + `darwin-x86_64` mapping to the same universal artifact** (the mac-match correctness case); URLs + signatures placed correctly; a missing platform → a clear error (don't ship a half-manifest that strands an OS).
- **`useUpdater` composable** — unit-test the pure check→prompt-state mapping (update-available vs up-to-date vs error) with the plugin mocked; assert it never throws into the app.
- **Rust:** the plugin registration compiles + `cargo test`/clippy stay green (no logic to unit-test in the registration itself).
- **The real proof — manual cross-version update test** (CI can't simulate a version delta): build a local vX+1 as a draft; run a vX build (or a locally-lowered version) pointed at the feed; confirm it detects, downloads, verifies the signature, installs, and relaunches into vX+1 on macOS. Note the **v0.1.1-stranded** caveat (v0.1.1 has no updater; its users grab v0.1.2 manually once) in the release notes + the PR.
- **e2e:** the Docker e2e can't exercise a real update (no version delta, `--no-bundle`); confirm it stays green (the updater plugin registration + the frontend `check()` guarded so it no-ops off-bundle / on failure).

## Out of scope
Code-signing/notarization (separate; the updater's minisign is independent of OS code-signing) · `.deb`/`.rpm` auto-update · a self-hosted/dynamic update server (GitHub Releases is the feed) · update channels (stable/beta) · delta updates · in-app release-notes rendering beyond the version + a short note.

## Decisions incorporated (assumed — pending author confirmation)
① **Full-release feed** (not prerelease) → the simple `/releases/latest/download/latest.json` endpoint, zero extra infra; `create-release` drops `--prerelease`, keeps the draft gate. ② **Startup check + dismissable prompt** + a manual check (not silent, not manual-only) — a dev keeps control of when the tool restarts. Both cheaply reversible if vetoed: ① is the endpoint + one CI flag; ② is the frontend composable's trigger.
