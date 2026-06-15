import { invoke } from "@tauri-apps/api/core";
import type { CodingWorkspace, WorktreeDiff } from "../types/codingWorkspace";

export function listCodingWorkspaces(workspaceId: string): Promise<CodingWorkspace[]> {
  return invoke<CodingWorkspace[]>("list_coding_workspaces", { workspaceId });
}

export function getCodingWorkspace(id: string): Promise<CodingWorkspace | null> {
  return invoke<CodingWorkspace | null>("get_coding_workspace", { id });
}

export interface CreateCodingWorkspaceInput {
  projectId: string;
  repositorySourceId: string;
  baseBranch: string;
  branchName: string;
}

export function createCodingWorkspace(input: CreateCodingWorkspaceInput): Promise<CodingWorkspace> {
  return invoke<CodingWorkspace>("create_coding_workspace", { ...input });
}

export function getCodingWorkspaceDiff(id: string): Promise<WorktreeDiff> {
  return invoke<WorktreeDiff>("get_coding_workspace_diff", { id });
}

export function markCodingWorkspaceReadyForReview(id: string): Promise<CodingWorkspace | null> {
  return invoke<CodingWorkspace | null>("mark_coding_workspace_ready_for_review", { id });
}

export function discardCodingWorkspace(id: string, force: boolean): Promise<boolean> {
  return invoke<boolean>("discard_coding_workspace", { id, force });
}
