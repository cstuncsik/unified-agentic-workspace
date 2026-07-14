<script setup lang="ts">
import { useUpdater } from "../composables/useUpdater";

const { available, installing, installAndRestart, dismiss } = useUpdater();
</script>

<template>
  <div v-if="available" class="update-banner" role="status" data-testid="update-banner">
    <div class="update-banner__row">
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
    <details v-if="available.body" class="update-banner__notes">
      <summary>What's new</summary>
      <pre>{{ available.body }}</pre>
    </details>
  </div>
</template>

<style scoped>
.update-banner {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  padding: 0.5rem 1rem;
  background: var(--re-color-bg-muted);
  border-bottom: 1px solid var(--re-color-border);
}

.update-banner__row {
  display: flex;
  gap: 0.75rem;
  align-items: center;
}

.update-banner__notes summary {
  cursor: pointer;
  font-size: 0.85rem;
  color: var(--re-color-text-muted);
}

.update-banner__notes pre {
  margin: 0.5rem 0 0;
  max-height: 8rem;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: inherit;
  font-size: 0.85rem;
  color: var(--re-color-text);
}
</style>
