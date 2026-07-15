<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useAgentSessionsStore } from "../stores/agentSessions";
import { useCodingWorkspacesStore } from "../stores/codingWorkspaces";
import { useWorkspacesStore } from "../stores/workspaces";
import { useProviderAccountsStore } from "../stores/providerAccounts";
import { useAccountModelsStore } from "../stores/accountModels";
import { useToast } from "../composables/useToast";
import * as codingApi from "../api/codingWorkspaces";
import TerminalTab from "./TerminalTab.vue";
import SdkRunView from "./SdkRunView.vue";

const store = useAgentSessionsStore();
const coding = useCodingWorkspacesStore();
const workspaces = useWorkspacesStore();
const providerAccounts = useProviderAccountsStore();
const accountModels = useAccountModelsStore();
const toast = useToast();

const newWorktreeId = ref("");
const newAdapterId = ref("");
const newAccountId = ref("");
const newGoal = ref("");
const newMode = ref("plan");
const newModelId = ref("");
const starting = ref(false);

// Goal prefill from a dispatched worktree. `seededValue` is the last value we
// seeded (for the dirty-check); `goalToken` (non-reactive, like the store's
// loadToken) makes "last prefill started wins" deterministic across async fetches.
const seededValue = ref("");
const seeded = computed(() => newGoal.value === seededValue.value && seededValue.value !== "");
const goalBytes = computed(() => new TextEncoder().encode(newGoal.value).length);
const goalKb = computed(() => Math.round(goalBytes.value / 1000));
const goalTooLarge = computed(() => selectedIsSdk.value && goalBytes.value > 100_000);
let goalToken = 0;

onMounted(async () => {
  await store.loadAdapters();
  if (store.adapters.length > 0) newAdapterId.value = store.adapters[0].id;
});

const accountModelOptions = computed(() => accountModels.modelsByAccount[newAccountId.value] ?? []);
const modelsLoading = computed(() => accountModels.loadingByAccount[newAccountId.value] ?? false);
const modelsError = computed(() => accountModels.errorByAccount[newAccountId.value] ?? null);

const adapterKind = (id: string) => store.adapters.find((a) => a.id === id)?.kind ?? "pty";
const selectedIsSdk = computed(() => adapterKind(newAdapterId.value) === "sdk");

const canStart = computed(
  () =>
    newWorktreeId.value !== "" &&
    newAdapterId.value !== "" &&
    !starting.value &&
    (!selectedIsSdk.value || (newGoal.value.trim() !== "" && newAccountId.value !== "")),
);

const worktreeLabel = (id: string) => {
  const cw = coding.list.find((c) => c.id === id);
  return cw ? cw.branch_name : id;
};
const adapterLabel = (id: string) => store.adapters.find((a) => a.id === id)?.name ?? id;

const adapterProvider = (id: string) => store.adapters.find((a) => a.id === id)?.provider ?? null;
const adapterSupportsAccounts = computed(() => adapterProvider(newAdapterId.value) !== null);
const accountOptions = computed(() =>
  providerAccounts.list.filter((a) => a.provider === adapterProvider(newAdapterId.value)),
);
const accountLabel = (id: string | null) =>
  id ? (providerAccounts.list.find((a) => a.id === id)?.display_name ?? "") : "";

// Reset the chosen account when the adapter changes (its provider — and thus the
// valid accounts — differ); a stale account would fail the provider check.
watch(newAdapterId, (val) => {
  newAccountId.value = "";
  newGoal.value = "";
  newMode.value = "plan";
  newModelId.value = "";
  seededValue.value = "";
  // Switched into the SDK with a worktree already chosen → seed now. Use
  // adapterKind(val) (not the lazy selectedIsSdk) so correctness doesn't depend on
  // computed-evaluation timing.
  if (adapterKind(val) === "sdk" && newWorktreeId.value) prefillGoal(newWorktreeId.value);
});

// When the account changes, reset the model and lazy-load that account's models
// (only for an SDK adapter with a worktree selected — the command needs both).
watch(newAccountId, (val) => {
  newModelId.value = "";
  if (val && selectedIsSdk.value && newWorktreeId.value) {
    accountModels.loadModels(newWorktreeId.value, val);
  }
});

// On worktree change (SDK): lazy-load that account's models (if an account is set)
// and seed the goal from the dispatched task.
watch(newWorktreeId, (val) => {
  if (val && selectedIsSdk.value) {
    if (newAccountId.value) accountModels.loadModels(val, newAccountId.value);
    prefillGoal(val);
  }
});

// Agent tabs persist in memory for the whole app session, but each belongs to one
// workspace. Only show the current workspace's terminals; the others stay mounted
// (hidden) and reappear when you switch back.
const visibleTabs = computed(() =>
  store.tabs.filter((t) => t.session.workspace_id === workspaces.currentId),
);

// On a workspace switch, clear the stale worktree selection and re-point the
// active tab at the new workspace. We remember the tab that was focused in each
// workspace so switching back restores *that* tab (not just the last one), and
// only fall back to the last visible tab when there's nothing to restore.
watch(
  () => workspaces.currentId,
  (newId, oldId) => {
    if (oldId) store.lastActiveByWorkspace[oldId] = store.activeId;
    newWorktreeId.value = "";
    newAccountId.value = "";
    newGoal.value = "";
    newMode.value = "plan";
    newModelId.value = "";
    seededValue.value = "";

    const remembered = newId ? store.lastActiveByWorkspace[newId] : null;
    if (remembered && visibleTabs.value.some((t) => t.session.id === remembered)) {
      store.activeId = remembered;
    } else if (!visibleTabs.value.some((t) => t.session.id === store.activeId)) {
      store.activeId = visibleTabs.value.length
        ? visibleTabs.value[visibleTabs.value.length - 1].session.id
        : null;
    }
  },
);

// Seed the goal from a dispatched worktree (SDK-only). Best-effort: a failed fetch
// never clobbers or toasts. Dirty-checked so it never overwrites the user's edits.
async function prefillGoal(id: string) {
  if (!id) return;
  const token = ++goalToken;
  let goal: string | null = null;
  try {
    goal = await codingApi.getDispatchedGoal(id);
  } catch {
    return;
  }
  if (token !== goalToken) return; // a newer prefill superseded us
  if (newWorktreeId.value !== id || !selectedIsSdk.value) return;
  if (newGoal.value === seededValue.value || newGoal.value.trim() === "") {
    newGoal.value = goal ?? "";
    seededValue.value = goal ?? "";
  }
}

async function openTerminal() {
  if (!canStart.value) return;
  starting.value = true;
  try {
    // 80x24 is a safe initial size; the TerminalTab fits + resizes on mount.
    await store.start(
      newWorktreeId.value,
      newAdapterId.value,
      newAccountId.value || null,
      selectedIsSdk.value ? newGoal.value.trim() || null : null,
      selectedIsSdk.value ? newMode.value : null,
      selectedIsSdk.value ? newModelId.value || null : null,
      80,
      24,
    );
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
        <select
          v-if="adapterSupportsAccounts"
          v-model="newAccountId"
          class="re-select"
          data-size="sm"
          aria-label="Provider account"
        >
          <option value="">Default (no key)</option>
          <option v-for="acct in accountOptions" :key="acct.id" :value="acct.id">
            {{ acct.display_name }}
          </option>
        </select>
        <p
          v-if="!adapterSupportsAccounts && !selectedIsSdk"
          class="muted new__hint"
          data-testid="pty-ambient-hint"
        >
          This agent uses your own CLI login. Accounts apply to the SDK agent only.
        </p>
        <select
          v-if="selectedIsSdk"
          v-model="newMode"
          class="re-select"
          data-size="sm"
          aria-label="Agent mode"
        >
          <option value="plan">Plan</option>
          <option value="edit">Edit</option>
        </select>
        <select
          v-if="selectedIsSdk"
          v-model="newModelId"
          class="re-select"
          data-size="sm"
          aria-label="Agent model"
          :disabled="modelsLoading"
        >
          <option value="">
            {{ modelsLoading ? "Loading models…" : "Default (SDK chooses)" }}
          </option>
          <option v-for="m in accountModelOptions" :key="m.id" :value="m.id">
            {{ m.display_name }}
          </option>
        </select>
        <p v-if="selectedIsSdk && modelsError" class="muted new__hint">
          models unavailable — check your API key
        </p>
        <textarea
          v-if="selectedIsSdk"
          v-model="newGoal"
          class="re-input new__goal"
          :rows="seeded ? 8 : 2"
          placeholder="What should the agent do?"
          aria-label="Agent goal"
        ></textarea>
        <p v-if="seeded" class="muted new__hint" data-testid="goal-seeded-hint">
          Prefilled from the dispatched task — editable.
        </p>
        <p v-if="goalTooLarge" class="muted new__hint" data-testid="goal-too-large">
          Plan is large (~{{ goalKb }} KB) — trim before starting; very large goals can fail to
          launch.
        </p>
        <p v-if="selectedIsSdk && newMode === 'edit'" class="muted new__hint">
          Edit mode applies file changes but can't run builds or tests; the review verifies.
        </p>
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
          <span class="muted">
            {{ t.session.command }} · {{ t.session.status }}
            <template v-if="t.session.kind === 'sdk' && accountLabel(t.session.account_id)">
              · {{ accountLabel(t.session.account_id) }}
            </template>
          </span>
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
        <SdkRunView v-if="t.session.kind === 'sdk'" :session="t.session" />
        <TerminalTab
          v-else-if="t.session.kind === 'pty'"
          :session-id="t.session.id"
          :active="t.session.id === store.activeId"
        />
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
  flex: 1;
  min-height: 0;
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
  flex-wrap: wrap;
  gap: 0.35rem;
}
.new__goal {
  flex-basis: 100%;
  resize: vertical;
}
.new__hint {
  flex-basis: 100%;
  font-size: 0.8rem;
  margin: 0;
}
.agents__pane {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.agents__term {
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
  flex: 1;
  min-height: 0;
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
