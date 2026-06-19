<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useWorkspacesStore } from "../stores/workspaces";
import { useProjectsStore } from "../stores/projects";
import { useRepositoriesStore } from "../stores/repositories";
import { useCodingWorkspacesStore } from "../stores/codingWorkspaces";
import { useReviewsStore } from "../stores/reviews";
import { useToast } from "../composables/useToast";
import { useConfirm } from "../composables/useConfirm";
import { useRepositoryBranches } from "../composables/useRepositoryBranches";

const workspaces = useWorkspacesStore();
const projects = useProjectsStore();
const repositories = useRepositoriesStore();
const coding = useCodingWorkspacesStore();
const reviews = useReviewsStore();
const toast = useToast();
const { confirm } = useConfirm();

const newProjectId = ref("");
const newRepoId = ref("");
const newBranchName = ref("");
const submitting = ref(false);
const expandedId = ref<string | null>(null);
const completingId = ref<string | null>(null);

// Worktrees can only be created for repo-backed projects.
const codeProjects = computed(() =>
  projects.list.filter((p) => p.mode === "code" || p.mode === "mixed"),
);

const projectNames = computed(() => new Map(projects.list.map((p) => [p.id, p.name])));
const repoNames = computed(() => new Map(repositories.list.map((r) => [r.id, r.name])));

const canCreate = computed(
  () =>
    newProjectId.value !== "" &&
    newRepoId.value !== "" &&
    newBaseBranch.value !== "" &&
    newBranchName.value.trim() !== "",
);

// Repo→branches loading + default base live in a shared composable (also used by
// the dispatch dialog). The watch only surfaces errors; the composable owns the
// monotonic-token staleness guard.
const branchHelper = useRepositoryBranches();
const branches = branchHelper.branches;
const newBaseBranch = branchHelper.baseBranch;
watch(newRepoId, async (repoId) => {
  try {
    await branchHelper.selectRepo(repoId);
  } catch (e) {
    toast.error(String(e));
  }
});

// A workspace switch resets the form (repos/projects belong to one workspace).
watch(
  () => workspaces.currentId,
  () => {
    newProjectId.value = "";
    newRepoId.value = "";
    newBranchName.value = "";
  },
);

// Default the project/repository selects to the first available option once the
// lists load, so a workspace with a single project/repo is immediately ready to
// create a worktree (rather than leaving the Create button silently disabled
// because the project select is still on its placeholder).
watch(
  [codeProjects, () => repositories.list.length],
  () => {
    if (newProjectId.value === "" && codeProjects.value.length > 0) {
      newProjectId.value = codeProjects.value[0].id;
    }
    if (newRepoId.value === "" && repositories.list.length > 0) {
      newRepoId.value = repositories.list[0].id;
    }
  },
  { immediate: true },
);

async function createWorktree() {
  if (!canCreate.value) return;
  submitting.value = true;
  try {
    const cw = await coding.create({
      projectId: newProjectId.value,
      repositorySourceId: newRepoId.value,
      baseBranch: newBaseBranch.value,
      branchName: newBranchName.value.trim(),
    });
    newBranchName.value = "";
    await coding.refreshDiff(cw.id);
    toast.success("Worktree created");
  } catch (e) {
    toast.error(String(e));
  } finally {
    submitting.value = false;
  }
}

async function toggleDiff(id: string) {
  if (expandedId.value === id) {
    expandedId.value = null;
    return;
  }
  expandedId.value = id;
  try {
    await coding.refreshDiff(id);
  } catch (e) {
    toast.error(String(e));
  }
}

async function markReady(id: string) {
  try {
    await coding.markReady(id);
    toast.success("Marked ready for review");
  } catch (e) {
    toast.error(String(e));
  }
}

async function createReview(id: string) {
  try {
    await reviews.createForCodingWorkspace(id);
    toast.success("Review created — see Reviews");
  } catch (e) {
    toast.error(String(e));
  }
}

async function completeAndReview(id: string) {
  completingId.value = id;
  try {
    const review = await coding.complete(id);
    reviews.insert(review);
    toast.success("Completed — review ready in Reviews");
  } catch (e) {
    toast.error(String(e));
  } finally {
    completingId.value = null;
  }
}

async function discardWorktree(id: string, branch: string) {
  // Refresh the diff so the confirmation can warn about uncommitted work.
  await coding.refreshDiff(id);
  const diff = coding.diffs[id];
  const uncertain = !diff || diff.error != null;
  const dirty = diff ? !diff.is_clean : false;
  const message = uncertain
    ? `Discard worktree "${branch}"? Its state could not be read; any uncommitted changes will be lost. The branch is kept.`
    : dirty
      ? `Discard worktree "${branch}"? It has uncommitted changes that will be lost. The branch is kept.`
      : `Discard worktree "${branch}"? The branch is kept; only the working tree is removed.`;
  if (!(await confirm(message, "Discard worktree", "Discard"))) return;
  try {
    await coding.discard(id, true);
    if (expandedId.value === id) expandedId.value = null;
    toast.success("Worktree discarded");
  } catch (e) {
    toast.error(String(e));
  }
}
</script>

<template>
  <section>
    <h2 class="view-title">Coding</h2>
    <h3 class="section-title">Worktrees</h3>

    <form class="create" @submit.prevent="createWorktree">
      <select v-model="newProjectId" class="re-select" aria-label="Coding project">
        <option value="" disabled>Project</option>
        <option v-for="p in codeProjects" :key="p.id" :value="p.id">{{ p.name }}</option>
      </select>
      <select v-model="newRepoId" class="re-select" aria-label="Coding repository">
        <option value="" disabled>Repository</option>
        <option v-for="r in repositories.list" :key="r.id" :value="r.id">{{ r.name }}</option>
      </select>
      <select
        v-model="newBaseBranch"
        class="re-select"
        aria-label="Base branch"
        :disabled="branches.length === 0"
      >
        <option value="" disabled>Base branch</option>
        <option v-for="b in branches" :key="b" :value="b">{{ b }}</option>
      </select>
      <input
        v-model="newBranchName"
        class="re-input create__branch"
        type="text"
        placeholder="new-branch-name"
        aria-label="New branch name"
      />
      <button
        class="re-button"
        data-variant="brand"
        type="submit"
        :disabled="submitting || !canCreate"
      >
        Create worktree
      </button>
    </form>
    <p v-if="codeProjects.length === 0 || repositories.list.length === 0" class="muted hint">
      Need a code/mixed project and an attached repository (Sources) to create a worktree.
    </p>

    <p v-if="coding.loading" class="muted">Loading worktrees…</p>
    <p v-else-if="coding.error" class="error">{{ coding.error }}</p>
    <p v-else-if="coding.list.length === 0" class="muted">
      No coding workspaces yet. Create a worktree to start implementation work.
    </p>
    <ul v-else class="rows">
      <li v-for="cw in coding.list" :key="cw.id" class="re-card coding" data-testid="coding-row">
        <div class="coding__head">
          <span class="coding__main">
            <span class="coding__branch">{{ cw.branch_name }}</span>
            <span class="coding__meta">
              from {{ cw.base_branch }} · {{ repoNames.get(cw.repository_source_id) ?? "repo" }} ·
              {{ projectNames.get(cw.project_id) ?? "project" }}
            </span>
            <span class="coding__path">{{ cw.worktree_path }}</span>
          </span>
          <span class="re-badge" :data-tone="cw.status === 'needs-review' ? 'info' : undefined">
            {{ cw.status }}
          </span>
          <span class="coding__actions">
            <button
              type="button"
              class="re-button"
              data-variant="ghost"
              data-size="sm"
              @click="toggleDiff(cw.id)"
            >
              {{ expandedId === cw.id ? "Hide diff" : "View diff" }}
            </button>
            <button
              v-if="cw.status !== 'needs-review'"
              type="button"
              class="re-button"
              data-variant="secondary"
              data-size="sm"
              @click="markReady(cw.id)"
            >
              Mark ready
            </button>
            <button
              type="button"
              class="re-button"
              data-variant="secondary"
              data-size="sm"
              @click="createReview(cw.id)"
            >
              Create review
            </button>
            <button
              type="button"
              class="re-button"
              data-variant="brand"
              data-size="sm"
              :disabled="completingId === cw.id"
              @click="completeAndReview(cw.id)"
            >
              {{ completingId === cw.id ? "Running checks…" : "Complete and review" }}
            </button>
            <button
              type="button"
              class="re-button"
              data-variant="danger"
              data-size="sm"
              @click="discardWorktree(cw.id, cw.branch_name)"
            >
              Discard
            </button>
          </span>
        </div>
        <div v-if="expandedId === cw.id" class="coding__diff">
          <template v-if="coding.diffs[cw.id]">
            <p v-if="coding.diffs[cw.id].error" class="error">{{ coding.diffs[cw.id].error }}</p>
            <p v-else-if="coding.diffs[cw.id].is_clean" class="muted">
              No changes in the worktree yet.
            </p>
            <template v-else>
              <!-- changed_files includes untracked files, which the patch below omits. -->
              <ul class="diff__files">
                <li v-for="f in coding.diffs[cw.id].changed_files" :key="f">{{ f }}</li>
              </ul>
              <pre v-if="coding.diffs[cw.id].diff_stat" class="diff__stat">{{
                coding.diffs[cw.id].diff_stat
              }}</pre>
              <pre v-if="coding.diffs[cw.id].diff_text" class="diff__text">{{
                coding.diffs[cw.id].diff_text
              }}</pre>
            </template>
          </template>
          <p v-else class="muted">Loading diff…</p>
        </div>
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

.create {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
  margin-bottom: 0.5rem;
}

.create__branch {
  flex: 1;
  min-width: 12rem;
}

.hint {
  margin: 0 0 1rem;
  font-size: 0.8rem;
}

.rows {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}

.rows .coding {
  display: flex;
  flex-direction: column;
  gap: 0.6rem;
  padding: 0.6rem 0.85rem;
}

.coding__head {
  display: flex;
  flex-direction: row;
  align-items: center;
  gap: 0.6rem;
}

.coding__main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 0.15rem;
}

.coding__branch {
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.coding__meta,
.coding__path {
  font-size: 0.75rem;
  color: var(--re-color-text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.coding__actions {
  display: flex;
  gap: 0.35rem;
  flex: 0 0 auto;
}

.coding__diff {
  border-top: 1px solid var(--re-color-border);
  padding-top: 0.5rem;
}

.diff__files {
  list-style: none;
  margin: 0 0 0.5rem;
  padding: 0;
  font-size: 0.78rem;
  font-family: ui-monospace, monospace;
  color: var(--re-color-text-muted);
}

.diff__stat,
.diff__text {
  margin: 0;
  font-size: 0.75rem;
  white-space: pre;
  overflow: auto;
  max-height: 22rem;
  background: var(--re-color-bg-muted);
  border-radius: var(--re-radius-md, 6px);
  padding: 0.5rem 0.7rem;
  color: var(--re-color-text);
}

.diff__stat {
  margin-bottom: 0.35rem;
  max-height: 10rem;
}

.muted {
  color: var(--re-color-text-muted);
}

.error {
  color: var(--re-color-danger-text);
}
</style>
