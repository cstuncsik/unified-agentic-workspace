<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useAgentSessionsStore } from "../stores/agentSessions";
import { useCodingWorkspacesStore } from "../stores/codingWorkspaces";
import { useReviewsStore } from "../stores/reviews";
import { useToast } from "../composables/useToast";
import type { AgentSession, SdkEvent } from "../types/agentSession";

const props = defineProps<{ session: AgentSession }>();
const store = useAgentSessionsStore();
const coding = useCodingWorkspacesStore();
const reviews = useReviewsStore();
const toast = useToast();

const events = computed(() => store.sdkEvents[props.session.id] ?? []);
onMounted(() => store.loadSdkTranscript(props.session.id));

const tag = (e: SdkEvent) =>
  e.type === "tool"
    ? `🔧 ${e.name ?? "tool"}`
    : e.type === "result"
      ? "✓"
      : e.type === "error"
        ? "✗"
        : "";
const text = (e: SdkEvent) => e.text ?? e.summary ?? e.message ?? "";

// Completion + review-the-diff. Only edit-mode sessions can dirty the worktree, so
// only they query the diff and offer a review — a plan run over a pre-dirty worktree
// must not falsely offer one.
const isEdit = computed(() => props.session.mode === "edit");
const finished = computed(() => props.session.status !== "running");
const diff = computed(() => coding.diffs[props.session.coding_workspace_id]);
const changedCount = computed(() => diff.value?.changed_files.length ?? 0);
const reviewed = ref(false);
const showReview = computed(
  () =>
    isEdit.value &&
    finished.value &&
    !!diff.value &&
    !diff.value.is_clean &&
    !diff.value.error &&
    !reviewed.value,
);
// Surface a diff-load failure for a finished edit run rather than silently hiding
// the footer (showReview already excludes the error case).
const diffError = computed(() =>
  isEdit.value && finished.value ? (diff.value?.error ?? null) : null,
);
const completing = ref(false);

// One-shot: when an edit session finishes, fetch the worktree diff once (covers
// reopening an already-finished session via immediate).
watch(
  finished,
  async (done) => {
    if (done && isEdit.value) {
      await coding.refreshDiff(props.session.coding_workspace_id);
    }
  },
  { immediate: true },
);

async function reviewChanges() {
  if (completing.value) return;
  completing.value = true;
  try {
    const review = await coding.complete(props.session.coding_workspace_id);
    reviews.insert(review);
    reviewed.value = true; // collapse the CTA so a second click can't double-complete
    toast.success("Review created — see Reviews");
  } catch (e) {
    toast.error(String(e));
  } finally {
    completing.value = false;
  }
}
</script>

<template>
  <div class="sdk-wrap">
    <div class="sdk-feed" data-testid="agent-sdk-feed">
      <div
        v-for="(e, i) in events"
        :key="i"
        class="sdk-row"
        data-testid="sdk-event"
        :data-kind="e.type"
      >
        <span class="sdk-row__tag">{{ tag(e) }}</span>
        <span class="sdk-row__text">{{ text(e) }}</span>
      </div>
      <p v-if="events.length === 0" class="muted">Waiting for the agent…</p>
    </div>
    <footer v-if="showReview" class="sdk-foot" data-testid="sdk-review-cta">
      <span>Agent changed {{ changedCount }} file{{ changedCount === 1 ? "" : "s" }}</span>
      <button
        type="button"
        class="re-button"
        data-variant="brand"
        data-size="sm"
        :disabled="completing"
        @click="reviewChanges"
      >
        {{ completing ? "Creating review…" : "Review changes" }}
      </button>
    </footer>
    <footer v-else-if="reviewed" class="sdk-foot" data-testid="sdk-review-done">
      <span>✓ Review created — see Reviews</span>
    </footer>
    <p v-else-if="diffError" class="muted sdk-differr" data-testid="sdk-diff-error">
      Could not read changes — {{ diffError }}
    </p>
  </div>
</template>

<style scoped>
.sdk-wrap {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.sdk-feed {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 0.5rem;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}
.sdk-row {
  display: flex;
  gap: 0.5rem;
  font-size: 0.85rem;
}
.sdk-row[data-kind="error"] {
  color: var(--re-color-danger-text);
}
.sdk-row__tag {
  flex-shrink: 0;
}
.sdk-row__text {
  white-space: pre-wrap;
  word-break: break-word;
}
.sdk-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem;
  padding: 0.5rem;
  border-top: 1px solid var(--re-color-border);
  font-size: 0.85rem;
}
.sdk-differr {
  padding: 0.5rem;
  margin: 0;
  font-size: 0.8rem;
}
.muted {
  color: var(--re-color-text-muted);
}
</style>
