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
let dbDir: string | undefined;

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
    // Give every run an isolated, empty database so assertions are deterministic.
    dbDir = fs.mkdtempSync(path.join(os.tmpdir(), "uaw-e2e-"));
    process.env.UAW_DB_PATH = path.join(dbDir, "uaw.sqlite");
  },

  beforeSession: () => {
    // tauri-driver listens on :4444 and forwards to the platform WebDriver
    // (WebKitWebDriver on Linux). It inherits UAW_DB_PATH from this process.
    tauriDriver = spawn("tauri-driver", [], {
      stdio: [null, process.stdout, process.stderr],
    });
  },

  afterSession: () => {
    tauriDriver?.kill();
  },

  onComplete: () => {
    if (dbDir) fs.rmSync(dbDir, { recursive: true, force: true });
  },
};
