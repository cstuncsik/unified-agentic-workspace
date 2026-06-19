import { invoke } from "@tauri-apps/api/core";
import type { Artifact } from "../types/artifact";

export function listArtifacts(workspaceId: string): Promise<Artifact[]> {
  return invoke<Artifact[]>("list_artifacts", { workspaceId });
}

export function createArtifact(
  workspaceId: string,
  projectId: string | null,
  title: string,
): Promise<Artifact> {
  return invoke<Artifact>("create_artifact", { workspaceId, projectId, title });
}

export function updateArtifact(
  id: string,
  title: string,
  content: string,
): Promise<Artifact | null> {
  return invoke<Artifact | null>("update_artifact", { id, title, content });
}

export function deleteArtifact(id: string): Promise<boolean> {
  return invoke<boolean>("delete_artifact", { id });
}
