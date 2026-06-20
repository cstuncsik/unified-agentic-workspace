<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useAgentSessionsStore } from "../stores/agentSessions";
import type { SdkEvent } from "../types/agentSession";

const props = defineProps<{ sessionId: string }>();
const store = useAgentSessionsStore();
const events = computed(() => store.sdkEvents[props.sessionId] ?? []);
onMounted(() => store.loadSdkTranscript(props.sessionId));

const tag = (e: SdkEvent) =>
  e.type === "tool" ? `🔧 ${e.name ?? "tool"}` : e.type === "result" ? "✓" : e.type === "error" ? "✗" : "";
const text = (e: SdkEvent) => e.text ?? e.summary ?? e.message ?? "";
</script>

<template>
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
</template>

<style scoped>
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
.muted {
  color: var(--re-color-text-muted);
}
</style>
