<script setup lang="ts">
import { useUiStore } from "../stores/ui";
import type { ThemeMode } from "../theme";

const ui = useUiStore();
const modes: { value: ThemeMode; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

function onChange(event: Event) {
  ui.setTheme((event.target as HTMLSelectElement).value as ThemeMode);
}
</script>

<template>
  <label class="re-field theme-toggle">
    <span class="re-field__label">Theme</span>
    <select
      class="re-select"
      data-size="sm"
      aria-label="Theme"
      :value="ui.theme"
      @change="onChange"
    >
      <option v-for="m in modes" :key="m.value" :value="m.value">{{ m.label }}</option>
    </select>
  </label>
</template>

<style scoped>
.theme-toggle {
  margin-top: auto;
}
</style>
