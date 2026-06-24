<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useAgentSessionsStore } from "../stores/agentSessions";
import { useCodingWorkspacesStore } from "../stores/codingWorkspaces";
import { useReviewsStore } from "../stores/reviews";
import { useToast } from "../composables/useToast";
import type { AgentSession, SdkEvent } from "../types/agentSession";
import type { Review } from "../types/review";

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
// The agent is "done" when it emits a terminal event (result/error) — its logical
// completion, which is more reliable than the OS process-exit event (status flips on
// `agent-exit`, which can lag badly until the child is reaped, especially under CI
// load). Either signal counts.
const completed = computed(
  () => finished.value || events.value.some((e) => e.type === "result" || e.type === "error"),
);
const diff = computed(() => coding.diffs[props.session.coding_workspace_id]);
const changedCount = computed(() => diff.value?.changed_files.length ?? 0);
// Collapses the CTA after a successful completion so a second click can't double-
// complete. Durable for the app session: AgentsView keeps tabs mounted (v-show, never
// remounted), so this flag survives tab/workspace switches.
const reviewed = ref(false);
const showReview = computed(
  () =>
    isEdit.value &&
    completed.value &&
    !!diff.value &&
    !diff.value.is_clean &&
    !diff.value.error &&
    !reviewed.value,
);
// Surface a diff-load failure for a completed edit run rather than silently hiding
// the footer (showReview already excludes the error case).
const diffError = computed(() =>
  isEdit.value && completed.value ? (diff.value?.error ?? null) : null,
);
const completing = ref(false);
// The review this run created, and whether its async checks are still running
// (drives the footer + the Reviews "running checks…" indicator via the store).
const createdReviewId = ref<string | null>(null);
const rechecking = computed(
  () => createdReviewId.value !== null && reviews.rechecking[createdReviewId.value] === true,
);

// One ordered place for completion handling. When an edit run finishes, REFRESH the
// diff once on the completion transition — the cached diff is often a stale *clean*
// snapshot taken when the worktree was created (CodingView.createWorktree calls
// refreshDiff), so we must re-read it to see the agent's changes; also re-fetch if a
// workspace switch wiped coding.diffs. Then, once the fresh diff shows changes,
// auto-create the review. `showReview` stays purely presentational; the
// reviewed/completing guards keep this to one review per session.
watch(
  [completed, diff],
  async ([done, d], prev) => {
    if (!done || !isEdit.value || reviewed.value) return;
    const justCompleted = prev ? !prev[0] : false;
    if (justCompleted || !d) {
      await coding.refreshDiff(props.session.coding_workspace_id);
      return;
    }
    if (!d.is_clean && !d.error && !completing.value) {
      await reviewChanges();
    }
  },
  { immediate: true },
);

// Auto-create the review from the diff snapshot (instant, no check), then run the
// project's check asynchronously and update the review in place. Also the manual
// retry if the creation step errors. Idempotent per session via completing/reviewed.
async function reviewChanges() {
  if (completing.value) return;
  completing.value = true;
  let review: Review;
  try {
    review = await coding.complete(props.session.coding_workspace_id, false);
  } catch (e) {
    toast.error(String(e));
    return;
  } finally {
    completing.value = false;
  }
  reviews.insert(review);
  createdReviewId.value = review.id;
  reviewed.value = true;
  toast.success("Review created — see Reviews");

  // Async checks: fill in check results without blocking the review. Best-effort —
  // the review stands even if the check can't run.
  reviews.setRechecking(review.id, true);
  try {
    reviews.insert(await coding.recheck(review.id));
  } catch {
    /* leave the review without check results */
  } finally {
    reviews.setRechecking(review.id, false);
  }
}
</script>

<template>
  <div class="sdk-wrap">
    <p class="muted sdk-model" data-testid="sdk-model">
      Model: {{ session.model_id ?? "Default" }}
    </p>
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
      <span>✓ Review created — {{ rechecking ? "running checks…" : "see Reviews" }}</span>
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
.sdk-model {
  margin: 0;
  padding: 0.25rem 0.5rem;
  font-size: 0.8rem;
}
.muted {
  color: var(--re-color-text-muted);
}
</style>
