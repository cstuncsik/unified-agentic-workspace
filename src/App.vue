<script setup lang="ts">
import { onMounted, ref, watch } from "vue";
import { useWorkspacesStore } from "./stores/workspaces";
import { useProjectsStore } from "./stores/projects";
import { useSessionsStore } from "./stores/sessions";
import { STATUS_GROUPS } from "./types/session";
import WorkspaceSwitcher from "./components/WorkspaceSwitcher.vue";
import SessionsView from "./components/SessionsView.vue";
import ProjectsView from "./components/ProjectsView.vue";

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
        <button class="nav__item nav__item--primary" type="button" @click="openInbox(null)">
          New Session
        </button>

        <button
          class="nav__item"
          :class="{ 'nav__item--active': activeView === 'inbox' && !sessions.filterGroup }"
          type="button"
          @click="openInbox(null)"
        >
          Inbox
        </button>
        <button
          v-for="group in STATUS_GROUPS"
          :key="group.key"
          class="nav__item nav__item--sub"
          :class="{
            'nav__item--active': activeView === 'inbox' && sessions.filterGroup === group.key,
          }"
          type="button"
          @click="openInbox(group.key)"
        >
          {{ group.label }}
        </button>

        <button
          class="nav__item"
          :class="{ 'nav__item--active': activeView === 'projects' }"
          type="button"
          @click="activeView = 'projects'"
        >
          Projects
        </button>

        <button
          v-for="section in plannedSections"
          :key="section"
          class="nav__item"
          type="button"
          disabled
        >
          {{ section }}
        </button>
      </nav>
      <div class="sidebar__footer">Unified Agentic Workspace</div>
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
  border-right: 1px solid var(--uaw-border);
  background: var(--uaw-surface);
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

.nav__item {
  text-align: left;
  padding: 0.45rem 0.55rem;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--uaw-muted);
  font-size: 0.9rem;
  cursor: pointer;
}

.nav__item:disabled {
  cursor: not-allowed;
}

.nav__item:not(:disabled):hover {
  background: var(--uaw-surface-hover);
  color: var(--uaw-text);
}

.nav__item--active {
  background: var(--uaw-surface-hover);
  color: var(--uaw-text);
}

.nav__item--primary {
  color: var(--uaw-text);
  border: 1px solid var(--uaw-border);
  margin-bottom: 0.5rem;
}

.nav__item--sub {
  padding-left: 1.4rem;
  font-size: 0.82rem;
}

.sidebar__footer {
  margin-top: auto;
  font-size: 0.7rem;
  color: var(--uaw-muted);
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
