<script setup lang="ts">
import { computed, ref } from "vue";
import { useWorkspacesStore } from "../stores/workspaces";
import { useRepositoriesStore } from "../stores/repositories";
import { useToast } from "../composables/useToast";
import { useConfirm } from "../composables/useConfirm";
import type { GitInspection } from "../types/repository";

const workspaces = useWorkspacesStore();
const repositories = useRepositoriesStore();
const toast = useToast();
const { confirm } = useConfirm();

const newName = ref("");
const newPath = ref("");
const submitting = ref(false);
const preview = ref<GitInspection | null>(null);

const canAttach = computed(() => newName.value.trim() !== "" && newPath.value.trim() !== "");

async function validatePath() {
  const path = newPath.value.trim();
  if (!path) return;
  preview.value = null;
  try {
    preview.value = await repositories.validate(path);
  } catch (e) {
    toast.error(String(e));
  }
}

async function attach() {
  const name = newName.value.trim();
  const localPath = newPath.value.trim();
  if (!name || !localPath || !workspaces.currentId) return;
  submitting.value = true;
  try {
    await repositories.create({ workspaceId: workspaces.currentId, name, localPath });
    newName.value = "";
    newPath.value = "";
    preview.value = null;
    toast.success("Repository attached");
  } catch (e) {
    toast.error(String(e));
  } finally {
    submitting.value = false;
  }
}

async function removeRepo(id: string, name: string) {
  const confirmed = await confirm(
    `Detach repository "${name}"? The folder on disk is not touched.`,
    "Detach repository",
  );
  if (!confirmed) return;
  try {
    await repositories.remove(id);
    toast.success("Repository detached");
  } catch (e) {
    toast.error(String(e));
  }
}

function statusLabel(status: GitInspection | undefined): string {
  if (!status) return "checking…";
  if (!status.is_git_repo) return status.error ?? "not a git repository";
  const branch = status.current_branch ?? "unknown";
  return `${branch} · ${status.is_dirty ? "uncommitted changes" : "clean"}`;
}
</script>

<template>
  <section>
    <h2 class="view-title">Sources</h2>
    <h3 class="section-title">Git Repositories</h3>

    <form class="attach" @submit.prevent="attach">
      <input
        v-model="newName"
        class="re-input"
        type="text"
        placeholder="Name"
        aria-label="Repository name"
      />
      <input
        v-model="newPath"
        class="re-input attach__path"
        type="text"
        placeholder="/absolute/path/to/repo"
        aria-label="Repository path"
      />
      <button
        class="re-button"
        data-variant="ghost"
        type="button"
        :disabled="!newPath.trim()"
        @click="validatePath"
      >
        Validate
      </button>
      <button
        class="re-button"
        data-variant="brand"
        type="submit"
        :disabled="submitting || !canAttach"
      >
        Attach
      </button>
    </form>

    <p v-if="preview" class="preview" :class="preview.is_git_repo ? 'preview--ok' : 'preview--bad'">
      <template v-if="preview.is_git_repo">
        ✓ Git repository · branch {{ preview.current_branch ?? "unknown" }} · default
        {{ preview.default_branch ?? "unknown" }} ·
        {{ preview.is_dirty ? "uncommitted changes" : "clean" }}
      </template>
      <template v-else> ✗ {{ preview.error ?? "Not a git repository" }} </template>
    </p>

    <p v-if="repositories.loading" class="muted">Loading repositories…</p>
    <p v-else-if="repositories.error" class="error">{{ repositories.error }}</p>
    <p v-else-if="repositories.list.length === 0" class="muted">
      No repositories yet. Attach a local git repository to start coding sessions against it.
    </p>
    <ul v-else class="rows">
      <li
        v-for="repo in repositories.list"
        :key="repo.id"
        class="re-card"
        data-testid="repository-row"
      >
        <span class="repo__main">
          <span class="repo__name">{{ repo.name }}</span>
          <span class="repo__path">{{ repo.local_path }}</span>
        </span>
        <span class="repo__meta">
          <span class="re-badge">{{ repo.default_branch }}</span>
          <span
            class="repo__status"
            :class="{ 'repo__status--dirty': repositories.statuses[repo.id]?.is_dirty }"
          >
            {{ statusLabel(repositories.statuses[repo.id]) }}
          </span>
        </span>
        <button
          type="button"
          class="re-button"
          data-variant="ghost"
          data-size="sm"
          @click="repositories.refreshStatus(repo.id)"
        >
          Refresh
        </button>
        <button
          type="button"
          class="re-button"
          data-variant="danger"
          data-size="sm"
          @click="removeRepo(repo.id, repo.name)"
        >
          Detach
        </button>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.view-title {
  margin: 0 0 0.25rem;
  font-size: 1.2rem;
}

.section-title {
  margin: 0 0 1rem;
  font-size: 0.8rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--re-color-text-muted);
}

.attach {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
  margin-bottom: 0.75rem;
}

.attach__path {
  flex: 1;
  min-width: 16rem;
}

.preview {
  margin: 0 0 1rem;
  font-size: 0.85rem;
}

.preview--ok {
  color: var(--re-color-success-text, var(--re-color-text));
}

.preview--bad {
  color: var(--re-color-text-danger);
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
  flex-direction: row;
  align-items: center;
  gap: 0.6rem;
  padding: 0.6rem 0.85rem;
}

.repo__main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 0.2rem;
}

.repo__name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.repo__path {
  font-size: 0.75rem;
  color: var(--re-color-text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.repo__meta {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.repo__status {
  font-size: 0.75rem;
  color: var(--re-color-text-muted);
}

.repo__status--dirty {
  color: var(--re-color-warning-text, var(--re-color-text));
}

.muted {
  color: var(--re-color-text-muted);
}

.error {
  color: var(--re-color-text-danger);
}
</style>
