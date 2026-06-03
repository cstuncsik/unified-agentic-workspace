<script setup lang="ts">
import { computed } from "vue";
import { useWorkspacesStore } from "../stores/workspaces";

const store = useWorkspacesStore();

const selectedId = computed({
  get: () => store.currentId ?? "",
  set: (id: string) => store.select(id),
});

async function newWorkspace() {
  const name = window.prompt("New workspace name")?.trim();
  if (name) {
    await store.create(name);
  }
}
</script>

<template>
  <div class="switcher">
    <span class="switcher__label">Workspace</span>
    <div class="switcher__row">
      <select
        v-model="selectedId"
        class="switcher__select"
        :disabled="store.list.length === 0"
        aria-label="Select workspace"
      >
        <option v-for="ws in store.list" :key="ws.id" :value="ws.id">
          {{ ws.name }}
        </option>
      </select>
      <button
        class="switcher__new"
        type="button"
        title="New workspace"
        aria-label="New workspace"
        @click="newWorkspace"
      >
        +
      </button>
    </div>
  </div>
</template>

<style scoped>
.switcher {
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}

.switcher__label {
  font-size: 0.7rem;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--uaw-muted);
}

.switcher__row {
  display: flex;
  gap: 0.4rem;
}

.switcher__select {
  flex: 1;
  min-width: 0;
  padding: 0.4rem 0.5rem;
  border-radius: 6px;
  border: 1px solid var(--uaw-border);
  background: var(--uaw-bg);
  color: var(--uaw-text);
}

.switcher__new {
  width: 2rem;
  border-radius: 6px;
  border: 1px solid var(--uaw-border);
  background: var(--uaw-bg);
  color: var(--uaw-text);
  font-size: 1rem;
  line-height: 1;
  cursor: pointer;
}

.switcher__new:hover {
  background: var(--uaw-surface-hover);
}
</style>
