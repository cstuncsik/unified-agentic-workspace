import { invoke } from "@tauri-apps/api/core";
import type { BoardCard } from "../types/board";

export function getBoard(workspaceId: string): Promise<BoardCard[]> {
  return invoke<BoardCard[]>("get_board", { workspaceId });
}
