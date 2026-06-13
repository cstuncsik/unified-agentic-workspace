<script setup lang="ts">
import { onMounted, ref, watch } from "vue";
import { useWorkspacesStore } from "./stores/workspaces";
import { useProjectsStore } from "./stores/projects";
import { useSessionsStore } from "./stores/sessions";
import { STATUS_GROUPS } from "./types/session";
import WorkspaceSwitcher from "./components/WorkspaceSwitcher.vue";
import SessionsView from "./components/SessionsView.vue";
import ProjectsView from "./components/ProjectsView.vue";
import ThemeToggle from "./components/ThemeToggle.vue";
import ConfirmDialog from "./components/ConfirmDialog.vue";

const workspaces = useWorkspacesStore();
const projects = useProjectsStore();
const sessions = useSessionsStore();

type ActiveView = "inbox" | "projects";
const activeView = ref<ActiveView>("inbox");

// Placeholders for later milestones; kept visible so navigation stays product-shaped.
const plannedSections = ["Sources", "Skills", "Automations", "Reviews", "Settings"];

function openInbox(filterKey: string | null) {
  activeView.value = "inbox";
  sessions.setFilter(filterKey);
}

onMounted(() => {
  workspaces.load();
});

watch(
  () => workspaces.currentId,
  (workspaceId) => {
    if (workspaceId) {
      projects.load(workspaceId);
      sessions.load(workspaceId);
      sessions.setFilter(null);
    }
  },
);
</script>

<template>
  <div class="app">
    <aside class="sidebar">
      <div class="brand">UAW</div>
      <WorkspaceSwitcher />
      <nav class="nav">
        <button class="re-button" data-variant="primary" type="button" @click="openInbox(null)">
          New Session
        </button>

        <button
          class="re-button"
          data-variant="ghost"
          :aria-current="activeView === 'inbox' && !sessions.filterGroup ? 'page' : undefined"
          type="button"
          @click="openInbox(null)"
        >
          Inbox
        </button>
        <button
          v-for="group in STATUS_GROUPS"
          :key="group.key"
          class="re-button nav__sub"
          data-variant="ghost"
          :aria-current="activeView === 'inbox' && sessions.filterGroup === group.key ? 'page' : undefined"
          type="button"
          @click="openInbox(group.key)"
        >
          {{ group.label }}
        </button>

        <button
          class="re-button"
          data-variant="ghost"
          :aria-current="activeView === 'projects' ? 'page' : undefined"
          type="button"
          @click="activeView = 'projects'"
        >
          Projects
        </button>

        <button
          v-for="section in plannedSections"
          :key="section"
          class="re-button"
          data-variant="ghost"
          type="button"
          disabled
        >
          {{ section }}
        </button>
      </nav>
      <div class="sidebar__footer">
        <ThemeToggle />
        <span class="sidebar__footer-label">Unified Agentic Workspace</span>
      </div>
    </aside>

    <main class="main">
      <p v-if="workspaces.loading" class="muted">Loading workspace…</p>
      <p v-else-if="workspaces.error" class="error">{{ workspaces.error }}</p>
      <template v-else-if="workspaces.current">
        <header class="main__header">
          <h1>{{ workspaces.current.name }}</h1>
          <span class="badge">{{ workspaces.current.kind }}</span>
        </header>
        <SessionsView v-if="activeView === 'inbox'" />
        <ProjectsView v-else-if="activeView === 'projects'" />
      </template>
      <p v-else class="muted">No workspace selected.</p>
    </main>
    <ConfirmDialog />
  </div>
</template>

<style>
:root {
  --uaw-bg: var(--re-color-bg);
  --uaw-surface: var(--re-color-surface);
  --uaw-surface-hover: var(--re-color-bg-muted);
  --uaw-border: var(--re-color-border);
  --uaw-text: var(--re-color-text);
  --uaw-muted: var(--re-color-text-muted);
}

* {
  box-sizing: border-box;
}

html,
body,
#app {
  margin: 0;
  height: 100%;
}

body {
  font-family:
    system-ui,
    -apple-system,
    "Segoe UI",
    sans-serif;
}
</style>

<style scoped>
.app {
  display: grid;
  grid-template-columns: 240px 1fr;
  height: 100vh;
}

.sidebar {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  padding: 1rem;
  border-right: 1px solid var(--re-color-border);
  background: var(--re-color-surface);
  overflow-y: auto;
}

.brand {
  font-weight: 700;
  font-size: 1.1rem;
  letter-spacing: 0.06em;
}

.nav {
  display: flex;
  flex-direction: column;
  gap: 0.15rem;
}

.nav .re-button {
  justify-content: flex-start;
  width: 100%;
}

.nav .re-button[aria-current="page"] {
  background: var(--re-color-bg-muted);
}

.nav__sub {
  padding-left: 1.4rem;
}

.sidebar__footer {
  margin-top: auto;
  display: flex;
  flex-direction: column;
  gap: 0.6rem;
}

.sidebar__footer-label {
  font-size: 0.7rem;
  color: var(--re-color-text-muted);
}

.main {
  padding: 2rem;
  overflow: auto;
}

.main__header {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  margin-bottom: 1.5rem;
}

.main__header h1 {
  margin: 0;
  font-size: 1.5rem;
}

.badge {
  font-size: 0.7rem;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  padding: 0.15rem 0.5rem;
  border: 1px solid var(--re-color-border);
  border-radius: 999px;
  color: var(--re-color-text-muted);
}

.muted {
  color: var(--re-color-text-muted);
}

.error {
  color: var(--re-color-text-danger);
}
</style>
