<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useAgentSessionsStore } from "../stores/agentSessions";
import { useCodingWorkspacesStore } from "../stores/codingWorkspaces";
import { useWorkspacesStore } from "../stores/workspaces";
import { useToast } from "../composables/useToast";
import TerminalTab from "./TerminalTab.vue";

const store = useAgentSessionsStore();
const coding = useCodingWorkspacesStore();
const workspaces = useWorkspacesStore();
const toast = useToast();

const newWorktreeId = ref("");
const newAdapterId = ref("");
const starting = ref(false);

onMounted(async () => {
  await store.loadAdapters();
  if (store.adapters.length > 0) newAdapterId.value = store.adapters[0].id;
});

const canStart = computed(
  () => newWorktreeId.value !== "" && newAdapterId.value !== "" && !starting.value,
);

const worktreeLabel = (id: string) => {
  const cw = coding.list.find((c) => c.id === id);
  return cw ? cw.branch_name : id;
};
const adapterLabel = (id: string) => store.adapters.find((a) => a.id === id)?.name ?? id;

// Agent tabs persist in memory for the whole app session, but each belongs to one
// workspace. Only show the current workspace's terminals; the others stay mounted
// (hidden) and reappear when you switch back.
const visibleTabs = computed(() =>
  store.tabs.filter((t) => t.session.workspace_id === workspaces.currentId),
);

// On a workspace switch, clear the stale worktree selection and make sure the
// active tab belongs to the new workspace (otherwise another workspace's terminal
// would stay shown — and its label would resolve against the wrong coding list).
watch(
  () => workspaces.currentId,
  () => {
    newWorktreeId.value = "";
    if (!visibleTabs.value.some((t) => t.session.id === store.activeId)) {
      store.activeId = visibleTabs.value.length
        ? visibleTabs.value[visibleTabs.value.length - 1].session.id
        : null;
    }
  },
);

async function openTerminal() {
  if (!canStart.value) return;
  starting.value = true;
  try {
    // 80x24 is a safe initial size; the TerminalTab fits + resizes on mount.
    await store.start(newWorktreeId.value, newAdapterId.value, 80, 24);
  } catch (e) {
    toast.error(String(e));
  } finally {
    starting.value = false;
  }
}
</script>

<template>
  <section class="agents">
    <header class="agents__bar">
      <ul class="tabs">
        <li
          v-for="t in visibleTabs"
          :key="t.session.id"
          class="tab"
          :class="{ 'tab--active': t.session.id === store.activeId }"
          data-testid="agent-tab"
          @click="store.activeId = t.session.id"
        >
          <span class="tab__label">
            {{ adapterLabel(t.session.adapter_id) }} ·
            {{ worktreeLabel(t.session.coding_workspace_id) }}
          </span>
          <span class="re-badge" :data-tone="t.session.status === 'running' ? 'info' : undefined">
            {{ t.session.status }}
          </span>
          <button
            type="button"
            class="tab__close"
            aria-label="Close terminal tab"
            @click.stop="store.closeTab(t.session.id)"
          >
            ×
          </button>
        </li>
      </ul>

      <form class="new" @submit.prevent="openTerminal">
        <select
          v-model="newWorktreeId"
          class="re-select"
          data-size="sm"
          aria-label="Agent worktree"
        >
          <option value="" disabled>Worktree</option>
          <option v-for="cw in coding.list" :key="cw.id" :value="cw.id">
            {{ cw.branch_name }}
          </option>
        </select>
        <select v-model="newAdapterId" class="re-select" data-size="sm" aria-label="Agent CLI">
          <option v-for="a in store.adapters" :key="a.id" :value="a.id">{{ a.name }}</option>
        </select>
        <button
          class="re-button"
          data-variant="brand"
          data-size="sm"
          type="submit"
          :disabled="!canStart"
        >
          New terminal
        </button>
      </form>
    </header>

    <p v-if="coding.list.length === 0" class="muted hint">
      Create a worktree in Coding first, then open an agent terminal here.
    </p>

    <div v-if="store.activeId" class="agents__pane">
      <!-- Keep each terminal mounted so its xterm + stream persist across tab switches. -->
      <div
        v-for="t in store.tabs"
        v-show="t.session.id === store.activeId"
        :key="t.session.id"
        class="agents__term"
      >
        <div class="agents__termhead">
          <span class="muted">{{ t.session.command }} · {{ t.session.status }}</span>
          <button
            v-if="t.session.status === 'running'"
            type="button"
            class="re-button"
            data-variant="danger"
            data-size="sm"
            @click="store.stop(t.session.id)"
          >
            Stop
          </button>
        </div>
        <TerminalTab :session-id="t.session.id" />
      </div>
    </div>
    <p v-else class="muted">No terminals open. Pick a worktree and a CLI to start one.</p>
  </section>
</template>

<style scoped>
.agents {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  height: 100%;
}
.agents__bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 0.6rem;
}
.tabs {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-wrap: wrap;
  gap: 0.35rem;
}
.tab {
  display: flex;
  align-items: center;
  gap: 0.4rem;
  padding: 0.3rem 0.55rem;
  border: 1px solid var(--re-color-border);
  border-radius: var(--re-radius-md, 6px);
  cursor: pointer;
  font-size: 0.8rem;
}
.tab--active {
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--re-color-accent-600) 45%, transparent);
}
.tab__close {
  border: none;
  background: none;
  cursor: pointer;
  color: var(--re-color-text-muted);
  font-size: 1rem;
  line-height: 1;
}
.new {
  display: flex;
  gap: 0.35rem;
}
.agents__pane {
  flex: 1;
  min-height: 0;
}
.agents__term {
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
  height: 100%;
}
.agents__termhead {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 0.8rem;
}
.hint {
  font-size: 0.85rem;
}
.muted {
  color: var(--re-color-text-muted);
}
</style>
