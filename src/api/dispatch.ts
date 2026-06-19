import { invoke } from "@tauri-apps/api/core";
import type { Session } from "../types/session";

export interface DispatchTask {
  title: string;
  branch_name: string;
  include: boolean;
}
export interface DispatchTaskResult {
  title: string;
  session_id: string;
  coding_workspace_id: string | null;
  error: string | null;
}

export function extractArtifactTasks(artifactId: string): Promise<string[]> {
  return invoke<string[]>("extract_artifact_tasks", { artifactId });
}

export function listArtifactSessions(workspaceId: string, artifactId: string): Promise<Session[]> {
  return invoke<Session[]>("list_artifact_sessions", { workspaceId, artifactId });
}

export function dispatchArtifact(
  artifactId: string,
  projectId: string,
  repositorySourceId: string,
  baseBranch: string,
  tasks: DispatchTask[],
): Promise<{ results: DispatchTaskResult[] }> {
  return invoke("dispatch_artifact", {
    artifactId,
    projectId,
    repositorySourceId,
    baseBranch,
    tasks,
  });
}
