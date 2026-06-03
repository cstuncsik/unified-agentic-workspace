<script setup lang="ts">
import { onMounted } from "vue";
import { useWorkspacesStore } from "./stores/workspaces";
import WorkspaceSwitcher from "./components/WorkspaceSwitcher.vue";

const store = useWorkspacesStore();

// Product-shaped navigation placeholders. Wired up in later milestones.
const navSections = [
  "New Session",
  "Inbox",
  "Projects",
  "Sources",
  "Skills",
  "Automations",
  "Reviews",
  "Settings",
];

onMounted(() => {
  store.load();
});
</script>

<template>
  <div class="app">
    <aside class="sidebar">
      <div class="brand">UAW</div>
      <WorkspaceSwitcher />
      <nav class="nav">
        <button
          v-for="section in navSections"
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
      <p v-if="store.loading" class="muted">Loading workspace…</p>
      <p v-else-if="store.error" class="error">{{ store.error }}</p>
      <template v-else-if="store.current">
        <header class="main__header">
          <h1>{{ store.current.name }}</h1>
          <span class="badge">{{ store.current.kind }}</span>
        </header>
        <div class="empty">
          <p>This workspace is empty.</p>
          <p class="muted">
            Projects, sessions, sources, skills, automations, and reviews will appear here.
          </p>
        </div>
      </template>
      <p v-else class="muted">No workspace selected.</p>
    </main>
  </div>
</template>

<style>
:root {
  --uaw-bg: #0f1115;
  --uaw-surface: #171a21;
  --uaw-surface-hover: #1f232c;
  --uaw-border: #2a2f3a;
  --uaw-text: #e6e8ec;
  --uaw-muted: #8a92a3;
  color-scheme: dark;
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
  background: var(--uaw-bg);
  color: var(--uaw-text);
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
  cursor: not-allowed;
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

.empty {
  margin-top: 2rem;
}

.muted {
  color: var(--uaw-muted);
}

.error {
  color: #ff6b6b;
}
</style>
