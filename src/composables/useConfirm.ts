import { ref } from "vue";

const open = ref(false);
const title = ref("Confirm");
const message = ref("");
const confirmLabel = ref("Delete");
let resolver: ((value: boolean) => void) | null = null;

export function useConfirm() {
  function confirm(msg: string, dialogTitle = "Confirm", label = "Delete"): Promise<boolean> {
    // Settle any in-flight confirm as cancelled before starting a new one,
    // so its awaiter never hangs if a second confirm opens first.
    resolver?.(false);
    message.value = msg;
    title.value = dialogTitle;
    confirmLabel.value = label;
    open.value = true;
    return new Promise((resolve) => {
      resolver = resolve;
    });
  }

  function settle(value: boolean) {
    open.value = false;
    resolver?.(value);
    resolver = null;
  }

  return { open, title, message, confirmLabel, confirm, settle };
}
