<script setup lang="ts">
import { ref, watch } from "vue";
import { useConfirm } from "../composables/useConfirm";

const { open, title, message, settle } = useConfirm();
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
  <dialog ref="dialog" class="re-dialog" @close="onClose">
    <div class="re-dialog__body">
      <h2>{{ title }}</h2>
      <p>{{ message }}</p>
      <div class="re-dialog__footer">
        <button type="button" class="re-button" data-variant="ghost" @click="settle(false)">
          Cancel
        </button>
        <button type="button" class="re-button" data-variant="danger" @click="settle(true)">
          Delete
        </button>
      </div>
    </div>
  </dialog>
</template>
