<script setup lang="ts">
import { ref } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import { useWorkspacesStore } from "../stores/workspaces";
import { useProjectsStore } from "../stores/projects";
import { useSessionsStore } from "../stores/sessions";
import { PROJECT_MODES, type ProjectMode } from "../types/project";

const workspaces = useWorkspacesStore();
const projects = useProjectsStore();
const sessions = useSessionsStore();

const newName = ref("");
const newMode = ref<ProjectMode>("research");
const submitting = ref(false);
const formError = ref<string | null>(null);

const editingId = ref<string | null>(null);
const editName = ref("");

async function createProject() {
  const name = newName.value.trim();
  if (!name || !workspaces.currentId) return;
  submitting.value = true;
  formError.value = null;
  try {
    await projects.create(workspaces.currentId, name, newMode.value);
    newName.value = "";
  } catch (e) {
    formError.value = String(e);
  } finally {
    submitting.value = false;
  }
}

function startRename(id: string, currentName: string) {
  editingId.value = id;
  editName.value = currentName;
}

function cancelRename() {
  editingId.value = null;
  editName.value = "";
}

async function saveRename() {
  const id = editingId.value;
  const name = editName.value.trim();
  if (!id || !name) return;
  formError.value = null;
  try {
    await projects.rename(id, name);
    cancelRename();
  } catch (e) {
    formError.value = String(e);
  }
}

async function removeProject(id: string, name: string) {
  const confirmed = await ask(`Delete project "${name}"? Its sessions are kept and detached.`, {
    title: "Delete project",
    kind: "warning",
  });
  if (!confirmed) return;
  formError.value = null;
  try {
    await projects.remove(id);
    sessions.detachProject(id);
    if (editingId.value === id) cancelRename();
  } catch (e) {
    formError.value = String(e);
  }
}
</script>

<template>
  <section>
    <h2 class="view-title">Projects</h2>

    <form class="create" @submit.prevent="createProject">
      <input
        v-model="newName"
        class="create__input"
        type="text"
        placeholder="New project name"
        aria-label="New project name"
      />
      <select v-model="newMode" class="create__select" aria-label="Project mode">
        <option v-for="mode in PROJECT_MODES" :key="mode" :value="mode">{{ mode }}</option>
      </select>
      <button class="create__submit" type="submit" :disabled="submitting || !newName.trim()">
        Create
      </button>
    </form>
    <p v-if="formError" class="error">{{ formError }}</p>

    <p v-if="projects.loading" class="muted">Loading projects…</p>
    <p v-else-if="projects.error" class="error">{{ projects.error }}</p>
    <p v-else-if="projects.list.length === 0" class="muted">
      No projects yet. Create a research, code, or mixed project to get started.
    </p>
    <ul v-else class="rows">
      <li v-for="project in projects.list" :key="project.id" class="row" data-testid="project-row">
        <template v-if="editingId === project.id">
          <input
            v-model="editName"
            class="row__edit"
            type="text"
            aria-label="Project name"
            @keyup.enter="saveRename"
            @keyup.esc="cancelRename"
          />
          <span class="row__actions">
            <button
              type="button"
              class="row__action"
              :disabled="!editName.trim()"
              @click="saveRename"
            >
              Save
            </button>
            <button type="button" class="row__action" @click="cancelRename">Cancel</button>
          </span>
        </template>
        <template v-else>
          <span class="row__title">{{ project.name }}</span>
          <span class="badge">{{ project.mode }}</span>
          <span class="row__actions">
            <button
              type="button"
              class="row__action"
              @click="startRename(project.id, project.name)"
            >
              Rename
            </button>
            <button
              type="button"
              class="row__action row__action--danger"
              @click="removeProject(project.id, project.name)"
            >
              Delete
            </button>
          </span>
        </template>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.view-title {
  margin: 0 0 1rem;
  font-size: 1.2rem;
}

.create {
  display: flex;
  gap: 0.5rem;
  margin-bottom: 1rem;
}

.create__input {
  flex: 1;
  min-width: 0;
}

.create__input,
.create__select,
.row__edit {
  padding: 0.45rem 0.55rem;
  border-radius: 6px;
  border: 1px solid var(--uaw-border);
  background: var(--uaw-bg);
  color: var(--uaw-text);
}

.create__submit {
  padding: 0.45rem 0.9rem;
  border-radius: 6px;
  border: 1px solid var(--uaw-border);
  background: var(--uaw-surface);
  color: var(--uaw-text);
  cursor: pointer;
}

.create__submit:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.create__submit:not(:disabled):hover {
  background: var(--uaw-surface-hover);
}

.rows {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}

.row {
  display: flex;
  align-items: center;
  gap: 0.6rem;
  padding: 0.55rem 0.7rem;
  border: 1px solid var(--uaw-border);
  border-radius: 8px;
  background: var(--uaw-surface);
}

.row__title,
.row__edit {
  flex: 1;
  min-width: 0;
}

.row__title {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.row__actions {
  display: flex;
  gap: 0.35rem;
}

.row__action {
  padding: 0.25rem 0.55rem;
  border-radius: 6px;
  border: 1px solid var(--uaw-border);
  background: transparent;
  color: var(--uaw-muted);
  font-size: 0.78rem;
  cursor: pointer;
}

.row__action:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.row__action:not(:disabled):hover {
  background: var(--uaw-surface-hover);
  color: var(--uaw-text);
}

.row__action--danger:not(:disabled):hover {
  color: #ff6b6b;
}

.badge {
  font-size: 0.7rem;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  padding: 0.15rem 0.5rem;
  border: 1px solid var(--uaw-border);
  border-radius: 999px;
  color: var(--uaw-muted);
}

.muted {
  color: var(--uaw-muted);
}

.error {
  color: #ff6b6b;
}
</style>
