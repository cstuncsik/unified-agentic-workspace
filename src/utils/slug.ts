/** Git-ref-safe branch slug, mirroring the backend services/dispatch.rs::slugify_branch. */
export function slugifyBranch(title: string): string {
  return title
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}
