<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { useReviewsStore } from "../stores/reviews";
import { useCodingWorkspacesStore } from "../stores/codingWorkspaces";
import { reviewTone } from "../utils/reviewTone";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ close: [] }>();

const reviews = useReviewsStore();
const coding = useCodingWorkspacesStore();

const dialog = ref<HTMLDialogElement | null>(null);
const selectedIds = ref<string[]>([]);

const codingLabel = (codingWorkspaceId: string) => {
  const cw = coding.list.find((c) => c.id === codingWorkspaceId);
  return cw ? cw.branch_name : "worktree";
};

const selected = computed(() => reviews.list.filter((r) => selectedIds.value.includes(r.id)));

function toggle(id: string, on: boolean) {
  selectedIds.value = on ? [...selectedIds.value, id] : selectedIds.value.filter((x) => x !== id);
}

watch(
  () => props.open,
  (open) => {
    if (open) {
      selectedIds.value = [];
      dialog.value?.showModal();
    }
  },
);

function close() {
  dialog.value?.close();
  emit("close");
}
</script>

<template>
  <dialog
    ref="dialog"
    class="re-dialog compare"
    data-testid="compare-dialog"
    @close="emit('close')"
  >
    <header class="re-dialog__header">
      <h2 class="re-dialog__title">Compare reviews</h2>
    </header>
    <div class="re-dialog__body">
      <p v-if="reviews.list.length === 0" class="muted">No reviews to compare yet.</p>
      <template v-else>
        <ul class="picker">
          <li v-for="r in reviews.list" :key="r.id" class="picker__row" data-testid="compare-pick">
            <label class="re-field re-field--inline">
              <input
                type="checkbox"
                class="re-checkbox"
                :checked="selectedIds.includes(r.id)"
                :aria-label="`Compare ${codingLabel(r.coding_workspace_id)} review`"
                @change="toggle(r.id, ($event.target as HTMLInputElement).checked)"
              />
              <span>{{ codingLabel(r.coding_workspace_id) }} · {{ r.summary }}</span>
            </label>
          </li>
        </ul>
        <div v-if="selected.length >= 2" class="compare__grid" data-testid="compare-grid">
          <article v-for="r in selected" :key="r.id" class="compare__col">
            <h4 class="compare__head">
              {{ codingLabel(r.coding_workspace_id) }}
              <span class="re-badge" :data-tone="reviewTone(r.status)">{{ r.status }}</span>
            </h4>
            <p class="compare__summary">{{ r.summary }}</p>
            <ul v-if="r.risk_notes.length" class="compare__risk">
              <li v-for="(n, i) in r.risk_notes" :key="i">{{ n }}</li>
            </ul>
            <pre class="compare__pre">{{ r.diff_stat || "(no diff)" }}</pre>
            <ul class="compare__files">
              <li v-for="f in r.files" :key="f">{{ f }}</li>
            </ul>
          </article>
        </div>
        <p v-else class="muted">Select two or more reviews to compare.</p>
      </template>
    </div>
    <div class="re-dialog__footer">
      <button type="button" class="re-button" data-variant="ghost" @click="close">Close</button>
    </div>
  </dialog>
</template>

<style scoped>
.compare {
  min-width: 34rem;
  max-width: 56rem;
}
.picker {
  list-style: none;
  margin: 0 0 0.6rem;
  padding: 0;
  max-height: 12rem;
  overflow: auto;
}
.compare__grid {
  display: grid;
  grid-auto-flow: column;
  grid-auto-columns: minmax(16rem, 1fr);
  gap: 0.6rem;
  overflow-x: auto;
}
.compare__col {
  border: 1px solid var(--re-color-border);
  border-radius: var(--re-radius-md, 6px);
  padding: 0.5rem 0.6rem;
}
.compare__head {
  margin: 0 0 0.3rem;
  display: flex;
  align-items: center;
  gap: 0.4rem;
  font-size: 0.85rem;
}
.compare__summary {
  margin: 0 0 0.3rem;
  font-size: 0.8rem;
}
.compare__risk {
  margin: 0 0 0.3rem;
  padding-left: 1rem;
  font-size: 0.78rem;
  color: var(--re-color-danger-text);
}
.compare__pre {
  margin: 0 0 0.3rem;
  font-size: 0.72rem;
  white-space: pre;
  overflow: auto;
  max-height: 12rem;
  background: var(--re-color-bg-muted);
  border-radius: var(--re-radius-md, 6px);
  padding: 0.4rem 0.5rem;
}
.compare__files {
  list-style: none;
  margin: 0;
  padding: 0;
  font-family: ui-monospace, monospace;
  font-size: 0.72rem;
  color: var(--re-color-text-muted);
}
.muted {
  color: var(--re-color-text-muted);
}
</style>
