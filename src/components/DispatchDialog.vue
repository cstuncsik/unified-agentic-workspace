<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { useProjectsStore } from "../stores/projects";
import { useRepositoriesStore } from "../stores/repositories";
import { useRepositoryBranches } from "../composables/useRepositoryBranches";
import { slugifyBranch } from "../utils/slug";
import { useToast } from "../composables/useToast";
import * as api from "../api/dispatch";
import type { DispatchTaskResult } from "../api/dispatch";

const props = defineProps<{ open: boolean; artifactId: string | null }>();
const emit = defineEmits<{ close: []; dispatched: [] }>();

const projects = useProjectsStore();
const repositories = useRepositoriesStore();
const { branches, baseBranch, selectRepo } = useRepositoryBranches();
const toast = useToast();

const dialog = ref<HTMLDialogElement | null>(null);
const projectId = ref("");
const repoId = ref("");
const submitting = ref(false);

interface Row {
  title: string;
  branch: string;
  include: boolean;
}
const rows = ref<Row[]>([]);
const results = ref<DispatchTaskResult[] | null>(null);

const codeProjects = computed(() =>
  projects.list.filter((p) => p.mode === "code" || p.mode === "mixed"),
);
const ready = computed(() => codeProjects.value.length > 0 && repositories.list.length > 0);
const includedCount = computed(() => rows.value.filter((r) => r.include).length);
const canDispatch = computed(
  () =>
    ready.value &&
    projectId.value !== "" &&
    repoId.value !== "" &&
    baseBranch.value !== "" &&
    includedCount.value > 0 &&
    !submitting.value,
);

watch(repoId, (id) => void selectRepo(id));

// When opened, load the artifact's tasks and reset state.
watch(
  () => props.open,
  async (open) => {
    if (!open || !props.artifactId) return;
    results.value = null;
    projectId.value = codeProjects.value[0]?.id ?? "";
    repoId.value = repositories.list[0]?.id ?? "";
    try {
      const tasks = await api.extractArtifactTasks(props.artifactId);
      rows.value =
        tasks.length > 0
          ? tasks.map((t) => ({ title: t, branch: slugifyBranch(t), include: true }))
          : [{ title: "", branch: "", include: true }];
    } catch (e) {
      toast.error(String(e));
      rows.value = [{ title: "", branch: "", include: true }];
    }
    dialog.value?.showModal();
  },
);

function onTitleInput(row: Row) {
  // Keep the branch in sync until the user hand-edits it (simple: always reslug).
  row.branch = slugifyBranch(row.title);
}

function close() {
  dialog.value?.close();
  emit("close");
}

async function dispatch() {
  if (!canDispatch.value || !props.artifactId) return;
  submitting.value = true;
  try {
    const res = await api.dispatchArtifact(
      props.artifactId,
      projectId.value,
      repoId.value,
      baseBranch.value,
      rows.value.map((r) => ({
        title: r.title.trim(),
        branch_name: r.branch.trim(),
        include: r.include,
      })),
    );
    results.value = res.results;
    const ok = res.results.filter((r) => !r.error).length;
    toast.success(`Dispatched ${ok}/${res.results.length} task(s)`);
    emit("dispatched");
  } catch (e) {
    toast.error(String(e));
  } finally {
    submitting.value = false;
  }
}
</script>

<template>
  <dialog
    ref="dialog"
    class="re-dialog dispatch"
    data-testid="dispatch-dialog"
    @close="emit('close')"
  >
    <header class="re-dialog__header">
      <h2 class="re-dialog__title">Dispatch to coding tasks</h2>
    </header>
    <div class="re-dialog__body">
      <p v-if="!ready" class="muted">
        Need a code/mixed project and an attached repository (Sources) to dispatch.
      </p>
      <template v-else-if="!results">
        <div class="pickers">
          <select
            v-model="projectId"
            class="re-select"
            data-size="sm"
            aria-label="Dispatch project"
          >
            <option value="" disabled>Project</option>
            <option v-for="p in codeProjects" :key="p.id" :value="p.id">{{ p.name }}</option>
          </select>
          <select
            v-model="repoId"
            class="re-select"
            data-size="sm"
            aria-label="Dispatch repository"
          >
            <option value="" disabled>Repository</option>
            <option v-for="r in repositories.list" :key="r.id" :value="r.id">{{ r.name }}</option>
          </select>
          <select
            v-model="baseBranch"
            class="re-select"
            data-size="sm"
            aria-label="Dispatch base branch"
            :disabled="branches.length === 0"
          >
            <option value="" disabled>Base branch</option>
            <option v-for="b in branches" :key="b" :value="b">{{ b }}</option>
          </select>
        </div>
        <ul class="tasks">
          <li v-for="(row, i) in rows" :key="i" class="task" data-testid="dispatch-task-row">
            <label class="re-field re-field--inline">
              <input
                type="checkbox"
                class="re-checkbox"
                v-model="row.include"
                :aria-label="`Include task ${i + 1}`"
              />
            </label>
            <input
              v-model="row.title"
              class="re-input"
              type="text"
              :aria-label="`Task ${i + 1} title`"
              placeholder="Task title"
              @input="onTitleInput(row)"
            />
            <input
              v-model="row.branch"
              class="re-input task__branch"
              type="text"
              :aria-label="`Task ${i + 1} branch`"
              placeholder="branch-name"
            />
          </li>
        </ul>
      </template>
      <ul v-else class="results">
        <li v-for="(r, i) in results" :key="i" :class="{ 'result--err': r.error }">
          {{ r.error ? "✗" : "✓" }} {{ r.title }}
          <span v-if="r.error" class="muted">— {{ r.error }}</span>
          <span v-else class="muted">— worktree created</span>
        </li>
      </ul>
    </div>
    <div class="re-dialog__footer">
      <button type="button" class="re-button" data-variant="ghost" @click="close">
        {{ results ? "Close" : "Cancel" }}
      </button>
      <button
        v-if="!results && ready"
        type="button"
        class="re-button"
        data-variant="brand"
        :disabled="!canDispatch"
        @click="dispatch"
      >
        Dispatch {{ includedCount }} task(s)
      </button>
    </div>
  </dialog>
</template>

<style scoped>
.dispatch {
  min-width: 32rem;
  max-width: 44rem;
}
.pickers {
  display: flex;
  gap: 0.4rem;
  margin-bottom: 0.6rem;
  flex-wrap: wrap;
}
.tasks {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
  max-height: 22rem;
  overflow: auto;
}
.task {
  display: flex;
  align-items: center;
  gap: 0.4rem;
}
.task .re-input {
  flex: 1;
}
.task__branch {
  flex: 0 1 12rem;
  font-family: ui-monospace, monospace;
}
.results {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  font-size: 0.85rem;
}
.result--err {
  color: var(--re-color-danger-text);
}
.muted {
  color: var(--re-color-text-muted);
}
</style>
