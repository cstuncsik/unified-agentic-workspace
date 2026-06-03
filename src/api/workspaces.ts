import { invoke } from "@tauri-apps/api/core";
import type { Workspace } from "../types/workspace";

export function listWorkspaces(): Promise<Workspace[]> {
  return invoke<Workspace[]>("list_workspaces");
}

export function getWorkspace(id: string): Promise<Workspace | null> {
  return invoke<Workspace | null>("get_workspace", { id });
}

export function createWorkspace(name: string, kind?: string): Promise<Workspace> {
  return invoke<Workspace>("create_workspace", { name, kind });
}

export function updateWorkspace(
  id: string,
  name: string,
  kind?: string,
): Promise<Workspace | null> {
  return invoke<Workspace | null>("update_workspace", { id, name, kind });
}

export function deleteWorkspace(id: string): Promise<boolean> {
  return invoke<boolean>("delete_workspace", { id });
}
