<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { useWorkspacesStore } from "../stores/workspaces";
import { useProjectsStore } from "../stores/projects";
import { useArtifactsStore } from "../stores/artifacts";
import { useToast } from "../composables/useToast";
import { useConfirm } from "../composables/useConfirm";
import { renderMarkdown } from "../utils/markdown";
import DispatchDialog from "./DispatchDialog.vue";
import * as dispatchApi from "../api/dispatch";
import { useCodingWorkspacesStore } from "../stores/codingWorkspaces";
import type { Session } from "../types/session";

const workspaces = useWorkspacesStore();
const projects = useProjectsStore();
const artifacts = useArtifactsStore();
const toast = useToast();
const { confirm } = useConfirm();
const coding = useCodingWorkspacesStore();
const dispatchOpen = ref(false);
const dispatchedSessions = ref<Session[]>([]);

const selectedId = ref<string | null>(null);
const mode = ref<"edit" | "preview">("edit");
const newTitle = ref("");
const newProjectId = ref("");

// Local edit buffer — never mutate the store object; diff by value for dirty.
const buffer = reactive({ title: "", content: "" });

const selected = computed(() => artifacts.list.find((a) => a.id === selectedId.value) ?? null);
const projectName = (id: string | null) =>
  id ? (projects.list.find((p) => p.id === id)?.name ?? "project") : null;

const dirty = computed(
  () =>
    selected.value != null &&
    (buffer.title !== selected.value.title || buffer.content !== selected.value.content),
);
const canSave = computed(() => dirty.value && buffer.title.trim() !== "");

// Reseed the buffer whenever the selected artifact changes (or its saved copy is
// replaced after a Save). value-equality dirty then clears automatically.
watch(
  selected,
  (a) => {
    buffer.title = a?.title ?? "";
    buffer.content = a?.content ?? "";
  },
  { immediate: true },
);

async function loadDispatched() {
  if (!selected.value || !workspaces.currentId) {
    dispatchedSessions.value = [];
    return;
  }
  try {
    dispatchedSessions.value = await dispatchApi.listArtifactSessions(
      workspaces.currentId,
      selected.value.id,
    );
  } catch {
    dispatchedSessions.value = [];
  }
}
watch(selected, () => void loadDispatched(), { immediate: true });

async function onDispatched() {
  await loadDispatched();
  if (workspaces.currentId) await coding.load(workspaces.currentId);
}

async function selectArtifact(id: string) {
  if (id === selectedId.value) return;
  if (dirty.value) {
    const ok = await confirm(
      "Discard unsaved changes to this artifact?",
      "Discard changes",
      "Discard",
    );
    if (!ok) return;
  }
  selectedId.value = id;
  mode.value = "edit";
}

async function createArtifact() {
  const title = newTitle.value.trim();
  if (!title || !workspaces.currentId) return;
  try {
    const a = await artifacts.create(
      workspaces.currentId,
      newProjectId.value === "" ? null : newProjectId.value,
      title,
    );
    newTitle.value = "";
    newProjectId.value = "";
    selectedId.value = a.id;
    mode.value = "edit";
    toast.success("Artifact created");
  } catch (e) {
    toast.error(String(e));
  }
}

async function save() {
  if (!selected.value || !canSave.value) return;
  try {
    await artifacts.update(selected.value.id, buffer.title.trim(), buffer.content);
    toast.success("Saved");
  } catch (e) {
    toast.error(String(e));
  }
}

async function removeArtifact(id: string, title: string) {
  if (!(await confirm(`Delete artifact "${title}"?`, "Delete artifact", "Delete"))) return;
  try {
    await artifacts.remove(id);
    if (selectedId.value === id) selectedId.value = null;
    toast.success("Artifact deleted");
  } catch (e) {
    toast.error(String(e));
  }
}
</script>

<template>
  <section>
    <h2 class="view-title">Artifacts</h2>

    <form class="create" @submit.prevent="createArtifact">
      <input
        v-model="newTitle"
        class="re-input"
        type="text"
        placeholder="New artifact title"
        aria-label="New artifact title"
      />
      <select v-model="newProjectId" class="re-select" aria-label="Artifact project">
        <option value="">No project</option>
        <option v-for="p in projects.list" :key="p.id" :value="p.id">{{ p.name }}</option>
      </select>
      <button class="re-button" data-variant="brand" type="submit" :disabled="!newTitle.trim()">
        Create
      </button>
    </form>

    <p v-if="artifacts.loading" class="muted">Loading artifacts…</p>
    <p v-else-if="artifacts.error" class="error">{{ artifacts.error }}</p>
    <p v-else-if="artifacts.list.length === 0" class="muted">
      No artifacts yet. Create a markdown document to capture research or a plan.
    </p>
    <div v-else class="layout">
      <ul class="rows">
        <li
          v-for="a in artifacts.list"
          :key="a.id"
          class="re-card artifact"
          :class="{ 'artifact--active': a.id === selectedId }"
          data-testid="artifact-row"
          @click="selectArtifact(a.id)"
        >
          <span class="artifact__title">{{ a.title }}</span>
          <span v-if="a.project_id" class="re-badge">{{ projectName(a.project_id) }}</span>
        </li>
      </ul>

      <div v-if="selected" class="editor re-card" data-testid="artifact-editor">
        <header class="editor__head">
          <input v-model="buffer.title" class="re-input" type="text" aria-label="Artifact title" />
          <span v-if="dirty" class="editor__dirty" data-testid="artifact-dirty">• Unsaved</span>
          <button
            type="button"
            class="re-button"
            data-variant="secondary"
            data-size="sm"
            data-testid="dispatch-button"
            @click="dispatchOpen = true"
          >
            Dispatch
          </button>
        </header>

        <div class="editor__bar">
          <fieldset class="re-segmented" data-size="sm" aria-label="Editor mode">
            <label class="re-segmented__option">
              <input type="radio" value="edit" v-model="mode" name="artifact-mode" />
              <span>Edit</span>
            </label>
            <label class="re-segmented__option">
              <input type="radio" value="preview" v-model="mode" name="artifact-mode" />
              <span>Preview</span>
            </label>
          </fieldset>
          <span class="editor__actions">
            <button
              type="button"
              class="re-button"
              data-variant="brand"
              data-size="sm"
              :disabled="!canSave"
              @click="save"
            >
              Save
            </button>
            <button
              type="button"
              class="re-button"
              data-variant="danger"
              data-size="sm"
              @click="removeArtifact(selected.id, selected.title)"
            >
              Delete
            </button>
          </span>
        </div>

        <textarea
          v-show="mode === 'edit'"
          v-model="buffer.content"
          class="re-textarea editor__source"
          aria-label="Markdown source"
          placeholder="# Write markdown…"
        ></textarea>
        <!-- eslint-disable-next-line vue/no-v-html -- sanitized by renderMarkdown -->
        <div
          v-show="mode === 'preview'"
          class="markdown-body"
          data-testid="artifact-preview"
          v-html="renderMarkdown(buffer.content)"
        ></div>

        <div v-if="dispatchedSessions.length" class="dispatched" data-testid="dispatched-sessions">
          <h4 class="detail__label">Dispatched: {{ dispatchedSessions.length }} coding sessions</h4>
          <ul class="dispatched__list">
            <li v-for="s in dispatchedSessions" :key="s.id">{{ s.title }} · {{ s.status }}</li>
          </ul>
        </div>
      </div>
      <p v-else class="muted">Select an artifact to edit, or create one.</p>
    </div>

    <DispatchDialog
      :open="dispatchOpen"
      :artifact-id="selected?.id ?? null"
      @close="dispatchOpen = false"
      @dispatched="onDispatched"
    />
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
.create .re-input {
  flex: 1;
}
.layout {
  display: grid;
  grid-template-columns: minmax(14rem, 20rem) 1fr;
  gap: 1rem;
  align-items: start;
}
.rows {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}
.artifact {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.6rem;
  padding: 0.6rem 0.85rem;
  cursor: pointer;
}
.artifact--active {
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--re-color-accent-600) 45%, transparent);
}
.artifact__title {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.editor {
  padding: 0.85rem 1rem;
  display: flex;
  flex-direction: column;
  gap: 0.6rem;
}
.editor__head {
  display: flex;
  align-items: center;
  gap: 0.6rem;
}
.editor__head .re-input {
  flex: 1;
  font-weight: 600;
}
.editor__dirty {
  font-size: 0.75rem;
  color: var(--re-color-warning-text);
  white-space: nowrap;
}
.editor__bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.6rem;
}
.editor__actions {
  display: flex;
  gap: 0.35rem;
}
.editor__source {
  width: 100%;
  min-height: 22rem;
  font-family: ui-monospace, monospace;
  resize: vertical;
}
.markdown-body {
  min-height: 22rem;
  font-size: 0.9rem;
  line-height: 1.5;
  overflow: auto;
}
.markdown-body :deep(h1),
.markdown-body :deep(h2),
.markdown-body :deep(h3) {
  margin: 0.6em 0 0.3em;
}
.markdown-body :deep(p) {
  margin: 0.5em 0;
}
.markdown-body :deep(pre) {
  background: var(--re-color-bg-muted);
  padding: 0.6rem 0.8rem;
  border-radius: var(--re-radius-md, 6px);
  overflow: auto;
}
.markdown-body :deep(code) {
  font-family: ui-monospace, monospace;
}
.markdown-body :deep(a) {
  color: var(--re-color-link);
}
.dispatched {
  margin-top: 0.6rem;
}
.dispatched__list {
  list-style: none;
  margin: 0.2rem 0 0;
  padding: 0;
  font-size: 0.8rem;
  color: var(--re-color-text-muted);
}
.detail__label {
  margin: 0.5rem 0 0;
  font-size: 0.75rem;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--re-color-text-muted);
}
.muted {
  color: var(--re-color-text-muted);
}
.error {
  color: var(--re-color-danger-text);
}
</style>
