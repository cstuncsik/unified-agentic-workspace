import { showToast } from "@relements/core/behaviors/toast";

export function useToast() {
  return {
    success: (message: string) => showToast(message, { tone: "success" }),
    error: (message: string) => showToast(message, { tone: "danger" }),
  };
}
