<script setup lang="ts">
import { computed, ref } from "vue";
import { useReviewsStore } from "../stores/reviews";
import { useToast } from "../composables/useToast";
import { REVIEW_ACTIONS } from "../types/review";
import { reviewTone } from "../utils/reviewTone";

const reviews = useReviewsStore();
const toast = useToast();
const selectedId = ref<string | null>(null);

// Pending first, then by recency — the queue the user works top-down.
const ordered = computed(() =>
  [...reviews.list].sort((a, b) => {
    const ap = a.status === "pending" ? 0 : 1;
    const bp = b.status === "pending" ? 0 : 1;
    if (ap !== bp) return ap - bp;
    return b.created_at.localeCompare(a.created_at);
  }),
);

const selected = computed(() => reviews.list.find((r) => r.id === selectedId.value) ?? null);

async function setStatus(id: string, status: string) {
  try {
    // A null result means the review no longer exists — don't claim success for
    // a no-op (e.g. it was discarded with its worktree in another view).
    const updated = await reviews.updateStatus(id, status);
    if (updated) {
      toast.success(`Review ${status}`);
    } else {
      toast.error("Review no longer exists");
    }
  } catch (e) {
    toast.error(String(e));
  }
}
</script>

<template>
  <section>
    <h2 class="view-title">Reviews</h2>
    <h3 class="section-title">Pending and decided</h3>

    <p v-if="reviews.loading" class="muted">Loading reviews…</p>
    <p v-else-if="reviews.error" class="error">{{ reviews.error }}</p>
    <p v-else-if="reviews.list.length === 0" class="muted">
      No reviews yet. Create a review from a coding worktree to start.
    </p>
    <div v-else class="layout">
      <ul class="rows">
        <li
          v-for="r in ordered"
          :key="r.id"
          class="re-card review"
          :class="{ 'review--active': r.id === selectedId }"
          data-testid="review-row"
          @click="selectedId = r.id"
        >
          <span class="review__summary">{{ r.summary }}</span>
          <span
            v-if="reviews.rechecking[r.id]"
            class="review__checks"
            data-testid="review-rechecking"
          >
            running checks…
          </span>
          <span class="re-badge" :data-tone="reviewTone(r.status)">{{ r.status }}</span>
        </li>
      </ul>

      <div v-if="selected" class="detail re-card" data-testid="review-detail">
        <header class="detail__head">
          <span class="detail__summary">{{ selected.summary }}</span>
          <span class="re-badge" :data-tone="reviewTone(selected.status)">
            {{ selected.status }}
          </span>
        </header>

        <div class="detail__actions">
          <button
            v-for="a in REVIEW_ACTIONS"
            :key="a.status"
            type="button"
            class="re-button"
            :data-variant="a.status === 'approved' ? 'brand' : 'secondary'"
            data-size="sm"
            :disabled="selected.status === a.status"
            @click="setStatus(selected.id, a.status)"
          >
            {{ a.label }}
          </button>
        </div>

        <h4 class="detail__label">Risk notes</h4>
        <ul v-if="selected.risk_notes.length" class="detail__risk">
          <li v-for="(note, i) in selected.risk_notes" :key="i">{{ note }}</li>
        </ul>
        <p v-else class="muted">No risk flags.</p>

        <h4 class="detail__label">Files changed</h4>
        <ul v-if="selected.files.length" class="detail__files">
          <li v-for="f in selected.files" :key="f">{{ f }}</li>
        </ul>
        <p v-else class="muted">No files changed.</p>

        <h4 class="detail__label">Diff stat</h4>
        <pre v-if="selected.diff_stat" class="detail__pre">{{ selected.diff_stat }}</pre>
        <p v-else class="muted">No diff.</p>

        <h4 class="detail__label">Test output</h4>
        <pre v-if="selected.test_output" class="detail__pre">{{ selected.test_output }}</pre>
        <p v-else class="muted">Not run yet.</p>
      </div>
    </div>
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

.layout {
  display: grid;
  grid-template-columns: minmax(16rem, 22rem) 1fr;
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

.review {
  display: flex;
  flex-direction: row;
  align-items: center;
  justify-content: space-between;
  gap: 0.6rem;
  padding: 0.6rem 0.85rem;
  cursor: pointer;
}

.review--active {
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--re-color-accent-600) 45%, transparent);
}

.review__summary {
  flex: 1 1 0;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.review__checks {
  flex-shrink: 0;
  font-size: 0.75rem;
  color: var(--re-color-text-muted);
  white-space: nowrap;
}

.detail {
  padding: 0.85rem 1rem;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.detail__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.6rem;
}

.detail__summary {
  font-weight: 600;
}

.detail__actions {
  display: flex;
  flex-wrap: wrap;
  gap: 0.35rem;
  margin-bottom: 0.25rem;
}

.detail__label {
  margin: 0.5rem 0 0;
  font-size: 0.75rem;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--re-color-text-muted);
}

.detail__risk,
.detail__files {
  list-style: none;
  margin: 0;
  padding: 0;
  font-size: 0.8rem;
}

.detail__risk li {
  color: var(--re-color-danger-text);
}

.detail__files {
  font-family: ui-monospace, monospace;
  color: var(--re-color-text-muted);
}

.detail__pre {
  margin: 0;
  font-size: 0.75rem;
  white-space: pre;
  overflow: auto;
  max-height: 16rem;
  background: var(--re-color-bg-muted);
  border-radius: var(--re-radius-md, 6px);
  padding: 0.5rem 0.7rem;
  color: var(--re-color-text);
}

.muted {
  color: var(--re-color-text-muted);
}

.error {
  color: var(--re-color-danger-text);
}
</style>
