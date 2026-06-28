import { realpathSync, existsSync, rmSync } from "node:fs";
import { join, sep } from "node:path";
import { fileURLToPath } from "node:url";

const SIDECAR_PKG = "sidecar/claude-agent-sdk/node_modules/@anthropic-ai/claude-agent-sdk";
const KNOWN_ARCHES = ["arm64-darwin", "x64-darwin", "x64-linux", "arm64-linux", "x64-win32"];

// Pure core — which ripgrep arch dirs to keep for a build target.
export function dirsToKeep(target) {
  if (target === "mac-universal") return ["arm64-darwin", "x64-darwin"]; // runs as either arch
  if (target === "linux-x64") return ["x64-linux"];
  if (target === "win-x64") return ["x64-win32"];
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
  const expected = join("@anthropic-ai", "claude-agent-sdk", "vendor");
  if (!root.endsWith(sep + expected) && root !== expected) {
    throw new Error(`refusing to prune: not a sidecar vendor root: ${root}`);
  }
  // Defense-in-depth: unreachable given the root-guard + the hardcoded-literal rels below
  // (no external input reaches here); guards a future caller that passes a dynamic rel.
  const within = (rel) => {
    const p = join(root, rel);
    if (p !== root && !p.startsWith(root + sep)) {
      throw new Error(`refusing to delete outside vendor root: ${p}`);
    }
    return p;
  };
  for (const arch of archesToDelete(target)) {
    rmSync(within(join("ripgrep", arch)), { recursive: true, force: true });
  }
  rmSync(within("claude-code-jetbrains-plugin"), { recursive: true, force: true });

  // Postcondition: kept arches intact, SDK entry present, jetbrains gone.
  for (const arch of dirsToKeep(target)) {
    const rgBin = arch.includes("win32") ? "rg.exe" : "rg"; // win32 ships rg.exe, others bare rg
    for (const f of [rgBin, "ripgrep.node"]) {
      if (!existsSync(join(root, "ripgrep", arch, f))) {
        throw new Error(`prune postcondition failed: missing ripgrep/${arch}/${f}`);
      }
    }
  }
  // `sdk.mjs` is the @anthropic-ai package's own entry (one dir up from vendor) — its presence
  // proves `sidecar:install` ran. (The repo's own index.mjs is always present in a checkout.)
  if (!existsSync(join(root, "..", "sdk.mjs"))) {
    throw new Error("prune postcondition failed: missing sdk.mjs (sidecar not installed?)");
  }
  if (existsSync(join(root, "claude-code-jetbrains-plugin"))) {
    throw new Error("prune postcondition failed: jetbrains plugin still present");
  }
}

function argFor(argv, flag) {
  const i = argv.indexOf(flag);
  if (i === -1 || i + 1 >= argv.length) throw new Error(`missing ${flag} <value>`);
  return argv[i + 1];
}

function main(argv) {
  const target = argFor(argv, "--target");
  const force = argv.includes("--force");
  // CI-only: the prune mutates the tree `pnpm dev` resolves the sidecar from (mod.rs dev branch).
  if (!process.env.CI && !force) {
    throw new Error(
      "refusing to prune outside CI (would mutate the dev tree the SDK agent runs from in `pnpm dev`). " +
        "Pass --force to override; restore afterward with `pnpm sidecar:install`.",
    );
  }
  const vendor = join(SIDECAR_PKG, "vendor");
  if (!existsSync(vendor)) {
    throw new Error(`sidecar vendor not found at ${vendor} — run \`pnpm sidecar:install\` first`);
  }
  pruneVendor(vendor, target);
  console.log(`pruned sidecar vendor for ${target} (kept ${dirsToKeep(target).join(", ")})`);
}

// Run as CLI only when invoked directly (not when imported by the test).
if (
  process.argv[1] &&
  realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))
) {
  try {
    main(process.argv.slice(2));
  } catch (e) {
    console.error(String(e?.message ?? e));
    process.exit(1);
  }
}
