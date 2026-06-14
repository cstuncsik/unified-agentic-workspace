export interface RepositorySource {
  id: string;
  workspace_id: string;
  project_id: string | null;
  name: string;
  local_path: string;
  default_branch: string;
  enabled: boolean;
  created_at: string;
  updated_at: string;
}

/** Result of inspecting a path with git (validation + live status). */
export interface GitInspection {
  is_git_repo: boolean;
  current_branch: string | null;
  default_branch: string | null;
  is_dirty: boolean;
  toplevel: string | null;
  error: string | null;
}
