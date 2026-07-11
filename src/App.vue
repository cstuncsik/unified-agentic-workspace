<script setup lang="ts">
import { onMounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useWorkspacesStore } from "./stores/workspaces";
import { useProjectsStore } from "./stores/projects";
import { useSessionsStore } from "./stores/sessions";
import { useRepositoriesStore } from "./stores/repositories";
import { useCodingWorkspacesStore } from "./stores/codingWorkspaces";
import { useReviewsStore } from "./stores/reviews";
import { useArtifactsStore } from "./stores/artifacts";
import { useProviderAccountsStore } from "./stores/providerAccounts";
import { STATUS_GROUPS } from "./types/session";
import WorkspaceSwitcher from "./components/WorkspaceSwitcher.vue";
import SessionsView from "./components/SessionsView.vue";
import ProjectsView from "./components/ProjectsView.vue";
import SourcesView from "./components/SourcesView.vue";
import CodingView from "./components/CodingView.vue";
import ReviewsView from "./components/ReviewsView.vue";
import AgentsView from "./components/AgentsView.vue";
import BoardView from "./components/BoardView.vue";
import ProvidersView from "./components/ProvidersView.vue";
import ArtifactsView from "./components/ArtifactsView.vue";
import ThemeToggle from "./components/ThemeToggle.vue";
import ConfirmDialog from "./components/ConfirmDialog.vue";
import UpdateBanner from "./components/UpdateBanner.vue";
import { useUpdater } from "./composables/useUpdater";

const workspaces = useWorkspacesStore();
const projects = useProjectsStore();
const sessions = useSessionsStore();
const repositories = useRepositoriesStore();
const coding = useCodingWorkspacesStore();
const reviews = useReviewsStore();
const artifacts = useArtifactsStore();
const providerAccounts = useProviderAccountsStore();
const updater = useUpdater();

type ActiveView =
  | "inbox"
  | "projects"
  | "artifacts"
  | "sources"
  | "coding"
  | "reviews"
  | "board"
  | "agents"
  | "providers";
const activeView = ref<ActiveView>("inbox");

// Placeholders for later milestones; kept visible so navigation stays product-shaped.
const plannedSections = ["Skills", "Automations", "Settings"];

function openInbox(filterKey: string | null) {
  activeView.value = "inbox";
  sessions.setFilter(filterKey);
}

onMounted(async () => {
  workspaces.load();
  if (await invoke<boolean>("updater_enabled")) {
    void updater.checkForUpdate({ silent: true });
  }
});

watch(
  () => workspaces.currentId,
  (workspaceId) => {
    if (workspaceId) {
      projects.load(workspaceId);
      artifacts.load(workspaceId);
      sessions.load(workspaceId);
      repositories.load(workspaceId);
      coding.load(workspaceId);
      reviews.load(workspaceId);
      providerAccounts.load(workspaceId);
      sessions.setFilter(null);
    }
  },
);
</script>

<template>
  <div class="app-root">
    <UpdateBanner />
    <div class="app">
      <aside class="sidebar">
        <div class="brand">UAW</div>
        <WorkspaceSwitcher />
        <nav class="nav">
          <button class="re-button" data-variant="brand" type="button" @click="openInbox(null)">
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
            :aria-current="
              activeView === 'inbox' && sessions.filterGroup === group.key ? 'page' : undefined
            "
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
            class="re-button"
            data-variant="ghost"
            :aria-current="activeView === 'artifacts' ? 'page' : undefined"
            type="button"
            @click="activeView = 'artifacts'"
          >
            Artifacts
          </button>

          <button
            class="re-button"
            data-variant="ghost"
            :aria-current="activeView === 'sources' ? 'page' : undefined"
            type="button"
            @click="activeView = 'sources'"
          >
            Sources
          </button>

          <button
            class="re-button"
            data-variant="ghost"
            :aria-current="activeView === 'coding' ? 'page' : undefined"
            type="button"
            @click="activeView = 'coding'"
          >
            Coding
          </button>

          <button
            class="re-button"
            data-variant="ghost"
            :aria-current="activeView === 'reviews' ? 'page' : undefined"
            type="button"
            @click="activeView = 'reviews'"
          >
            Reviews
          </button>

          <button
            class="re-button"
            data-variant="ghost"
            :aria-current="activeView === 'board' ? 'page' : undefined"
            type="button"
            @click="activeView = 'board'"
          >
            Board
          </button>

          <button
            class="re-button"
            data-variant="ghost"
            :aria-current="activeView === 'agents' ? 'page' : undefined"
            type="button"
            @click="activeView = 'agents'"
          >
            Agents
          </button>

          <button
            class="re-button"
            data-variant="ghost"
            :aria-current="activeView === 'providers' ? 'page' : undefined"
            type="button"
            @click="activeView = 'providers'"
          >
            Providers
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
          <button
            class="re-button"
            data-variant="ghost"
            @click="updater.checkForUpdate({ silent: false })"
          >
            Check for updates
          </button>
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
          <ArtifactsView v-else-if="activeView === 'artifacts'" />
          <SourcesView v-else-if="activeView === 'sources'" />
          <CodingView v-else-if="activeView === 'coding'" />
          <ReviewsView v-else-if="activeView === 'reviews'" />
          <BoardView v-else-if="activeView === 'board'" />
          <AgentsView v-else-if="activeView === 'agents'" />
          <ProvidersView v-else-if="activeView === 'providers'" />
        </template>
        <p v-else class="muted">No workspace selected.</p>
      </main>
      <ConfirmDialog />
    </div>
  </div>
</template>

<style>
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
.app-root {
  display: flex;
  flex-direction: column;
  height: 100vh;
}

.app {
  display: grid;
  grid-template-columns: 240px 1fr;
  flex: 1;
  min-height: 0;
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

.nav .re-button[data-variant="ghost"]:not([aria-current="page"]):not(:disabled):hover {
  background: color-mix(in srgb, var(--re-color-accent-600) 12%, transparent);
  color: var(--re-color-text);
}

/* Pressed (mousedown). Declared after :hover so it wins while pressing — the
   custom hover above otherwise suppresses the design system's :active state. */
.nav .re-button[data-variant="ghost"]:not([aria-current="page"]):not(:disabled):active {
  background: color-mix(in srgb, var(--re-color-accent-600) 20%, transparent);
  color: var(--re-color-text);
}

.nav .re-button[data-variant="ghost"][aria-current="page"] {
  background: color-mix(in srgb, var(--re-color-accent-600) 24%, transparent);
  color: var(--re-color-text);
  /* Full accent outline follows the button's rounded corners cleanly,
     avoiding the clipped/odd left-edge a left-only inset bar produced. */
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--re-color-accent-600) 45%, transparent);
}

/* The selected item also needs hover/press feedback (re-clicking the active
   filter) — the custom hover/active rules above exclude aria-current. */
.nav .re-button[data-variant="ghost"][aria-current="page"]:hover {
  background: color-mix(in srgb, var(--re-color-accent-600) 32%, transparent);
}

.nav .re-button[data-variant="ghost"][aria-current="page"]:active {
  background: color-mix(in srgb, var(--re-color-accent-600) 40%, transparent);
}

/* Restore the DS keyboard focus ring on the selected item; its inset outline
   would otherwise replace the focus box-shadow. */
.nav .re-button[data-variant="ghost"][aria-current="page"]:focus-visible {
  box-shadow:
    inset 0 0 0 1px color-mix(in srgb, var(--re-color-accent-600) 45%, transparent),
    var(--re-shadow-focus);
  outline: none;
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
  /* Flex column so a full-height view (e.g. the agent terminal) can fill the
     space left under the header instead of overflowing the scroll area. */
  display: flex;
  flex-direction: column;
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
  color: var(--re-color-danger-text);
}
</style>
