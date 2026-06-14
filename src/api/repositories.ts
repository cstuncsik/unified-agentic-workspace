import { invoke } from "@tauri-apps/api/core";
import type { GitInspection, RepositorySource } from "../types/repository";

export function validateRepositoryPath(path: string): Promise<GitInspection> {
  return invoke<GitInspection>("validate_repository_path", { path });
}

export function listRepositorySources(workspaceId: string): Promise<RepositorySource[]> {
  return invoke<RepositorySource[]>("list_repository_sources", { workspaceId });
}

export function getRepositorySource(id: string): Promise<RepositorySource | null> {
  return invoke<RepositorySource | null>("get_repository_source", { id });
}

export interface CreateRepositoryInput {
  workspaceId: string;
  name: string;
  localPath: string;
  projectId?: string;
}

export function createRepositorySource(input: CreateRepositoryInput): Promise<RepositorySource> {
  return invoke<RepositorySource>("create_repository_source", { ...input });
}

export function getRepositoryStatus(id: string): Promise<GitInspection> {
  return invoke<GitInspection>("get_repository_status", { id });
}

export function listRepositoryBranches(id: string): Promise<string[]> {
  return invoke<string[]>("list_repository_branches", { id });
}

export function deleteRepositorySource(id: string): Promise<boolean> {
  return invoke<boolean>("delete_repository_source", { id });
}
