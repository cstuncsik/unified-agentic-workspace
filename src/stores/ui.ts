import { ref } from "vue";
import { defineStore } from "pinia";
import { applyTheme, getStoredTheme, type ThemeMode } from "../theme";

export const useUiStore = defineStore("ui", () => {
  const theme = ref<ThemeMode>(getStoredTheme());

  function setTheme(mode: ThemeMode) {
    theme.value = mode;
    applyTheme(mode);
  }

  return { theme, setTheme };
});
