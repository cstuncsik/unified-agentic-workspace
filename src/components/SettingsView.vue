<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useAppConfigStore } from "../stores/appConfig";
import { useToast } from "../composables/useToast";
import type { EditConfig } from "../types/appConfig";

const appConfig = useAppConfigStore();
const toast = useToast();

const AGENTS = [
  { id: "claude-code", label: "Claude Code" },
  { id: "codex", label: "Codex" },
  { id: "gemini", label: "Gemini" },
] as const;

const bins = ref<Record<string, string>>({});
const argsText = ref<Record<string, string>>({});
const fontSize = ref(13);
const submitting = ref(false);
const loaded = ref(false);

onMounted(async () => {
  const cfg = await appConfig.getForEdit();
  for (const { id } of AGENTS) {
    bins.value[id] = cfg.agents[id]?.bin ?? "";
    argsText.value[id] = (cfg.agents[id]?.args ?? []).join("\n");
  }
  fontSize.value = cfg.fontSize;
  if (cfg.warning) toast.error(cfg.warning);
  loaded.value = true;
});

async function save() {
  if (!Number.isFinite(fontSize.value)) {
    toast.error("Font size must be 6–72.");
    return;
  }
  const edits: EditConfig = { agents: {}, fontSize: fontSize.value };
  for (const { id } of AGENTS) {
    const bin = bins.value[id]?.trim();
    edits.agents[id] = {
      bin: bin ? bin : null,
      args: (argsText.value[id] ?? "")
        .split("\n")
        .map((s) => s.trim())
        .filter(Boolean),
    };
  }
  submitting.value = true;
  const res = await appConfig.save(edits);
  submitting.value = false;
  if (res.ok) toast.success("Settings saved.");
  else toast.error(res.error ?? "Save failed.");
}
</script>

<template>
  <section data-testid="settings-view">
    <h1 class="view-title">Settings</h1>
    <p v-if="!loaded" class="muted">Loading settings…</p>
    <form v-else class="settings" @submit.prevent="save">
      <fieldset v-for="agent in AGENTS" :key="agent.id" class="settings__agent">
        <legend>{{ agent.label }}</legend>
        <label class="settings__field">
          <span>Binary</span>
          <input
            v-model="bins[agent.id]"
            class="re-input"
            type="text"
            :placeholder="agent.id"
            :aria-label="`${agent.label} binary`"
            :data-testid="`bin-${agent.id}`"
          />
        </label>
        <label class="settings__field">
          <span>Args (one per line)</span>
          <textarea
            v-model="argsText[agent.id]"
            class="re-input"
            rows="3"
            :aria-label="`${agent.label} args`"
            :data-testid="`args-${agent.id}`"
          ></textarea>
        </label>
      </fieldset>

      <label class="settings__field">
        <span>Terminal font size</span>
        <input
          v-model.number="fontSize"
          class="re-input"
          type="number"
          min="6"
          max="72"
          aria-label="Terminal font size"
          data-testid="font-size"
        />
      </label>

      <button
        class="re-button"
        data-variant="brand"
        type="submit"
        :disabled="submitting"
        data-testid="settings-save"
      >
        Save
      </button>
      <p class="muted settings__note">Terminal colours: edit <code>theme</code> in config.json.</p>
    </form>
  </section>
</template>

<style scoped>
.view-title {
  margin: 0 0 0.25rem;
  font-size: 1.2rem;
}
.settings {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  max-width: 40rem;
}
.settings__agent {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  border: 1px solid var(--re-color-border);
  border-radius: var(--re-radius-md, 6px);
  padding: 0.75rem;
}
.settings__field {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}
.settings__note {
  font-size: 0.8rem;
}
</style>
