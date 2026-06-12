<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import { useWorkspacesStore } from "../stores/workspaces";
import { useProjectsStore } from "../stores/projects";
import { useSessionsStore } from "../stores/sessions";
import {
  SESSION_MODES,
  SESSION_STATUSES,
  STATUS_GROUPS,
  type SessionMode,
  type SessionStatus,
} from "../types/session";

const workspaces = useWorkspacesStore();
const projects = useProjectsStore();
const sessions = useSessionsStore();

const newTitle = ref("");
const newMode = ref<SessionMode>("research");
const newProjectId = ref<string>("");
const submitting = ref(false);
const formError = ref<string | null>(null);

// A project picked in one workspace must never carry over to another.
watch(
  () => workspaces.currentId,
  () => {
    newProjectId.value = "";
  },
);

const heading = computed(() => {
  const group = STATUS_GROUPS.find((g) => g.key === sessions.filterGroup);
  return group ? group.label : "Inbox";
});

const projectNames = computed(() => new Map(projects.list.map((p) => [p.id, p.name])));

async function createSession() {
  const title = newTitle.value.trim();
  if (!title || !workspaces.currentId) return;
  submitting.value = true;
  formError.value = null;
  try {
    await sessions.create({
      workspaceId: workspaces.currentId,
      title,
      mode: newMode.value,
      projectId: newProjectId.value || undefined,
    });
    newTitle.value = "";
  } catch (e) {
    formError.value = String(e);
  } finally {
    submitting.value = false;
  }
}

async function moveSession(id: string, previous: SessionStatus, event: Event) {
  const select = event.target as HTMLSelectElement;
  const status = select.value as SessionStatus;
  formError.value = null;
  try {
    await sessions.setStatus(id, status);
  } catch (e) {
    formError.value = String(e);
    select.value = previous;
  }
}

async function removeSession(id: string, title: string) {
  const confirmed = await ask(`Delete session "${title}"?`, {
    title: "Delete session",
    kind: "warning",
  });
  if (!confirmed) return;
  formError.value = null;
  try {
    await sessions.remove(id);
  } catch (e) {
    formError.value = String(e);
  }
}
</script>

<template>
  <section>
    <h2 class="view-title">{{ heading }}</h2>

    <form class="create" @submit.prevent="createSession">
      <input
        v-model="newTitle"
        class="create__input"
        type="text"
        placeholder="New session title"
        aria-label="New session title"
      />
      <select v-model="newMode" class="create__select" aria-label="Session mode">
        <option v-for="mode in SESSION_MODES" :key="mode" :value="mode">{{ mode }}</option>
      </select>
      <select v-model="newProjectId" class="create__select" aria-label="Session project">
        <option value="">No project</option>
        <option v-for="project in projects.list" :key="project.id" :value="project.id">
          {{ project.name }}
        </option>
      </select>
      <button class="create__submit" type="submit" :disabled="submitting || !newTitle.trim()">
        Create
      </button>
    </form>
    <p v-if="formError" class="error">{{ formError }}</p>

    <p v-if="sessions.loading" class="muted">Loading sessions…</p>
    <p v-else-if="sessions.error" class="error">{{ sessions.error }}</p>
    <p v-else-if="sessions.visibleGroups.every((g) => g.sessions.length === 0)" class="muted">
      No sessions here yet.
    </p>
    <template v-else>
      <div v-for="group in sessions.visibleGroups" :key="group.key" class="group">
        <h3 v-if="group.sessions.length > 0" class="group__title">
          {{ group.label }}
          <span class="group__count">{{ group.sessions.length }}</span>
        </h3>
        <ul class="rows">
          <li v-for="session in group.sessions" :key="session.id" class="row">
            <span class="row__main">
              <span class="row__title">{{ session.title }}</span>
              <span class="row__meta">
                <span class="badge">{{ session.mode }}</span>
                <span v-if="session.project_id" class="row__project">
                  {{ projectNames.get(session.project_id) ?? "Unknown project" }}
                </span>
              </span>
            </span>
            <select
              class="row__status"
              :value="session.status"
              aria-label="Session status"
              @change="moveSession(session.id, session.status, $event)"
            >
              <option v-for="status in SESSION_STATUSES" :key="status" :value="status">
                {{ status }}
              </option>
            </select>
            <button
              type="button"
              class="row__action row__action--danger"
              @click="removeSession(session.id, session.title)"
            >
              Delete
            </button>
          </li>
        </ul>
      </div>
    </template>
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
.row__status {
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

.group {
  margin-bottom: 1.25rem;
}

.group__title {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  margin: 0 0 0.5rem;
  font-size: 0.8rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--uaw-muted);
}

.group__count {
  padding: 0 0.45rem;
  border: 1px solid var(--uaw-border);
  border-radius: 999px;
  font-size: 0.7rem;
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

.row__main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 0.2rem;
}

.row__title {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.row__meta {
  display: flex;
  align-items: center;
  gap: 0.45rem;
}

.row__project {
  font-size: 0.75rem;
  color: var(--uaw-muted);
}

.row__status {
  font-size: 0.8rem;
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

.row__action:hover {
  background: var(--uaw-surface-hover);
}

.row__action--danger:hover {
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
