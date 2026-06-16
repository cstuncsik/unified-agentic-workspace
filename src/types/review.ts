export interface Review {
  id: string;
  workspace_id: string;
  coding_workspace_id: string;
  status: string;
  summary: string;
  status_short: string;
  diff_stat: string;
  files: string[];
  test_command: string | null;
  test_output: string;
  risk_notes: string[];
  created_at: string;
  updated_at: string;
}

/** Verdict states, in display order. Mirrors REVIEW_STATUSES in the backend. */
export const REVIEW_STATUSES = [
  "pending",
  "approved",
  "rejected",
  "changes-requested",
  "done",
] as const;

/** Actions the user can take on a review, with the status they set. */
export const REVIEW_ACTIONS: ReadonlyArray<{ status: string; label: string }> = [
  { status: "approved", label: "Approve" },
  { status: "rejected", label: "Reject" },
  { status: "changes-requested", label: "Request changes" },
  { status: "done", label: "Mark done" },
];
