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
