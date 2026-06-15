export interface CodingWorkspace {
  id: string;
  workspace_id: string;
  project_id: string;
  repository_source_id: string;
  session_id: string | null;
  repo_path: string;
  worktree_path: string;
  branch_name: string;
  base_branch: string;
  status: string;
  created_at: string;
  updated_at: string;
}

/** Working-tree changes inside a coding worktree. */
export interface WorktreeDiff {
  changed_files: string[];
  diff_stat: string;
  diff_text: string;
  is_clean: boolean;
  error: string | null;
}
