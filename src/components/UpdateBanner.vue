<script setup lang="ts">
import { useUpdater } from "../composables/useUpdater";

const { available, installing, installAndRestart, dismiss } = useUpdater();
</script>

<template>
  <div v-if="available" class="update-banner" role="status" data-testid="update-banner">
    <span>UAW {{ available.version }} is available.</span>
    <button
      type="button"
      class="re-button"
      data-variant="brand"
      :disabled="installing"
      @click="installAndRestart"
    >
      {{ installing ? "Updating…" : "Update & Restart" }}
    </button>
    <button
      type="button"
      class="re-button"
      data-variant="ghost"
      :disabled="installing"
      @click="dismiss"
    >
      Dismiss
    </button>
  </div>
</template>

<style scoped>
.update-banner {
  display: flex;
  gap: 0.75rem;
  align-items: center;
  padding: 0.5rem 1rem;
  background: var(--re-color-bg-muted);
  border-bottom: 1px solid var(--re-color-border);
}
</style>
