<script setup lang="ts">
import { ref, watch } from "vue";
import { useConfirm } from "../composables/useConfirm";

const { open, title, message, confirmLabel, settle } = useConfirm();
const dialog = ref<HTMLDialogElement | null>(null);

watch(open, (isOpen) => {
  const el = dialog.value;
  if (!el) return;
  if (isOpen && !el.open) el.showModal();
  if (!isOpen && el.open) el.close();
});

// Backdrop/Esc close resolves as cancel.
function onClose() {
  if (open.value) settle(false);
}
</script>

<template>
  <dialog ref="dialog" class="re-dialog" data-testid="confirm-dialog" @close="onClose">
    <header class="re-dialog__header">
      <h2 class="re-dialog__title">{{ title }}</h2>
    </header>
    <div class="re-dialog__body">
      <p>{{ message }}</p>
    </div>
    <div class="re-dialog__footer">
      <button type="button" class="re-button" data-variant="ghost" @click="settle(false)">
        Cancel
      </button>
      <button type="button" class="re-button" data-variant="danger" @click="settle(true)">
        {{ confirmLabel }}
      </button>
    </div>
  </dialog>
</template>
