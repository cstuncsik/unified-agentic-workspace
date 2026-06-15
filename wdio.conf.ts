import { spawn, type ChildProcess } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";
import os from "node:os";
import fs from "node:fs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// The real built debug binary the WebDriver session drives. Overridable so CI or
// a packaged path can point elsewhere; defaults to the cargo debug output.
const application = process.env.UAW_BIN ?? path.resolve(__dirname, "src-tauri/target/debug/uaw");

let tauriDriver: ChildProcess | undefined;
let sessionDir: string | undefined;

export const config: WebdriverIO.Config = {
  hostname: "127.0.0.1",
  port: 4444,
  specs: ["./e2e/specs/**/*.e2e.ts"],
  // tauri-driver proxies a single native WebDriver session; parallel windows are not supported.
  maxInstances: 1,
  capabilities: [
    {
      // Consumed by tauri-driver, not part of the standard capability set.
      "tauri:options": { application },
    } as unknown as WebdriverIO.Capabilities,
  ],
  logLevel: "info",
  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: {
    ui: "bdd",
    // The app cold-starts a real window and SQLite db, so allow generous time.
    timeout: 120_000,
  },

  onPrepare: () => {
    if (!fs.existsSync(application)) {
      throw new Error(
        `Tauri binary not found at ${application}. Build it first with "pnpm e2e:build".`,
      );
    }
  },

  // Runs in each spec file's worker process. Give every spec a FRESH db +
  // worktrees dir so specs are hermetic (no state leaks across spec files).
  // The dir is derived per-worker from the spec path because module state set in
  // onPrepare (the launcher process) is not shared with workers. The app reads
  // these env vars at startup; set them before spawning tauri-driver, which
  // launches the app and inherits this environment.
  beforeSession: (_config, _capabilities, specs: string[]) => {
    const specName = path.basename(specs?.[0] ?? "spec").replace(/[^a-z0-9]+/gi, "-");
    sessionDir = fs.mkdtempSync(path.join(os.tmpdir(), `uaw-e2e-${specName}-`));
    process.env.UAW_DB_PATH = path.join(sessionDir, "uaw.sqlite");
    process.env.UAW_WORKTREES_DIR = path.join(sessionDir, "worktrees");

    // tauri-driver listens on :4444 and forwards to the platform WebDriver.
    tauriDriver = spawn("tauri-driver", [], {
      stdio: [null, process.stdout, process.stderr],
    });
  },

  afterSession: () => {
    tauriDriver?.kill();
    if (sessionDir) {
      try {
        fs.rmSync(sessionDir, { recursive: true, force: true });
      } catch {
        // best-effort; the dir is in the OS temp space
      }
    }
  },
};
