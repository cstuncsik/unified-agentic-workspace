<script setup lang="ts">
import { computed, onUnmounted, ref } from "vue";
import { useWorkspacesStore } from "../stores/workspaces";
import { useProviderAccountsStore } from "../stores/providerAccounts";
import { useToast } from "../composables/useToast";
import { useConfirm } from "../composables/useConfirm";

const workspaces = useWorkspacesStore();
const accounts = useProviderAccountsStore();
const toast = useToast();
const { confirm } = useConfirm();

const provider = ref("anthropic");
const displayName = ref("");
const apiKey = ref("");
const submitting = ref(false);

const canAdd = computed(() => displayName.value.trim() !== "" && apiKey.value.trim() !== "");

function clearKey() {
  apiKey.value = "";
}
onUnmounted(clearKey);

async function add() {
  const name = displayName.value.trim();
  const key = apiKey.value.trim();
  if (!name || !key || !workspaces.currentId) return;
  submitting.value = true;
  try {
    await accounts.create({
      workspaceId: workspaces.currentId,
      provider: provider.value,
      displayName: name,
      apiKey: key,
    });
    displayName.value = "";
    toast.success("Account added");
  } catch (e) {
    toast.error(String(e));
  } finally {
    // Clear the secret on success AND failure.
    clearKey();
    submitting.value = false;
  }
}

async function removeAccount(id: string, name: string) {
  const confirmed = await confirm(
    `Remove account "${name}"? Its stored API key is deleted from the keychain.`,
    "Remove account",
    "Remove",
  );
  if (!confirmed) return;
  try {
    await accounts.remove(id);
    toast.success("Account removed");
  } catch (e) {
    toast.error(String(e));
  }
}

const providerLabel = (p: string) =>
  p === "anthropic" ? "Anthropic" : p === "openai" ? "OpenAI" : p;
</script>

<template>
  <section>
    <h2 class="view-title">Providers</h2>
    <h3 class="section-title">API Key Accounts</h3>

    <form class="attach" @submit.prevent="add">
      <select v-model="provider" class="re-select" aria-label="Provider">
        <option value="anthropic">Anthropic</option>
        <option value="openai">OpenAI</option>
      </select>
      <input
        v-model="displayName"
        class="re-input"
        type="text"
        placeholder="Account name"
        aria-label="Account display name"
      />
      <input
        v-model="apiKey"
        class="re-input attach__key"
        type="password"
        autocomplete="new-password"
        placeholder="API key"
        aria-label="API key"
      />
      <button
        class="re-button"
        data-variant="brand"
        type="submit"
        :disabled="submitting || !canAdd"
      >
        Add account
      </button>
    </form>

    <p v-if="accounts.loading" class="muted">Loading accounts…</p>
    <p v-else-if="accounts.error" class="error">{{ accounts.error }}</p>
    <p v-else-if="accounts.list.length === 0" class="muted">
      No provider accounts yet — add one to use API-based agents.
    </p>
    <ul v-else class="rows">
      <li
        v-for="account in accounts.list"
        :key="account.id"
        class="re-card"
        data-testid="provider-row"
      >
        <span class="acct__main">
          <span class="acct__name">{{ account.display_name }}</span>
          <span class="acct__meta">
            {{ providerLabel(account.provider) }} · {{ account.auth_mode }}
          </span>
        </span>
        <button
          type="button"
          class="re-button"
          data-variant="danger"
          data-size="sm"
          @click="removeAccount(account.id, account.display_name)"
        >
          Remove
        </button>
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

.attach {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
  margin-bottom: 0.75rem;
}

.attach__key {
  flex: 1;
  min-width: 16rem;
}

.rows {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}

.rows .re-card {
  display: flex;
  flex-direction: row;
  align-items: center;
  gap: 0.6rem;
  padding: 0.6rem 0.85rem;
}

.acct__main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 0.2rem;
}

.acct__meta {
  font-size: 0.75rem;
  color: var(--re-color-text-muted);
}

.muted {
  color: var(--re-color-text-muted);
}

.error {
  color: var(--re-color-danger-text);
}
</style>
