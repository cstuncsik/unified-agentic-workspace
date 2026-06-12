export type SessionMode = "research" | "document" | "code" | "review" | "terminal";

export const SESSION_MODES: SessionMode[] = ["research", "document", "code", "review", "terminal"];

export type SessionStatus =
  | "backlog"
  | "todo"
  | "running"
  | "worktree-created"
  | "agent-running"
  | "tests-running"
  | "review-agent-running"
  | "needs-review"
  | "done"
  | "merged"
  | "discarded"
  | "cancelled"
  | "archived"
  | "flagged";

export const SESSION_STATUSES: SessionStatus[] = [
  "backlog",
  "todo",
  "running",
  "worktree-created",
  "agent-running",
  "tests-running",
  "review-agent-running",
  "needs-review",
  "done",
  "merged",
  "discarded",
  "cancelled",
  "archived",
  "flagged",
];

export interface Session {
  id: string;
  workspace_id: string;
  project_id: string | null;
  title: string;
  mode: SessionMode;
  status: SessionStatus;
  summary: string | null;
  permissions_json: string;
  context_refs_json: string;
  created_at: string;
  updated_at: string;
}

export interface StatusGroup {
  key: string;
  label: string;
  statuses: SessionStatus[];
}

/** Sidebar/inbox grouping per the PRD navigation model. */
export const STATUS_GROUPS: StatusGroup[] = [
  { key: "backlog", label: "Backlog", statuses: ["backlog"] },
  { key: "todo", label: "Todo", statuses: ["todo"] },
  {
    key: "running",
    label: "Running",
    statuses: [
      "running",
      "worktree-created",
      "agent-running",
      "tests-running",
      "review-agent-running",
    ],
  },
  { key: "needs-review", label: "Needs Review", statuses: ["needs-review"] },
  { key: "done", label: "Done", statuses: ["done", "merged"] },
  { key: "cancelled", label: "Cancelled", statuses: ["cancelled", "discarded"] },
  { key: "archived", label: "Archived", statuses: ["archived"] },
  { key: "flagged", label: "Flagged", statuses: ["flagged"] },
];
