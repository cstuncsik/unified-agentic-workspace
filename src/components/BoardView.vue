<script setup lang="ts">
import { onMounted, ref, watch } from "vue";
import { useWorkspacesStore } from "../stores/workspaces";
import { useBoardStore } from "../stores/board";
import { BOARD_STAGES, type BoardCard } from "../types/board";
import { reviewTone } from "../utils/reviewTone";
import ReviewCompareDialog from "./ReviewCompareDialog.vue";

const workspaces = useWorkspacesStore();
const board = useBoardStore();
const compareOpen = ref(false);

function refresh() {
  if (workspaces.currentId) board.load(workspaces.currentId);
}

onMounted(refresh);
// The board is not loaded by App.vue's watch (it runs git); load it here and
// whenever the workspace changes while this view is open.
watch(() => workspaces.currentId, refresh);

const byStage = (stage: string): BoardCard[] => board.cards.filter((c) => c.stage === stage);

function healthTone(health: string): string | undefined {
  if (health === "clean") return "success";
  if (health === "dirty") return "warning";
  return undefined; // unknown
}
function agentTone(status: string | null): string | undefined {
  if (status === "running") return "info";
  if (status === "failed") return "danger";
  return undefined;
}
</script>

<template>
  <section class="board" data-testid="board">
    <header class="board__head">
      <h2 class="view-title">Board</h2>
      <span class="board__actions">
        <button
          type="button"
          class="re-button"
          data-variant="secondary"
          data-size="sm"
          data-testid="board-compare"
          @click="compareOpen = true"
        >
          Compare reviews
        </button>
        <button
          type="button"
          class="re-button"
          data-variant="ghost"
          data-size="sm"
          @click="refresh"
        >
          Refresh
        </button>
      </span>
    </header>

    <p v-if="board.loading" class="muted">Loading board…</p>
    <p v-else-if="board.error" class="error">{{ board.error }}</p>
    <p v-else-if="board.cards.length === 0" class="muted">
      No coding work yet — dispatch from an artifact or create a worktree in Coding.
    </p>
    <div v-else class="columns">
      <div
        v-for="col in BOARD_STAGES"
        :key="col.key"
        class="column"
        data-testid="board-column"
        :data-stage="col.key"
      >
        <h3 class="column__title">{{ col.label }} ({{ byStage(col.key).length }})</h3>
        <ul class="cards">
          <li
            v-for="c in byStage(col.key)"
            :key="c.coding_workspace_id"
            class="re-card card"
            data-testid="board-card"
          >
            <span class="card__branch">{{ c.branch_name }}</span>
            <span class="card__meta">{{ c.project_name }} · {{ c.repo_name }}</span>
            <span class="card__badges">
              <span class="re-badge" :data-tone="healthTone(c.health)">
                {{
                  c.health === "unknown"
                    ? "health: unknown"
                    : c.is_clean
                      ? "clean"
                      : `${c.changed_files} changed`
                }}
              </span>
              <span
                v-if="c.latest_review_status"
                class="re-badge"
                :data-tone="reviewTone(c.latest_review_status)"
              >
                {{ c.latest_review_status }}
              </span>
              <span v-if="c.agent_status" class="re-badge" :data-tone="agentTone(c.agent_status)">
                agent: {{ c.agent_status }}
              </span>
            </span>
          </li>
        </ul>
      </div>
    </div>

    <ReviewCompareDialog :open="compareOpen" @close="compareOpen = false" />
  </section>
</template>

<style scoped>
.board {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  height: 100%;
  min-height: 0;
}
.board__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.view-title {
  margin: 0;
  font-size: 1.2rem;
}
.board__actions {
  display: flex;
  gap: 0.35rem;
}
.columns {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 0.75rem;
  align-items: start;
  flex: 1;
  min-height: 0;
  overflow: auto;
}
.column {
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
  background: var(--re-color-bg-muted);
  border-radius: var(--re-radius-md, 6px);
  padding: 0.5rem;
}
.column__title {
  margin: 0;
  font-size: 0.8rem;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--re-color-text-muted);
}
.cards {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}
.card {
  display: flex;
  flex-direction: column;
  gap: 0.2rem;
  padding: 0.55rem 0.7rem;
}
.card__branch {
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.card__meta {
  font-size: 0.75rem;
  color: var(--re-color-text-muted);
}
.card__badges {
  display: flex;
  flex-wrap: wrap;
  gap: 0.3rem;
  margin-top: 0.2rem;
}
.muted {
  color: var(--re-color-text-muted);
}
.error {
  color: var(--re-color-danger-text);
}
</style>
