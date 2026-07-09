# Auto-updater — Design

**Goal:** UAW checks for a newer release on launch and can update itself in place, so users don't manually re-download each version. First updater-enabled release = **v0.1.2**.

**Status:** Revised post 6-lens + codex review (findings folded in — the biggest: use `tauri-action`'s native manifest instead of a hand-rolled one). **Two decisions still ASSUMED (author away); confirm/veto before implementation:** ① releases published as **full (non-prerelease)** so the simple GitHub feed works; ② **startup check + dismissable prompt** (+ a manual check). Both recommended + cheaply reversible. Implementation is gated on the author's approval + generating the signing key.

**Context:** Tauri 2 (updater supported, not yet wired). Ships via the tag-triggered `release.yml` (3-OS matrix → `tauri-action` → draft → manual publish; `finalize` adds checksums + attestation). Chosen now for the **"add early or strand users" property** — only builds *with* the updater can auto-update, so every prior release strands its users; v0.1.1 is the one stranded version, v0.1.2+ auto-update.

---

## Design

### 1. Signing key (update integrity — the real control)
`tauri signer generate` → a minisign keypair. The **public key** goes in `tauri.conf.json` (`plugins.updater.pubkey`, committed); the **private key** + password become repo secrets `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (the author generates + `gh secret set`s them — a subagent can't). The updater installs **only** artifacts signed by this key, so a compromised release can't push *attacker-authored* code without the private key. (`.gitignore` gains `*.key` as defense-in-depth; the key file lives in `$HOME`, outside the repo.)

### 2. `src-tauri/tauri.conf.json`
- `bundle.createUpdaterArtifacts: true` — `tauri build` emits the signed update artifact per platform alongside the installer.
- `plugins.updater`: `{ "pubkey": "<generated>", "endpoints": ["https://github.com/cstuncsik/unified-agentic-workspace/releases/latest/download/latest.json"] }`. `/releases/latest/` resolves the latest **non-prerelease** release (decision ①).
- **Version → `0.1.2`** here AND in `src-tauri/Cargo.toml` `[package].version` AND `package.json` — all three (the release version-gate asserts `tag == tauri.conf == Cargo.toml`; a missed `Cargo.toml` bump fails `create-release`).

### 3. Plugins, capabilities, and the e2e gate (`src-tauri`)
- Add `tauri-plugin-updater` + `tauri-plugin-process` (Cargo + JS `@tauri-apps/plugin-updater`/`-process`); register both in `lib.rs`.
- `capabilities/default.json`: add `"updater:default"` + `"process:allow-restart"` (the narrow grant — not `process:default`).
- **`updater_enabled()` command** — returns `std::env::var("UAW_DISABLE_UPDATER").is_err()`. The e2e harness sets `UAW_DISABLE_UPDATER=1`, so the startup auto-check is a deterministic no-op there (see §4/Testing). The Rust updater fetch is server-side, so the webview CSP `connect-src` is untouched.

### 4. Update flow — frontend-driven, layout-safe, e2e-gated
- **`useUpdater.ts`** — a module-singleton composable (module-scope `ref`s + a non-reactive `let pending: Update | null`), mirroring the existing `useConfirm.ts`/`ConfirmDialog.vue` precedent (this is the repo's convention for App-root singletons — NOT a Pinia store). Exposes `available`, `installing`, `checkForUpdate({silent})`, `installAndRestart()`, `dismiss()`. `check()` errors are swallowed; on a manual re-check that returns `null` it clears `available` (else a stale banner + dead button persists).
- **`UpdateBanner.vue`** — a small component using the **design system**: `class="re-button" data-variant="brand|ghost"` buttons and `--re-color-*` tokens (NOT invented vars), `role="status"` on the banner (async announcement). Rendered at App root.
- **Layout:** the banner must **span the full width** — the root `.app` is `display:grid; grid-template-columns:240px 1fr; height:100vh`, so a bare first-child banner lands in the sidebar cell and breaks the layout. Fix: `.update-banner { grid-column: 1 / -1 }` + the grid gains `grid-template-rows: auto 1fr` (or wrap the grid in an outer flex column with the banner on top).
- **Startup check (gated):** in `App.vue` `onMounted`, `if (await invoke("updater_enabled")) void updater.checkForUpdate({ silent: true })` — so it never runs (never renders a banner) under the e2e's `UAW_DISABLE_UPDATER`. `silent` = no toast when up-to-date / on error, only surface a real update.
- **Manual "Check for updates"** (decision ② — a `re-button data-variant="ghost"` by `ThemeToggle`): `checkForUpdate({ silent: false })` → toasts "You're on the latest version" / "Update check failed" via the existing `useToast`. On accept: `pending.downloadAndInstall()` → `relaunch()`.

### 5. Release CI — sign, publish as full release (`release.yml`)
- **`create-release`:** drop `--prerelease` (keep `--draft`; the draft → manual-publish review gate stays). Update the now-stale "pre-release" wording in the step name + `body.md`.
- **`build` matrix:** add `env: TAURI_SIGNING_PRIVATE_KEY` + `_PASSWORD` (from secrets) on the `tauri-action` step so `createUpdaterArtifacts` signs. **Leave `includeUpdaterJson` at its default `true`** — `tauri-action` builds `latest.json` from build **metadata** (no fragile filename-suffix guessing), natively writes **both** `darwin-aarch64` + `darwin-x86_64` at the same universal artifact, sets `pub_date`/`notes`, reads each `.sig`, and **merges** across the matrix (read-modify-write, not clobber). A rare RMW race is caught by the draft→manual-publish gate + a mandatory pre-publish "all four platform keys present in `latest.json`" check (add `strategy.max-parallel: 1` if a deterministic serialization is preferred).
- **`finalize`:** unchanged — it downloads all assets (now incl. `latest.json`) and checksums + attests them. (No hand-rolled manifest assembly — deleted per the review; `tauri-action` owns it.)

### 6. Per-platform reach (documented)
Auto-update covers **macOS (universal DMG → `.app.tar.gz`)**, **Windows (NSIS)**, **Linux AppImage**. **`.deb` users do NOT auto-update** (grab the new `.deb`) — the release notes say so.

## Security
- **Signature chain (sound):** the committed `pubkey` + the CI-secret private key mean the app installs only updates minisign-signed by us — an attacker who compromises a GitHub release but lacks the private key can only supply a bogus signature, which fails verification. Stronger than `checksums.txt`.
- **Precision (not overstated):** the manifest's `version`/`url` are **not** signed (only the artifact is), so a release-write attacker could steer clients to an *older, legitimately-signed but vulnerable* build (Tauri's gate only blocks going *below* current). The updater still **can't run attacker-authored code** — the publish path (draft → manual publish, release-write token) is the rollback trust boundary. Don't claim "tamper-proof against any compromised release."
- **Key in CI:** the signing env is on the `tauri-action` **step** only — earlier steps (`pnpm install`, `sidecar:install --ignore-scripts`, prune) never see it. It IS in-env during the build (transitive `build.rs`/vite plugins can read it) — the standard Tauri flow, accepted; a leak = RCE-on-every-install, so treat the key as crown-jewel. `finalize` never receives it. The key is never committed (generated to `$HOME`, `git add` lists only conf/caps/lib/Cargo; `.gitignore` gains `*.key`).
- **The pubkey↔private-key coupling fails INVISIBLY** (a mismatch → healthy-looking feed, every client rejects the install) — the cross-version smoke is the **only** catch, so it's a **hard pre-publish gate**. **Key rotation strands all existing clients** (they hold the old pubkey) — treat rotation like the v0.1.1 note (old clients update manually once); documented in the release runbook.
- Capabilities minimal; single HTTPS endpoint; the startup check is fire-and-forget + error-swallowed (no startup hang/DoS).

## Testing
- **No custom manifest code to unit-test** — `tauri-action` owns `latest.json` (delegating to a tested library that derives platforms from metadata is strictly more robust than hand-rolled suffix matching, which the review found already wrong for Windows NSIS). The Rust plugin registration + the trivial frontend composable have no meaty pure core — covered by typecheck + the manual smoke (state this honestly; do NOT add a JS unit runner for it).
- **e2e stays green — verified, not assumed:** the startup check is gated off by `UAW_DISABLE_UPDATER=1` (set in `wdio.conf.ts`) → no banner renders. Add a positive assertion to an existing spec that `[data-testid="update-banner"]` is **absent**, and **run** `pnpm e2e:docker` on the branch to confirm 12/12 (don't assert it as fact — the container has egress, and an ungated check + a version delta would render the banner mid-suite and break the layout-sensitive `smoke.e2e.ts`).
- **Manual cross-version smoke — the real proof** (CI can't simulate a version delta), on **all three OSes** (Windows + Linux are the least-proven), for v0.1.2:
  - Confirm the published release has the updater artifacts + `.sig`s + a `latest.json` with **all four** platform keys (the key-pair + platform gate).
  - Launch a v0.1.1-or-lower build → banner → Update & Restart → it downloads, **verifies the signature**, installs, relaunches into v0.1.2.
  - **Negative (rejection) check** — the security control's actual failure mode: point the updater at a manifest whose signature is corrupted / signed with a different key → confirm `downloadAndInstall()` **fails** (the "Update failed to install" toast, no relaunch). "Verifies the signature" must be *proven*, not asserted.
- **Local `pnpm bundle` now requires `TAURI_SIGNING_PRIVATE_KEY`** (`createUpdaterArtifacts` → "public key found, but no private key") — documented; `e2e:build`'s `--no-bundle` is unaffected.

## Out of scope
Code-signing/notarization (the updater's minisign is independent of OS code-signing) · `.deb`/`.rpm` auto-update · a dynamic/self-hosted update server · update channels (stable/beta) · delta updates · in-app release-notes rendering.

## Decisions incorporated (assumed — pending author confirmation)
① **Full-release feed** (drop `--prerelease`) → the simple `/releases/latest/download/latest.json`. ② **Startup check + dismissable banner + manual check**, not silent. Both cheaply reversible.

## Review findings incorporated
**Keystone (delete code):** use `tauri-action`'s native `includeUpdaterJson` (metadata-based, both-darwin native, merges across the matrix) instead of a hand-rolled `build-latest-json.mjs` + `finalize` assembly — dissolves the **Windows `-setup.exe`-vs-`.nsis.zip` BLOCKER**, the Linux-suffix risk, and the "suffix unverifiable / not-gated / untested-glue" test gaps. **BLOCKERs:** bump `Cargo.toml` version (the release gate) · the banner must span the grid (`grid-column:1/-1` + rows), not sit in the sidebar cell. **MAJORs:** design-system integration (`re-button` + `--re-color-*` + `role="status"`) · gate + assert the startup check for the e2e (a real flake path) + RUN the e2e · the smoke must prove *rejection* of a bad signature and cover *all three* OSes · document the pubkey/key coupling as a hard smoke-gate + rotation-strands. **MINORs:** local `pnpm bundle` needs the key · the `useUpdater` stale-banner `else` fix · soften the "tamper-proof" rollback wording · `.gitignore *.key` · stale "pre-release" copy. Module-singleton `useUpdater` + split `UpdateBanner` **kept** (matches `useConfirm`/`ConfirmDialog`, not Pinia).
