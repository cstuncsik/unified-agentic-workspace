<script setup lang="ts">
import { computed, nextTick, ref } from "vue";
import { useWorkspacesStore } from "../stores/workspaces";
import { useToast } from "../composables/useToast";

const store = useWorkspacesStore();
const toast = useToast();

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
  try {
    await store.create(name);
    toast.success("Workspace created");
    cancelCreate();
  } catch (e) {
    toast.error(String(e));
  }
}
</script>

<template>
  <div class="switcher">
    <span class="switcher__label">Workspace</span>
    <div v-if="!creating" class="switcher__row">
      <select
        v-model="selectedId"
        class="re-select"
        data-size="sm"
        :disabled="store.list.length === 0"
        aria-label="Select workspace"
      >
        <option v-for="ws in store.list" :key="ws.id" :value="ws.id">
          {{ ws.name }}
        </option>
      </select>
      <button
        class="re-button"
        data-variant="ghost"
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
        class="re-input"
        data-size="sm"
        type="text"
        placeholder="Workspace name"
        aria-label="New workspace name"
        @keyup.esc="cancelCreate"
      />
      <button
        class="re-button"
        data-variant="ghost"
        type="submit"
        :disabled="!newName.trim()"
        title="Create"
      >
        ✓
      </button>
      <button
        class="re-button"
        data-variant="ghost"
        type="button"
        title="Cancel"
        @click="cancelCreate"
      >
        ×
      </button>
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
  color: var(--re-color-text-muted);
}

.switcher__row {
  display: flex;
  gap: 0.4rem;
}

.switcher__row .re-select,
.switcher__row .re-input {
  flex: 1;
  min-width: 0;
}

.switcher__row .re-button {
  flex: 0 0 auto;
}
</style>
