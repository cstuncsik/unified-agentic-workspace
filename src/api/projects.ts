import { invoke } from "@tauri-apps/api/core";
import type { Project, ProjectMode } from "../types/project";

export function listProjects(workspaceId: string): Promise<Project[]> {
  return invoke<Project[]>("list_projects", { workspaceId });
}

export function getProject(id: string): Promise<Project | null> {
  return invoke<Project | null>("get_project", { id });
}

export function createProject(
  workspaceId: string,
  name: string,
  mode?: ProjectMode,
): Promise<Project> {
  return invoke<Project>("create_project", { workspaceId, name, mode });
}

export function updateProject(
  id: string,
  name: string,
  mode?: ProjectMode,
): Promise<Project | null> {
  return invoke<Project | null>("update_project", { id, name, mode });
}

export function deleteProject(id: string): Promise<boolean> {
  return invoke<boolean>("delete_project", { id });
}

export function setProjectTestCommand(
  id: string,
  testCommand: string | null,
): Promise<Project | null> {
  return invoke<Project | null>("set_project_test_command", { id, testCommand });
}
