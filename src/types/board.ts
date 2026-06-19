export interface BoardCard {
  coding_workspace_id: string;
  branch_name: string;
  base_branch: string;
  project_name: string;
  repo_name: string;
  status: string;
  latest_review_status: string | null;
  agent_status: string | null;
  last_activity: string;
  stage: string; // "in-progress" | "needs-review" | "reviewed"
  is_clean: boolean;
  changed_files: number;
  health: string; // "clean" | "dirty" | "unknown"
}

export const BOARD_STAGES: ReadonlyArray<{ key: string; label: string }> = [
  { key: "in-progress", label: "In progress" },
  { key: "needs-review", label: "Needs review" },
  { key: "reviewed", label: "Reviewed" },
];
