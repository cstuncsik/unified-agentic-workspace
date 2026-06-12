export type ProjectMode = "research" | "code" | "mixed";

export const PROJECT_MODES: ProjectMode[] = ["research", "code", "mixed"];

export interface Project {
  id: string;
  workspace_id: string;
  name: string;
  mode: ProjectMode;
  settings_json: string;
  created_at: string;
  updated_at: string;
}
