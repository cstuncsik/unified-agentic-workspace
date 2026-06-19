export interface Artifact {
  id: string;
  workspace_id: string;
  project_id: string | null;
  title: string;
  content: string;
  created_at: string;
  updated_at: string;
}
