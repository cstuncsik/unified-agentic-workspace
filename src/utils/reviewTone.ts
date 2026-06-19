/** Map a review status to a renascent badge data-tone. Shared by Reviews + Board. */
export function reviewTone(status: string): string | undefined {
  if (status === "approved" || status === "done") return "success";
  if (status === "rejected") return "danger";
  if (status === "changes-requested") return "warning";
  return "info"; // pending
}
