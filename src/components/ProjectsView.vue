<script setup lang="ts">
import { ref } from "vue";
import { useWorkspacesStore } from "../stores/workspaces";
import { useProjectsStore } from "../stores/projects";
import { useSessionsStore } from "../stores/sessions";
import { PROJECT_MODES, type ProjectMode } from "../types/project";
import { useToast } from "../composables/useToast";
import { useConfirm } from "../composables/useConfirm";

const workspaces = useWorkspacesStore();
const projects = useProjectsStore();
const sessions = useSessionsStore();
const toast = useToast();
const { confirm } = useConfirm();

const newName = ref("");
const newMode = ref<ProjectMode>("research");
const submitting = ref(false);

const editingId = ref<string | null>(null);
const editName = ref("");

async function createProject() {
  const name = newName.value.trim();
  if (!name || !workspaces.currentId) return;
  submitting.value = true;
  try {
    await projects.create(workspaces.currentId, name, newMode.value);
    newName.value = "";
    toast.success("Project created");
  } catch (e) {
    toast.error(String(e));
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
  try {
    await projects.rename(id, name);
    cancelRename();
    toast.success("Project renamed");
  } catch (e) {
    toast.error(String(e));
  }
}

async function removeProject(id: string, name: string) {
  const confirmed = await confirm(
    `Delete project "${name}"? Its sessions are kept and detached.`,
    "Delete project",
  );
  if (!confirmed) return;
  try {
    await projects.remove(id);
    sessions.detachProject(id);
    if (editingId.value === id) cancelRename();
    toast.success("Project deleted");
  } catch (e) {
    toast.error(String(e));
  }
}
</script>

<template>
  <section>
    <h2 class="view-title">Projects</h2>

    <form class="create" @submit.prevent="createProject">
      <input
        v-model="newName"
        class="re-input"
        type="text"
        placeholder="New project name"
        aria-label="New project name"
      />
      <select v-model="newMode" class="re-select" aria-label="Project mode">
        <option v-for="mode in PROJECT_MODES" :key="mode" :value="mode">{{ mode }}</option>
      </select>
      <button
        class="re-button"
        data-variant="primary"
        type="submit"
        :disabled="submitting || !newName.trim()"
      >
        Create
      </button>
    </form>

    <p v-if="projects.loading" class="muted">Loading projects…</p>
    <p v-else-if="projects.error" class="error">{{ projects.error }}</p>
    <p v-else-if="projects.list.length === 0" class="muted">
      No projects yet. Create a research, code, or mixed project to get started.
    </p>
    <ul v-else class="rows">
      <li
        v-for="project in projects.list"
        :key="project.id"
        class="re-card"
        data-testid="project-row"
      >
        <template v-if="editingId === project.id">
          <input
            v-model="editName"
            class="re-input"
            data-size="sm"
            type="text"
            aria-label="Project name"
            @keyup.enter="saveRename"
            @keyup.esc="cancelRename"
          />
          <span class="row__actions">
            <button
              type="button"
              class="re-button"
              data-variant="ghost"
              data-size="sm"
              :disabled="!editName.trim()"
              @click="saveRename"
            >
              Save
            </button>
            <button
              type="button"
              class="re-button"
              data-variant="ghost"
              data-size="sm"
              @click="cancelRename"
            >
              Cancel
            </button>
          </span>
        </template>
        <template v-else>
          <span class="row__title">{{ project.name }}</span>
          <span class="re-badge" data-variant="neutral">{{ project.mode }}</span>
          <span class="row__actions">
            <button
              type="button"
              class="re-button"
              data-variant="ghost"
              data-size="sm"
              @click="startRename(project.id, project.name)"
            >
              Rename
            </button>
            <button
              type="button"
              class="re-button"
              data-variant="danger"
              data-size="sm"
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

.rows {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}

.rows .re-card {
  display: flex;
  align-items: center;
  gap: 0.6rem;
}

.row__title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.rows .re-card .re-input {
  flex: 1;
  min-width: 0;
}

.row__actions {
  display: flex;
  gap: 0.35rem;
}

.muted {
  color: var(--re-color-text-muted);
}

.error {
  color: var(--re-color-text-danger);
}
</style>
