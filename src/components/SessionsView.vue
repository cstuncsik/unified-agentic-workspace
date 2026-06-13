<script setup lang="ts">
import { computed, ref, watch } from "vue";
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
import { useToast } from "../composables/useToast";
import { useConfirm } from "../composables/useConfirm";

const workspaces = useWorkspacesStore();
const projects = useProjectsStore();
const sessions = useSessionsStore();
const toast = useToast();
const { confirm } = useConfirm();

const newTitle = ref("");
const newMode = ref<SessionMode>("research");
const newProjectId = ref<string>("");
const submitting = ref(false);

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
  try {
    await sessions.create({
      workspaceId: workspaces.currentId,
      title,
      mode: newMode.value,
      projectId: newProjectId.value || undefined,
    });
    newTitle.value = "";
    toast.success("Session created");
  } catch (e) {
    toast.error(String(e));
  } finally {
    submitting.value = false;
  }
}

async function moveSession(id: string, previous: SessionStatus, event: Event) {
  const select = event.target as HTMLSelectElement;
  const status = select.value as SessionStatus;
  try {
    await sessions.setStatus(id, status);
  } catch (e) {
    toast.error(String(e));
    select.value = previous;
  }
}

async function removeSession(id: string, title: string) {
  const confirmed = await confirm(`Delete session "${title}"?`, "Delete session");
  if (!confirmed) return;
  try {
    await sessions.remove(id);
    toast.success("Session deleted");
  } catch (e) {
    toast.error(String(e));
  }
}
</script>

<template>
  <section>
    <h2 class="view-title">{{ heading }}</h2>

    <form class="create" @submit.prevent="createSession">
      <input
        v-model="newTitle"
        class="re-input"
        type="text"
        placeholder="New session title"
        aria-label="New session title"
      />
      <select v-model="newMode" class="re-select" aria-label="Session mode">
        <option v-for="mode in SESSION_MODES" :key="mode" :value="mode">{{ mode }}</option>
      </select>
      <select v-model="newProjectId" class="re-select" aria-label="Session project">
        <option value="">No project</option>
        <option v-for="project in projects.list" :key="project.id" :value="project.id">
          {{ project.name }}
        </option>
      </select>
      <button
        class="re-button"
        data-variant="brand"
        type="submit"
        :disabled="submitting || !newTitle.trim()"
      >
        Create
      </button>
    </form>

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
          <li
            v-for="session in group.sessions"
            :key="session.id"
            class="re-card"
            data-testid="session-row"
          >
            <span class="row__main">
              <span class="row__title">{{ session.title }}</span>
              <span class="row__meta">
                <span class="re-badge">{{ session.mode }}</span>
                <span v-if="session.project_id" class="row__project">
                  {{ projectNames.get(session.project_id) ?? "Unknown project" }}
                </span>
              </span>
            </span>
            <select
              class="re-select"
              data-size="sm"
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
              class="re-button"
              data-variant="danger"
              data-size="sm"
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
  color: var(--re-color-text-muted);
}

.group__count {
  padding: 0 0.45rem;
  border: 1px solid var(--re-color-border);
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

.rows .re-card {
  display: flex;
  align-items: center;
  gap: 0.6rem;
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
  color: var(--re-color-text-muted);
}

.muted {
  color: var(--re-color-text-muted);
}

.error {
  color: var(--re-color-text-danger);
}
</style>
