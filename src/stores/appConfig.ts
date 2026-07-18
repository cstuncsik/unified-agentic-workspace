import { ref } from "vue";
import { defineStore } from "pinia";
import type { ITheme } from "@xterm/xterm";
import { getAppConfig } from "../api/appConfig";

// Kept in sync with `services/config.rs::default_theme()`. Duplicated JS-side so
// the pre-load terminal is correct without gating mount on the async load.
const DEFAULT_THEME: ITheme = {
  background: "#000000", foreground: "#cccccc", cursor: "#ffffff",
  black: "#000000", red: "#cd3131", green: "#0dbc79", yellow: "#e5e510",
  blue: "#2472c8", magenta: "#bc3fbc", cyan: "#11a8cd", white: "#e5e5e5",
  brightBlack: "#666666", brightRed: "#f14c4c", brightGreen: "#23d18b", brightYellow: "#f5f543",
  brightBlue: "#3b8eea", brightMagenta: "#d670d6", brightCyan: "#29b8db", brightWhite: "#ffffff",
};
const DEFAULT_FONT_SIZE = 13;

export const useAppConfig = defineStore("appConfig", () => {
  const terminal = ref<{ fontSize: number; theme: ITheme }>({
    fontSize: DEFAULT_FONT_SIZE,
    theme: DEFAULT_THEME,
  });
  const warning = ref<string | null>(null);
  let inflight: Promise<void> | null = null;

  function load() {
    if (!inflight) {
      inflight = getAppConfig()
        .then((cfg) => {
          terminal.value = cfg.terminal;
          warning.value = cfg.warning;
        })
        .catch(() => {
          /* keep the seeded defaults — a failed command must not blank the terminal */
        });
    }
    return inflight;
  }

  return { terminal, warning, load };
});
