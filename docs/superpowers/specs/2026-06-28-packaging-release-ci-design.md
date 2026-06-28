# Packaging Slice B — Release CI — Design

**Goal:** A pushed `v*` tag produces downloadable, **unsigned** installers for macOS (universal DMG), Windows (x64 NSIS), and Linux (AppImage + .deb) as a **draft** GitHub Release, with a per-OS-pruned sidecar (~75 MB → ~21–31 MB), SHA-256 checksums, and build-provenance attestation — for the owner to smoke-test and publish manually.

**Status:** Approved design (post 5-lens + codex review; findings folded in). Ready for an implementation plan. **Scope (user-confirmed):** all 3 OSes · unsigned v1 · full pipeline = publish + vendor prune + post-build assertion.

**Context:** Slice B of packaging, following merged Slice A (PR #23), which bundles the Node SDK sidecar via `bundle.resources` and resolves it from `resource_dir` in release / cwd in dev. The runtime needs **Node ≥18 on the user's PATH** (Node is not bundled in v1).

---

## 1. Pipeline — `.github/workflows/release.yml` (one workflow, mixed triggers)
```
on:
  push: { tags: ['v*'] }
  pull_request:                     # bit-rot guard: the prune test runs on PRs that touch it
    paths: ['.github/workflows/release.yml', 'scripts/prune-sidecar-vendor*', 'src-tauri/tauri.conf.json', 'sidecar/claude-agent-sdk/**']
permissions: {}                     # least-privilege FLOOR; jobs opt into contents:write
concurrency: { group: 'release-${{ github.ref }}', cancel-in-progress: false }
```
Four jobs. The release jobs are tag-only via `if: startsWith(github.ref, 'refs/tags/')`; `test-prune` runs on both (so a broken prune fails a PR *and* gates a release).

- **`test-prune`** (ubuntu-latest, no special perms): `node --check scripts/prune-sidecar-vendor.mjs` + `node --test scripts/prune-sidecar-vendor.test.mjs`. The fast, always-on guard.
- **`create-release`** (`if` tag; `needs: test-prune`; `permissions: contents: write`): assert the tag matches the bundle version, then create the **draft, prerelease** Release and emit its numeric id.
  ```bash
  test "v$(jq -r .version src-tauri/tauri.conf.json)" = "$GITHUB_REF_NAME" \
    || { echo "tag $GITHUB_REF_NAME != tauri.conf version"; exit 1; }
  test "v$(grep -m1 '^version' src-tauri/Cargo.toml | cut -d'\"' -f2)" = "$GITHUB_REF_NAME" || { echo "Cargo.toml version mismatch"; exit 1; }
  gh release create "$GITHUB_REF_NAME" --draft --prerelease --title "UAW $GITHUB_REF_NAME" --notes-file body.md
  id=$(gh api "repos/$GITHUB_REPOSITORY/releases" --jq ".[] | select(.tag_name==\"$GITHUB_REF_NAME\") | .id")
  echo "release_id=$id" >> "$GITHUB_OUTPUT"
  ```
  (`gh api …/releases` lists **drafts** — the by-tag REST endpoint does not. Passing the numeric `release_id` to the build jobs is the deterministic handshake; it sidesteps the draft-lookup-by-tag race.) `body.md` is written by a prior heredoc step (§5).
- **`build`** (matrix, `if` tag; `needs: create-release`; `permissions: contents: write`; `strategy.fail-fast: false`): per §2; uploads to `releaseId: ${{ needs.create-release.outputs.release_id }}`.
- **`finalize`** (ubuntu-latest, **no checkout**, `if` tag; `needs: build`; `permissions: { contents: write, id-token: write, attestations: write }`): `gh release download "$GITHUB_REF_NAME"` → `shasum -a 256 * > checksums.txt` → `gh release upload "$GITHUB_REF_NAME" checksums.txt` → `actions/attest-build-provenance` over the downloaded installers. (Release assets are **not** workflow artifacts — `gh`, never `download-artifact`.)

## 2. Build matrix (3 entries) — actions SHA-pinned
Each: checkout → pnpm/node-22/rust(+rust-cache) → `pnpm install --frozen-lockfile` → `pnpm sidecar:install` → **`node scripts/prune-sidecar-vendor.mjs --target <T>`** → `tauri-apps/tauri-action` (`releaseId` from job 3; `args` below). tauri-action runs the conf `beforeBuildCommand` (`pnpm build`, frontend-only) and bundles **whatever is in the tree at build time** — so the prior `sidecar:install` + prune steps are the *entire* mechanism that populates + shrinks the bundled sidecar; the prune's postcondition assertion (§3) is the guard that a clean/sparse tree can't silently ship a `node_modules`-less (→ resolver-fail-closed, silently dead) agent.

| Runner | `--target` | tauri-action `args` | Notes |
|---|---|---|---|
| `macos-latest` | `mac-universal` | `--target universal-apple-darwin --bundles dmg` | `rustup target add aarch64-apple-darwin x86_64-apple-darwin`; one DMG for Intel+Apple Silicon |
| `ubuntu-22.04` | `linux-x64` | `--bundles appimage,deb` | install the e2e webkit/gtk/appindicator/rsvg apt set (minus webdriver/xvfb); **22.04** = older glibc → wider AppImage/deb compat. *Note: 22.04 is on GitHub's deprecation track; migrate to 24.04 when pulled (newer glibc narrows compat — a conscious follow-up).* Keep the dep list in sync with `e2e.yml` (comment both). |
| `windows-latest` | `win-x64` | `--bundles nsis` | NSIS `.exe` |

`--bundles` overrides the conf per-run, so **`tauri.conf.json` is NOT changed** — `targets: "all"` stays (untouched: local `pnpm bundle` and the `--no-bundle` e2e are unaffected). `GITHUB_TOKEN` is wired into tauri-action's `env`.

## 3. Prune + assertion — `scripts/prune-sidecar-vendor.mjs` (Node, no deps)
Verified layout: `…/@anthropic-ai/claude-agent-sdk/vendor/ripgrep/<arch>/{rg,ripgrep.node}` for `arch ∈ {arm64-darwin, x64-darwin, x64-linux, arm64-linux, x64-win32}` (plus a `COPYING` file), and `vendor/claude-code-jetbrains-plugin/` (12 MB, inert). Runtime selects ripgrep by `${process.arch}-${process.platform}` → these exact dir names.

**Pure core (exported, unit-tested):**
```js
const KNOWN_ARCHES = ['arm64-darwin','x64-darwin','x64-linux','arm64-linux','x64-win32'];
function dirsToKeep(target) {            // KEEP-set polarity: unknowns are kept, not mis-deleted
  if (target === 'mac-universal') return ['arm64-darwin','x64-darwin'];   // BOTH — universal runs as either arch
  if (target === 'linux-x64')     return ['x64-linux'];
  if (target === 'win-x64')       return ['x64-win32'];
  throw new Error(`unknown prune target: ${target}`);                     // bad ARG → fail loud
}
const archesToDelete = (t) => KNOWN_ARCHES.filter(a => !dirsToKeep(t).includes(a));
```
**Side-effecting `pruneVendor(vendorRoot, target, {force})` (exported for the fixture test):**
1. **CI-scope guard** — refuse unless `process.env.CI` or `force`; print *"refusing to prune outside CI (would mutate the dev tree the SDK agent runs from in `pnpm dev`); restore with `pnpm sidecar:install`"*. (The prune is **CI-only**; local `pnpm bundle` stays unpruned — it ships the full sidecar, which is fine for the local proof. This closes the dev-tree-corruption footgun: the normal local path never prunes.)
2. **Root-guard** — `vendorRoot = realpathSync(vendor)`; assert it exists and ends with `…/@anthropic-ai/claude-agent-sdk/vendor`; every delete path must `startsWith(vendorRoot + sep)` (mirrors `index.mjs`'s fail-closed `withinWorktree`). Refuse otherwise — never `rm` a computed-empty/escaped path.
3. **Delete** only `ripgrep/<arch>` for `archesToDelete(target)` (the **known allowlist** — leaves `COPYING` and any unrecognized future dir untouched) + `claude-code-jetbrains-plugin` (delete-if-present; not assertion-fatal if a future SDK drops it).
4. **Postcondition assertion** (this IS the "post-build assertion", folded in) — for each kept arch, `ripgrep/<arch>/{rg,ripgrep.node}` exist; the package `sdk.mjs` exists; jetbrains is gone. Exit non-zero on any miss → catches over-prune AND a silently-missing/skipped `sidecar:install`.

Result: ~21 MB sidecar (Linux/Win, 1 arch) / ~31 MB (mac, 2 darwin arches). Saves ~44–54 MB per installer.

## 4. Node-on-PATH — a durable runtime error (`src-tauri/src/services/agent/sdk.rs`)
The release ships an app that needs Node on PATH; today a missing Node is an opaque spawn failure. Map `ErrorKind::NotFound` at the sidecar spawn site (`spawn` + `spawn_oneshot`) to a specific message: *"Node.js was not found on PATH. The SDK agent requires Node.js ≥18 (PTY agents are unaffected)."* This is the load-bearing contract (fires for installer users and source builders alike); the release-body note (§5) and a README line are the discovery surfaces.

## 5. Release output
- **Draft + prerelease, manual publish** — the owner downloads, smoke-tests the unsigned installers (incl. SDK-agent launch on each OS — the cross-OS bundle-copy proof, since AppImage/NSIS are packed and not peeked in CI), then publishes.
- **`body.md`** (heredoc in `create-release`): per-OS unsigned-install bypass (macOS right-click→Open / `xattr -dr com.apple.quarantine`; Windows SmartScreen → More info → Run anyway), **the Node ≥18 on PATH prerequisite**, the checksums note, and `gh attestation verify <file> --repo <owner>/<repo>` instructions.
- **`checksums.txt`** — relabeled honestly as a **transit-corruption** check, NOT tamper-evidence (a compromised release can regenerate it).
- **`actions/attest-build-provenance`** — the real integrity control for unsigned binaries (signed SLSA provenance in the Sigstore transparency log; not a swappable file). Requires a **public repo** (or GHAS); if unavailable, degrade to checksums-only with the relabel.

## Security
- **Least-privilege:** workflow `permissions: {}`; `contents: write` only on `create-release`/`build`/`finalize`; `id-token`+`attestations: write` only on `finalize`. Triggers are `push: tags` + `pull_request` (no `pull_request_target`; no untrusted-checkout write path).
- **SHA-pin every action in this workflow** (the only one building *shipped binaries*; a mutable tag = malware in the installer) while the CI gates stay `@v6`. Highest-risk first: `tauri-apps/tauri-action` (build + token) and `dtolnay/rust-toolchain` (pin a SHA, not `@stable` — a *branch*); then `pnpm/action-setup`, `swatinem/rust-cache`, `actions/checkout`, `actions/setup-node`, `actions/attest-build-provenance`. Comment each SHA with the tag it maps to. (Pinning ≠ transitive immutability — it pairs with the permissions floor.)
- **No secrets introduced** — the release build needs no API key (the SDK key is a *runtime* user-machine concern). `GITHUB_TOKEN` is the only credential, scoped per-job.
- **Supply chain preserved from Slice A:** the release path keeps `pnpm sidecar:install` = `npm ci --ignore-scripts --omit=optional` against the committed sha512 lockfile. Do **not** copy `sdk-sidecar.yml`'s `npm install` into the release path (consider aligning that smoke job to `npm ci` separately).

## Verification
- **`test-prune` unit test** (`node --test`, zero deps — the repo's sidecar is deliberately framework-free; do NOT add vitest): `dirsToKeep`/`archesToDelete` with **independently-written literals** (not derived from the same map). Cases: `mac-universal` keeps *exactly* `{arm64-darwin, x64-darwin}`; `linux-x64`→`{x64-linux}`; `win-x64`→`{x64-win32}`; an unknown target **throws**; `archesToDelete` never returns all five; jetbrains is dropped for every valid target. The on-runner postcondition assertion alone can't catch a mac-universal that dropped the *other* darwin arch (the mac runner is arm64 → Intel users break) — the table test is load-bearing for exactly that.
- **`pruneVendor` fixture test** (`node --test`): build a tmpdir fake `vendor/` (all 5 arch dirs + `COPYING` + jetbrains, empty marker files), run the destructive path for a target, assert exactly the right survivors + that `COPYING`/unknown dirs survive + that a missing kept dir makes the postcondition exit non-zero. Exercises deletion deterministically without the 75 MB download.
- **Pipeline dry-run:** push a throwaway `v0.0.0-rc1` tag → produces a **draft** (never public) to inspect, then delete. (`workflow_dispatch` is not added — it can't exercise the tag handshake; the throwaway-tag draft is the canonical pre-flight.)
- **Manual cross-OS publish smoke:** before publishing, launch each installer, confirm the SDK agent starts (proves the bundled+pruned sidecar resolves end-to-end on all 3 OSes — the coverage the CI source-assertion can't reach for packed bundles).

## Out of scope (later)
signing/notarization · bundling Node · auto-publish (stays manual) · the Tauri updater/`latest.json` · a single-OS build-smoke on PR (the prune unit test is the v1 bit-rot guard) · pruning `cli.js`/`yoga.wasm` (needed) · migrating off `ubuntu-22.04` (a conscious follow-up when the image is retired).

## Review findings incorporated
BLOCKERs: `gh release download/upload` not `download-artifact` (release assets ≠ workflow artifacts) · `permissions: {}` floor + per-job `contents: write` · CI-scope the prune so it can't corrupt the dev tree the dev resolver reads · the build-tree→bundle mechanism stated explicitly + guarded by the postcondition. MAJORs: the tag↔conf(+Cargo) version gate · attestation as the real integrity control + checksums relabeled · prune root-guard (realpath) + keep-set polarity + known-arch allowlist (don't nuke `COPYING`) + unknown-arg-throws · manual cross-OS launch smoke for the packed-bundle copy gap · the `test-prune` PR job so `release.yml` can't bit-rot · the `ubuntu-22.04` deprecation note. Resolved contradictions: keep the `dirsToKeep` unit test (the on-runner assertion misses the cross-arch mac-universal case) but via Node `--test`, not vitest · keep an explicit `create-release` (deterministic `releaseId` handshake, draft-safe) but make `finalize` checkout-free. Cuts: dropped the `tauri.conf.json targets` edit (use `--bundles`) · dropped the mac-only `.app` peek (asymmetric — can't cover packed Linux/Win; the manual smoke covers all three). Plus: the durable Node-on-PATH spawn-site error; `sidecar:install` keeps `npm ci`.
