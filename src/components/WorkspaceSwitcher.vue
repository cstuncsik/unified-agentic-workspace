<script setup lang="ts">
import { computed, nextTick, ref } from "vue";
import { useWorkspacesStore } from "../stores/workspaces";

const store = useWorkspacesStore();

const selectedId = computed({
  get: () => store.currentId ?? "",
  set: (id: string) => store.select(id),
});

// window.prompt is a no-op in Tauri's webview on macOS, so creation uses an inline input.
const creating = ref(false);
const newName = ref("");
const nameInput = ref<HTMLInputElement | null>(null);

async function startCreate() {
  creating.value = true;
  newName.value = "";
  await nextTick();
  nameInput.value?.focus();
}

function cancelCreate() {
  creating.value = false;
  newName.value = "";
}

async function submitCreate() {
  const name = newName.value.trim();
  if (!name) return;
  await store.create(name);
  cancelCreate();
}
</script>

<template>
  <div class="switcher">
    <span class="switcher__label">Workspace</span>
    <div v-if="!creating" class="switcher__row">
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
        @click="startCreate"
      >
        +
      </button>
    </div>
    <form v-else class="switcher__row" @submit.prevent="submitCreate">
      <input
        ref="nameInput"
        v-model="newName"
        class="switcher__input"
        type="text"
        placeholder="Workspace name"
        aria-label="New workspace name"
        @keyup.esc="cancelCreate"
      />
      <button class="switcher__new" type="submit" :disabled="!newName.trim()" title="Create">
        ✓
      </button>
      <button class="switcher__new" type="button" title="Cancel" @click="cancelCreate">×</button>
    </form>
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

.switcher__select,
.switcher__input {
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

.switcher__new:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.switcher__new:not(:disabled):hover {
  background: var(--uaw-surface-hover);
}
</style>
